//! Running a read-only query program and collecting what it printed.
//!
//! The programs themselves are started (`Command::new`) only in the module
//! that is listed for them in `ci/allowed-commands-readonly.txt`; this module
//! only waits for a command that module has already built.

use std::io::Read;
use std::process::Command;
use std::time::{Duration, Instant};

/// What a finished program printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// Whether it exited with status 0.
    pub success: bool,
    /// What it printed on stdout, lossily decoded.
    pub stdout: String,
    /// What it printed on stderr, lossily decoded.
    pub stderr: String,
}

/// Why a program could not be run to the end.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunError {
    /// The program is not in any searched folder.
    #[error("the program was not found")]
    NotFound,
    /// It did not finish in time and was stopped.
    #[error("the program took too long")]
    TimedOut,
    /// Anything else, in plain words.
    #[error("{0}")]
    Other(String),
}

/// Starts `command`, collects what it prints and waits for it, stopping it
/// if it takes longer than `timeout`. `name` is the program's name in
/// plain words, for error messages.
pub(crate) fn wait_with_timeout(
    mut command: Command,
    timeout: Duration,
    name: &str,
) -> Result<Output, RunError> {
    let mut child = command.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => RunError::NotFound,
        _ => RunError::Other(format!("{name} could not be started ({e}).")),
    })?;
    // Read both pipes on their own threads so a full pipe can't stall it.
    let stdout = child.stdout.take().map(drain);
    let stderr = child.stderr.take().map(drain);
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= timeout => {
                // Stopping the query is safe: it only reads.
                let _ = child.kill();
                let _ = child.wait();
                return Err(RunError::TimedOut);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(RunError::Other(format!("{name} could not be read ({e})."))),
        }
    };
    let collect = |h: Option<std::thread::JoinHandle<String>>| {
        h.and_then(|h| h.join().ok()).unwrap_or_default()
    };
    Ok(Output {
        success: status.success(),
        stdout: collect(stdout),
        stderr: collect(stderr),
    })
}

/// Reads a pipe to the end on its own thread.
fn drain<R: Read + Send + 'static>(mut pipe: R) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    })
}
