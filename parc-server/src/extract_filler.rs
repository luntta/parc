//! Background attachment text extraction in a long-running server: pending
//! attachments (e.g. brought in by a `git pull` or an outside edit) are
//! extracted in a low-priority background thread with a lease so two servers on
//! the same vault don't duplicate work.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::Duration;

use parc_core::{extract, index};

const SETTLE: Duration = Duration::from_secs(1);
const ROUND: usize = 8;
const CLAIM_SECONDS: u64 = 120;

/// Background thread that extracts pending attachment text.
pub struct ExtractFiller {
    nudge: SyncSender<()>,
}

impl ExtractFiller {
    pub fn spawn(vault: PathBuf) -> Self {
        let (nudge, nudges) = mpsc::sync_channel(1);
        let _ = nudge.try_send(());
        std::thread::Builder::new()
            .name("parc-extract".into())
            .spawn(move || run(&vault, nudges))
            .expect("start the attachment extract thread");
        Self { nudge }
    }

    pub fn nudge(&self) {
        let _ = self.nudge.try_send(());
    }
}

fn run(vault: &Path, nudges: Receiver<()>) {
    lower_priority();
    let holder = format!("parc-extract:{}:{:x}", std::process::id(), rand_tag());
    while nudges.recv().is_ok() {
        std::thread::sleep(SETTLE);
        while nudges.try_recv().is_ok() {}
        if let Err(e) = fill(vault, &holder) {
            eprintln!("parc-server: extract: {e}");
        }
    }
}

fn fill(vault: &Path, holder: &str) -> Result<(), parc_core::ParcError> {
    let conn = index::open_index(vault)?;
    if !extract::has_pending(&conn)? {
        return Ok(());
    }
    let result = (|| loop {
        if !extract::claim_lease(&conn, holder, CLAIM_SECONDS)? {
            return Ok(());
        }
        let report = extract::store::fill_limit(vault, &conn, Some(ROUND), false)?;
        if report.remaining == 0 {
            return Ok(());
        }
    })();
    let _ = extract::release_lease(&conn, holder);
    result
}

fn lower_priority() {
    #[cfg(target_os = "linux")]
    // SAFETY: plain system calls on the calling thread.
    unsafe {
        let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
        libc::setpriority(libc::PRIO_PROCESS, tid, 10);
    }
}

fn rand_tag() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(1);
    h.finish()
}
