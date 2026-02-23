use std::process::Command;

use parc_core::error::ParcError;
use parc_core::fragment::Fragment;
use parc_core::hook::{HookRunner, HookScript};

pub struct CliHookRunner;

impl HookRunner for CliHookRunner {
    fn run_pre_hook(
        &self,
        script: &HookScript,
        fragment: &Fragment,
    ) -> Result<Option<Fragment>, ParcError> {
        let json = serde_json::to_string(fragment).map_err(ParcError::Json)?;

        let output = Command::new(&script.path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(ref mut stdin) = child.stdin {
                    stdin.write_all(json.as_bytes())?;
                }
                child.wait_with_output()
            })
            .map_err(|e| {
                ParcError::Io(std::io::Error::new(
                    e.kind(),
                    format!("hook '{}': {}", script.path.display(), e),
                ))
            })?;

        if !output.status.success() {
            return Err(ParcError::ValidationError(format!(
                "pre-hook '{}' failed (exit {})",
                script.path.display(),
                output.status.code().unwrap_or(-1)
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stdout = stdout.trim();
        if stdout.is_empty() {
            return Ok(None);
        }

        // Try to parse stdout as modified fragment JSON
        let modified: Fragment =
            serde_json::from_str(stdout).map_err(|e| {
                ParcError::ParseError(format!(
                    "pre-hook '{}' produced invalid JSON: {}",
                    script.path.display(),
                    e
                ))
            })?;
        Ok(Some(modified))
    }

    fn run_post_hook(&self, script: &HookScript, fragment: &Fragment) -> Result<(), ParcError> {
        let json = serde_json::to_string(fragment).map_err(ParcError::Json)?;

        let output = Command::new(&script.path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::inherit())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(ref mut stdin) = child.stdin {
                    stdin.write_all(json.as_bytes())?;
                }
                child.wait_with_output()
            })
            .map_err(|e| {
                ParcError::Io(std::io::Error::new(
                    e.kind(),
                    format!("hook '{}': {}", script.path.display(), e),
                ))
            })?;

        if !output.status.success() {
            eprintln!(
                "warning: post-hook '{}' failed (exit {})",
                script.path.display(),
                output.status.code().unwrap_or(-1)
            );
        }

        Ok(())
    }
}
