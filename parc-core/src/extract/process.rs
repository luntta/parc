//! Child-process isolation for untrusted document extraction.
//!
//! Spawns `std::env::current_exe()` with [`CHILD_ARG`]. Both `parc` and
//! `parc-server` check for [`CHILD_ARG`] at the very start of `main()` and hand
//! off to [`child_main`]. The child sets `RLIMIT_AS` and `RLIMIT_CPU`, receives
//! the request header + raw file bytes on stdin (never a filesystem path), and
//! writes the JSON result on stdout. The parent enforces a wall-clock deadline
//! and serializes extractions one at a time per process.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::ParcError;
use crate::extract::{in_process, ExtractLimits, ExtractOutcome};

/// Hidden first argument that switches `parc` / `parc-server` into the
/// isolated extractor worker before normal CLI parsing runs.
pub const CHILD_ARG: &str = "--parc-internal-extract";

/// Maximum JSON response size read from the child stdout (64 MiB).
const MAX_STDOUT_BYTES: usize = 64 * 1024 * 1024;

/// Serialize extractions one at a time per process so a folder of 50 PDFs never
/// spawns 50 child processes simultaneously.
static EXTRACT_QUEUE: Mutex<()> = Mutex::new(());

#[derive(Debug, Serialize, Deserialize)]
struct ChildRequest {
    filename: Option<String>,
    limits: ExtractLimits,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum ChildResponse {
    Ok { outcome: ExtractOutcome },
    Err { message: String },
}

/// Extract text from `path` inside a bounded child process of the current binary.
pub fn isolated(path: &Path, limits: &ExtractLimits) -> Result<ExtractOutcome, ParcError> {
    let exe = std::env::current_exe().map_err(|e| {
        ParcError::ValidationError(format!("cannot locate parc binary for extraction: {e}"))
    })?;
    isolated_with_exe(&exe, path, limits)
}

/// Like [`isolated`], invoking a specific parc binary (used by integration tests
/// where `current_exe()` is the test harness rather than `parc`).
pub fn isolated_with_exe(
    exe: &Path,
    path: &Path,
    limits: &ExtractLimits,
) -> Result<ExtractOutcome, ParcError> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(ParcError::ValidationError(format!(
            "refusing to extract symlink '{}'",
            path.display()
        )));
    }
    if !meta.is_file() {
        return Err(ParcError::ValidationError(format!(
            "not a regular file: {}",
            path.display()
        )));
    }
    if meta.len() > limits.max_file_bytes as u64 {
        return Err(ParcError::ValidationError(format!(
            "too large ({} bytes; limit {} bytes)",
            meta.len(),
            limits.max_file_bytes
        )));
    }

    // Read up to max_file_bytes + 1 so a growing file cannot slip past the cap.
    let mut file = std::fs::File::open(path)?;
    let mut bytes = Vec::with_capacity(meta.len().min(limits.max_file_bytes as u64) as usize);
    Read::by_ref(&mut file)
        .take((limits.max_file_bytes as u64) + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limits.max_file_bytes {
        return Err(ParcError::ValidationError(format!(
            "too large (exceeds {} bytes)",
            limits.max_file_bytes
        )));
    }

    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string);

    let _guard = EXTRACT_QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    run_child(exe, bytes, filename, limits)
}

fn run_child(
    exe: &Path,
    bytes: Vec<u8>,
    filename: Option<String>,
    limits: &ExtractLimits,
) -> Result<ExtractOutcome, ParcError> {
    let header = serde_json::to_string(&ChildRequest {
        filename,
        limits: limits.clone(),
    })?;

    let mut child = Command::new(exe)
        .arg(CHILD_ARG)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ParcError::ValidationError(format!("could not start extractor: {e}")))?;

    let mut stdin = child.stdin.take().expect("stdin piped");
    let mut stdout = child.stdout.take().expect("stdout piped");
    let mut stderr = child.stderr.take().expect("stderr piped");

    // Feed stdin on a background thread so large payloads can't deadlock against stdout.
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(header.as_bytes());
        let _ = stdin.write_all(b"\n");
        let _ = stdin.write_all(&bytes);
        let _ = stdin.flush();
    });

    let stdout_reader = std::thread::spawn(move || -> std::io::Result<Vec<u8>> {
        let mut out = Vec::new();
        stdout
            .by_ref()
            .take((MAX_STDOUT_BYTES as u64) + 1)
            .read_to_end(&mut out)?;
        Ok(out)
    });

    let stderr_reader = std::thread::spawn(move || -> String {
        let mut buf = Vec::new();
        let _ = stderr.by_ref().take(4096).read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).trim().to_string()
    });

    let deadline = Instant::now() + Duration::from_secs(limits.wall_timeout_secs.max(1));
    let status = loop {
        match child.try_wait()? {
            Some(status) => break status,
            None => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = writer.join();
                    let _ = stdout_reader.join();
                    let _ = stderr_reader.join();
                    return Err(ParcError::ValidationError("took too long".into()));
                }
                std::thread::sleep(Duration::from_millis(15));
            }
        }
    };

    let _ = writer.join();
    let out_bytes = stdout_reader
        .join()
        .map_err(|_| ParcError::ValidationError("unreadable (extractor I/O failed)".into()))??;
    let err_text = stderr_reader.join().unwrap_or_default();

    if out_bytes.len() > MAX_STDOUT_BYTES {
        return Err(ParcError::ValidationError("too large".into()));
    }

    if !status.success() {
        return Err(ParcError::ValidationError(classify_exit(status, &err_text)));
    }

    let response: ChildResponse = serde_json::from_slice(&out_bytes).map_err(|_| {
        ParcError::ValidationError("unreadable (malformed extractor response)".into())
    })?;

    match response {
        ChildResponse::Ok { outcome } => Ok(outcome),
        ChildResponse::Err { message } => Err(ParcError::ValidationError(message)),
    }
}

fn classify_exit(status: std::process::ExitStatus, stderr: &str) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return match sig {
                libc::SIGXCPU => "took too long".to_string(),
                libc::SIGKILL | libc::SIGABRT => "too large or ran out of memory".to_string(),
                _ => format!("unreadable (extractor crashed with signal {sig})"),
            };
        }
    }
    let detail = stderr
        .lines()
        .last()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match detail {
        Some(msg)
            if msg.to_lowercase().contains("memory") || msg.to_lowercase().contains("alloc") =>
        {
            "too large or ran out of memory".to_string()
        }
        Some(msg) => format!("unreadable ({msg})"),
        None => format!("unreadable (extractor exited with {status})"),
    }
}

/// Entry point executed inside the isolated child process when invoked with
/// [`CHILD_ARG`]. Reads request + bytes on stdin, applies resource limits,
/// extracts text, writes JSON to stdout, and exits.
pub fn child_main() -> ! {
    let exit_code = match run_child_main() {
        Ok(()) => 0,
        Err(msg) => {
            let resp = ChildResponse::Err {
                message: msg.clone(),
            };
            if let Ok(json) = serde_json::to_string(&resp) {
                let _ = writeln!(std::io::stdout(), "{json}");
                let _ = std::io::stdout().flush();
                0
            } else {
                eprintln!("{msg}");
                1
            }
        }
    };
    std::process::exit(exit_code);
}

fn run_child_main() -> Result<(), String> {
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let mut header_line = String::new();
    let n = reader
        .read_line(&mut header_line)
        .map_err(|e| format!("failed to read request header: {e}"))?;
    if n == 0 {
        return Err("empty extraction request".into());
    }
    let req: ChildRequest = serde_json::from_str(header_line.trim())
        .map_err(|e| format!("invalid extraction request: {e}"))?;

    apply_resource_limits(&req.limits)
        .map_err(|e| format!("cannot enforce extractor resource limits: {e}"))?;

    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take((req.limits.max_file_bytes as u64) + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("failed to read input bytes: {e}"))?;

    let outcome =
        in_process(&bytes, req.filename.as_deref(), &req.limits).map_err(|e| e.to_string())?;
    let json = serde_json::to_string(&ChildResponse::Ok { outcome })
        .map_err(|e| format!("failed to serialize extraction result: {e}"))?;
    let mut stdout = std::io::stdout().lock();
    stdout
        .write_all(json.as_bytes())
        .and_then(|_| stdout.write_all(b"\n"))
        .and_then(|_| stdout.flush())
        .map_err(|e| format!("failed to write extraction result: {e}"))?;
    Ok(())
}

#[cfg(unix)]
fn apply_resource_limits(limits: &ExtractLimits) -> std::io::Result<()> {
    #[cfg(target_os = "openbsd")]
    let memory = libc::RLIMIT_DATA; // OpenBSD has no RLIMIT_AS.
    #[cfg(not(target_os = "openbsd"))]
    let memory = libc::RLIMIT_AS;
    set_rlimit(memory, limits.max_memory_bytes)?;
    set_rlimit(libc::RLIMIT_CPU, limits.max_cpu_secs)
}

#[cfg(all(
    unix,
    any(
        all(target_os = "linux", any(target_env = "gnu", target_env = "uclibc")),
        target_os = "hurd"
    )
))]
type Resource = libc::__rlimit_resource_t;
#[cfg(all(
    unix,
    not(any(
        all(target_os = "linux", any(target_env = "gnu", target_env = "uclibc")),
        target_os = "hurd"
    ))
))]
type Resource = libc::c_int;

#[cfg(unix)]
fn set_rlimit(resource: Resource, limit: u64) -> std::io::Result<()> {
    if limit == 0 {
        return Ok(());
    }
    let mut cur = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: valid pointer to initialized rlimit struct.
    if unsafe { libc::getrlimit(resource, &mut cur) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // Never raise an inherited limit or wrap a large request on 32-bit targets.
    let target = libc::rlim_t::try_from(limit)
        .unwrap_or(libc::rlim_t::MAX)
        .min(cur.rlim_cur)
        .min(cur.rlim_max);
    let next = libc::rlimit {
        rlim_cur: target,
        rlim_max: target,
    };
    // SAFETY: both limits are <= the inherited hard limit.
    if unsafe { libc::setrlimit(resource, &next) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(unix))]
fn apply_resource_limits(_limits: &ExtractLimits) -> std::io::Result<()> {
    Ok(())
}
