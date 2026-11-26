//! The meaning model in a long-running server: vectors for new and edited
//! fragments are computed in the background, at low priority, so recall can
//! use them by the time anyone asks.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::Duration;

use parc_core::{embed, index};

/// How long the filler waits after a nudge for more to arrive, so a burst of
/// requests (typing, which asks for recall after every pause) wakes it once.
const SETTLE: Duration = Duration::from_secs(2);

/// Fragments embedded between checks for another process's claim.
const ROUND: usize = 64;

/// How long a claim lasts unless renewed: long enough for a round.
const CLAIM_SECONDS: u64 = 120;

/// The background thread that fills in vectors. Dropping it stops the
/// thread after its current round.
pub struct Filler {
    nudge: SyncSender<()>,
}

impl Filler {
    /// Start the thread for `vault`. It looks at once (the model may be
    /// installed and vectors missing), then whenever nudged.
    pub fn spawn(vault: PathBuf) -> Filler {
        let (nudge, nudges) = mpsc::sync_channel(1);
        let _ = nudge.try_send(());
        std::thread::Builder::new()
            .name("parc-meaning".into())
            .spawn(move || run(&vault, nudges))
            .expect("start the meaning thread");
        Filler { nudge }
    }

    /// Something may have changed: a write, or a new model.
    pub fn nudge(&self) {
        let _ = self.nudge.try_send(());
    }
}

fn run(vault: &Path, nudges: Receiver<()>) {
    lower_priority();
    let holder = format!("parc-server:{}:{:x}", std::process::id(), rand_tag());
    while nudges.recv().is_ok() {
        std::thread::sleep(SETTLE);
        while nudges.try_recv().is_ok() {}
        if let Err(e) = fill(vault, &holder) {
            eprintln!("parc-server: meaning: {e}");
        }
    }
}

fn fill(vault: &Path, holder: &str) -> Result<(), parc_core::ParcError> {
    let Some(model) = embed::shared()? else {
        return Ok(());
    };
    let threads = std::thread::available_parallelism().map_or(1, |n| (n.get() / 2).max(1));
    let conn = index::open_index(vault)?;
    let result = (|| loop {
        if !embed::claim(&conn, model.spec(), holder, CLAIM_SECONDS)? {
            return Ok(());
        }
        let report = embed::fill(&conn, &model, Some(ROUND), threads, &mut |_, _| {})?;
        if report.remaining == 0 {
            return Ok(());
        }
    })();
    embed::release(&conn, holder)?;
    result
}

/// Run below normal priority, so filling never slows the app. On Linux the
/// nice value is per thread and inherited by the threads this one starts.
fn lower_priority() {
    #[cfg(target_os = "linux")]
    // SAFETY: plain system calls on the calling thread.
    unsafe {
        let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
        libc::setpriority(libc::PRIO_PROCESS, tid, 10);
    }
}

/// A few bits that tell two threads of one process apart in a claim.
fn rand_tag() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(0);
    h.finish()
}
