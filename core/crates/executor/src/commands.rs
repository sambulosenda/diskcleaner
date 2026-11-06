//! The allow-listed tool commands of `docs/safety/allowed-commands.md`.
//!
//! Only the two Docker commands are here so far:
//!
//! | Id | Preview (read-only) | Command |
//! |---|---|---|
//! | `docker_image_prune` | `docker image ls --filter dangling=true --format json` | `docker image prune` |
//! | `docker_builder_prune` | `docker builder du` | `docker builder prune` |
//!
//! How a command runs:
//!
//! 1. [`preview_docker`] runs the read-only preview and keeps what it listed
//!    in a [`DockerPlan`]. The user sees that list before anything runs.
//! 2. [`run_docker`] takes the plan by value (so it is used once), refuses it
//!    if it is older than [`planner::PLAN_LIFETIME`], runs the preview again
//!    and refuses if anything is listed now that was not listed before. Only
//!    then does it run the command, answering Docker's own "Are you sure?"
//!    prompt with `y` on stdin.
//!
//! Safety rules this module keeps (CLAUDE.md 6 and 8):
//!
//! - Every program runs with an argument array, never through a shell.
//! - The command arrays are fixed here and checked by [`check_run_args`]:
//!   never `-f` / `--force`, never `-a` / `--all`, and nothing that names
//!   volumes. Neither command can touch a Docker volume.
//! - The full stdout and stderr are handed back for History.

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime};

use analysers::docker::{self, Output, ParseError, RunError};

/// How long a read-only preview may take.
pub const PREVIEW_TIMEOUT: Duration = Duration::from_secs(60);

/// How long a prune may take. Removing many gigabytes of images can be slow.
pub const RUN_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// At most this much of stdout and of stderr is kept for History.
pub const KEPT_OUTPUT_BYTES: usize = 64 * 1024;

/// Flags and words a Docker command from Roomy must never contain.
const FORBIDDEN_ARGS: &[&str] = &[
    "-f",
    "--force",
    "-a",
    "--all",
    "--volumes",
    "volume",
    "volumes",
    "system",
];

/// One of the Docker clean-ups on the allow-list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DockerCleanup {
    /// `docker image prune`: images no tag and no container uses.
    UnusedImages,
    /// `docker builder prune`: build cache no image needs.
    BuildCache,
}

impl DockerCleanup {
    /// Both clean-ups.
    pub const ALL: [DockerCleanup; 2] = [Self::UnusedImages, Self::BuildCache];

    /// Its id in `docs/safety/allowed-commands.md`.
    pub fn id(self) -> &'static str {
        match self {
            Self::UnusedImages => "docker_image_prune",
            Self::BuildCache => "docker_builder_prune",
        }
    }

    /// The read-only preview's arguments, exactly as the allow-list gives them.
    pub fn preview_args(self) -> &'static [&'static str] {
        match self {
            Self::UnusedImages => &[
                "image",
                "ls",
                "--filter",
                "dangling=true",
                "--format",
                "json",
            ],
            Self::BuildCache => &["builder", "du"],
        }
    }

    /// The command's arguments: no `-f`, no `-a`, no `--all`.
    pub fn run_args(self) -> &'static [&'static str] {
        match self {
            Self::UnusedImages => &["image", "prune"],
            Self::BuildCache => &["builder", "prune"],
        }
    }

    /// The command as a person would type it, for History and the app.
    pub fn command_line(self) -> String {
        format!("docker {}", self.run_args().join(" "))
    }
}

/// Refuses any argument list that is not exactly one of the allow-listed
/// commands, or that holds a forbidden flag. Called just before running.
pub fn check_run_args(args: &[&str]) -> Result<(), DockerError> {
    let listed = DockerCleanup::ALL.iter().any(|c| c.run_args() == args);
    let forbidden = args
        .iter()
        .any(|a| FORBIDDEN_ARGS.contains(a) || a.starts_with("--force") || a.starts_with("--all"));
    if listed && !forbidden {
        Ok(())
    } else {
        Err(DockerError::NotAllowed)
    }
}

/// Runs the `docker` program. A trait so tests can stand in for Docker.
pub trait DockerRunner {
    /// Runs `docker <args>`, writing `stdin` to it, and waits at most `timeout`.
    fn run(&self, args: &[&str], stdin: &[u8], timeout: Duration) -> Result<Output, RunError>;
}

/// The real `docker` program, found in the folders [`docker::search_path`] names.
#[derive(Debug, Clone)]
pub struct RealDockerRunner {
    path: OsString,
}

impl RealDockerRunner {
    /// Looks for `docker` in the usual folders for `home`.
    pub fn for_home(home: &Path) -> Self {
        Self {
            path: docker::search_path(home),
        }
    }

    /// Looks for `docker` in exactly these folders. For tests.
    pub fn with_search_path(path: OsString) -> Self {
        Self { path }
    }
}

impl DockerRunner for RealDockerRunner {
    fn run(&self, args: &[&str], stdin: &[u8], timeout: Duration) -> Result<Output, RunError> {
        let mut command = Command::new("docker");
        command
            .args(args)
            .env("PATH", &self.path)
            // Keeps Docker's "What's next?" tips out of History.
            .env("DOCKER_CLI_HINTS", "false")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => RunError::NotFound,
            _ => RunError::Other(format!("Docker could not be started ({e}).")),
        })?;
        if let Some(mut pipe) = child.stdin.take() {
            // A program that never reads its input closes the pipe; that is fine.
            let _ = pipe.write_all(stdin);
        }
        let stdout = child.stdout.take().map(drain);
        let stderr = child.stderr.take().map(drain);
        let started = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if started.elapsed() >= timeout => {
                    // Stopping the docker program does not undo or corrupt
                    // anything: Docker itself finishes or abandons the work.
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(RunError::TimedOut);
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => return Err(RunError::Other(format!("Docker could not be read ({e})."))),
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
}

/// Reads a pipe to the end on its own thread.
fn drain<R: Read + Send + 'static>(mut pipe: R) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

/// Why a Docker clean-up was not previewed or not run. Plain words, no paths.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DockerError {
    /// No `docker` program was found.
    #[error("Docker isn't installed.")]
    NotInstalled,
    /// Docker is not running.
    #[error("Docker isn't running. Open Docker and try again.")]
    NotRunning,
    /// The plan is older than the plan lifetime, or from the future.
    #[error("This review has expired, so please review again.")]
    Expired,
    /// Something is listed now that the user did not see.
    #[error(
        "Docker's data changed since the review, so nothing was removed. Please review again."
    )]
    Changed,
    /// The preview listed nothing, so there is nothing to run.
    #[error("There is nothing to remove.")]
    NothingToClean,
    /// The arguments are not one of the allow-listed commands. A bug.
    #[error("Roomy refused to run this Docker command.")]
    NotAllowed,
    /// Docker's output could not be read, so Roomy did not act on it.
    #[error("Roomy could not read Docker's answer, so nothing was removed.")]
    Unreadable,
    /// Docker took too long.
    #[error("Docker took too long to answer. It may still be finishing.")]
    TimedOut,
    /// Docker reported a problem.
    #[error("{reason}")]
    Failed {
        /// Plain words.
        reason: String,
    },
}

impl From<ParseError> for DockerError {
    fn from(e: ParseError) -> Self {
        tracing::warn!(error = %e, "docker preview not understood");
        Self::Unreadable
    }
}

/// One thing a clean-up would remove, as its preview listed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerItem {
    /// Docker's id for it.
    pub id: String,
    /// Bytes, as Docker estimates it.
    pub size_bytes: u64,
    /// When it was made or last used, in Docker's words.
    pub age: String,
}

/// A previewed clean-up, waiting for the user. Single use: [`run_docker`]
/// takes it by value. Deliberately not `Clone`.
#[derive(Debug, PartialEq, Eq)]
pub struct DockerPlan {
    cleanup: DockerCleanup,
    made_at: SystemTime,
    items: Vec<DockerItem>,
    total_bytes: u64,
}

impl DockerPlan {
    /// Which clean-up this is.
    pub fn cleanup(&self) -> DockerCleanup {
        self.cleanup
    }

    /// What the preview listed.
    pub fn items(&self) -> &[DockerItem] {
        &self.items
    }

    /// The bytes Docker says would be freed.
    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// The ids the preview listed.
    fn ids(&self) -> BTreeSet<&str> {
        self.items.iter().map(|i| i.id.as_str()).collect()
    }
}

/// What a clean-up did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerRun {
    /// Which clean-up ran.
    pub cleanup: DockerCleanup,
    /// The command as typed, for History.
    pub command_line: String,
    /// Whether Docker exited with status 0.
    pub success: bool,
    /// Bytes Docker says it freed, from its "Total reclaimed space" line.
    pub reclaimed_bytes: Option<u64>,
    /// What Docker printed, cut to [`KEPT_OUTPUT_BYTES`].
    pub stdout: String,
    /// What Docker printed on stderr, cut to [`KEPT_OUTPUT_BYTES`].
    pub stderr: String,
}

/// Turns a failed or unfinished docker run into the error the user sees.
fn run_failure(err: RunError) -> DockerError {
    match err {
        RunError::NotFound => DockerError::NotInstalled,
        RunError::TimedOut => DockerError::TimedOut,
        RunError::Other(reason) => DockerError::Failed { reason },
    }
}

/// Runs a read-only preview and reads what it listed.
fn list(
    runner: &dyn DockerRunner,
    cleanup: DockerCleanup,
) -> Result<(Vec<DockerItem>, u64), DockerError> {
    let out = runner
        .run(cleanup.preview_args(), b"", PREVIEW_TIMEOUT)
        .map_err(run_failure)?;
    if !out.success {
        return Err(if docker::is_not_running(&out.stderr) {
            DockerError::NotRunning
        } else {
            tracing::warn!(stderr = %out.stderr.trim(), "docker preview failed");
            DockerError::Failed {
                reason: "Docker reported a problem, so Roomy could not list what it would remove."
                    .to_string(),
            }
        });
    }
    match cleanup {
        DockerCleanup::UnusedImages => {
            let images = docker::parse_dangling_images(&out.stdout)?;
            let total = images
                .iter()
                .fold(0u64, |sum, i| sum.saturating_add(i.size_bytes));
            let items = images
                .into_iter()
                .map(|i| DockerItem {
                    id: i.id,
                    size_bytes: i.size_bytes,
                    age: i.created,
                })
                .collect();
            Ok((items, total))
        }
        DockerCleanup::BuildCache => {
            let du = docker::parse_builder_du(&out.stdout)?;
            // `docker builder prune` without `--all` removes only cache
            // Docker marks reclaimable, so only those are listed.
            let items = du
                .records
                .into_iter()
                .filter(|r| r.reclaimable)
                .map(|r| DockerItem {
                    id: r.id,
                    size_bytes: r.size_bytes,
                    age: r.last_used,
                })
                .collect();
            Ok((items, du.reclaimable_bytes))
        }
    }
}

/// The dry run: lists what `cleanup` would remove, with the read-only
/// preview from the allow-list. Changes nothing.
pub fn preview_docker(
    runner: &dyn DockerRunner,
    cleanup: DockerCleanup,
    now: SystemTime,
) -> Result<DockerPlan, DockerError> {
    let (items, total_bytes) = list(runner, cleanup)?;
    Ok(DockerPlan {
        cleanup,
        made_at: now,
        items,
        total_bytes,
    })
}

/// Carries out a previewed clean-up, once.
///
/// Refuses, without running anything, if the plan has expired, if the
/// preview listed nothing, or if a fresh preview lists anything the user
/// did not see. Docker may have removed some of the listed items itself in
/// the meantime; that is fine, as the command can only remove less.
pub fn run_docker(
    runner: &dyn DockerRunner,
    plan: DockerPlan,
    now: SystemTime,
) -> Result<DockerRun, DockerError> {
    match now.duration_since(plan.made_at) {
        Ok(age) if age < planner::PLAN_LIFETIME => {}
        _ => return Err(DockerError::Expired),
    }
    if plan.items.is_empty() {
        return Err(DockerError::NothingToClean);
    }
    let args = plan.cleanup.run_args();
    check_run_args(args)?;
    let (now_listed, _) = list(runner, plan.cleanup)?;
    let seen = plan.ids();
    if now_listed.iter().any(|i| !seen.contains(i.id.as_str())) {
        return Err(DockerError::Changed);
    }
    if now_listed.is_empty() {
        return Err(DockerError::NothingToClean);
    }
    tracing::info!(command = %plan.cleanup.command_line(), items = plan.items.len(), "running docker clean-up");
    // Docker asks "Are you sure you want to continue? [y/N]"; this answers it.
    let out = runner.run(args, b"y\n", RUN_TIMEOUT).map_err(run_failure)?;
    if !out.success && docker::is_not_running(&out.stderr) {
        return Err(DockerError::NotRunning);
    }
    Ok(DockerRun {
        cleanup: plan.cleanup,
        command_line: plan.cleanup.command_line(),
        success: out.success,
        reclaimed_bytes: reclaimed(&out.stdout),
        stdout: cut(out.stdout),
        stderr: cut(out.stderr),
    })
}

/// The size on Docker's "Total reclaimed space: 1.2GB" line, if there is one.
fn reclaimed(stdout: &str) -> Option<u64> {
    stdout
        .lines()
        .rev()
        .find_map(|l| l.trim().strip_prefix("Total reclaimed space:"))
        .and_then(|rest| docker::parse_size(rest).ok())
}

/// Cuts `text` to at most [`KEPT_OUTPUT_BYTES`], on a character boundary.
fn cut(mut text: String) -> String {
    if text.len() > KEPT_OUTPUT_BYTES {
        let mut end = KEPT_OUTPUT_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("\n[cut short]");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    /// Answers each call with the next recorded output and remembers the calls.
    #[derive(Default)]
    struct Recorded {
        answers: RefCell<VecDeque<Result<Output, RunError>>>,
        calls: RefCell<Vec<(Vec<String>, Vec<u8>)>>,
    }

    impl Recorded {
        fn answering(answers: Vec<Result<Output, RunError>>) -> Self {
            Self {
                answers: RefCell::new(answers.into()),
                calls: RefCell::default(),
            }
        }
        fn calls(&self) -> Vec<(Vec<String>, Vec<u8>)> {
            self.calls.borrow().clone()
        }
    }

    impl DockerRunner for Recorded {
        fn run(&self, args: &[&str], stdin: &[u8], _: Duration) -> Result<Output, RunError> {
            self.calls
                .borrow_mut()
                .push((args.iter().map(|s| s.to_string()).collect(), stdin.to_vec()));
            self.answers
                .borrow_mut()
                .pop_front()
                .unwrap_or(Err(RunError::Other("no more answers".into())))
        }
    }

    fn ok(stdout: &str) -> Result<Output, RunError> {
        Ok(Output {
            success: true,
            stdout: stdout.to_string(),
            stderr: String::new(),
        })
    }

    fn failed(stderr: &str) -> Result<Output, RunError> {
        Ok(Output {
            success: false,
            stdout: String::new(),
            stderr: stderr.to_string(),
        })
    }

    const TWO_IMAGES: &str = r#"{"CreatedSince":"5 weeks ago","ID":"aaa111","Repository":"<none>","Size":"1GB","Tag":"<none>"}
{"CreatedSince":"2 days ago","ID":"bbb222","Repository":"<none>","Size":"500MB","Tag":"<none>"}
"#;
    const ONE_IMAGE: &str = r#"{"CreatedSince":"5 weeks ago","ID":"aaa111","Repository":"<none>","Size":"1GB","Tag":"<none>"}"#;
    const NEW_IMAGE: &str = r#"{"CreatedSince":"1 second ago","ID":"ccc333","Repository":"<none>","Size":"9GB","Tag":"<none>"}"#;
    const BUILDER_DU: &str = "ID  RECLAIMABLE  SIZE  LAST ACCESSED
abc123*  true  2GB  3 weeks ago
def456  false  1GB  1 minute ago
Reclaimable: 2GB
Total: 3GB
";
    const PRUNE_OUT: &str =
        "Deleted Images:\ndeleted: sha256:aaa111\n\nTotal reclaimed space: 1.5GB\n";

    fn now() -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(2_000_000_000)
    }

    #[test]
    fn command_arrays_match_the_allow_list_exactly() {
        assert_eq!(DockerCleanup::UnusedImages.id(), "docker_image_prune");
        assert_eq!(DockerCleanup::BuildCache.id(), "docker_builder_prune");
        assert_eq!(
            DockerCleanup::UnusedImages.preview_args().join(" "),
            "image ls --filter dangling=true --format json"
        );
        assert_eq!(
            DockerCleanup::BuildCache.preview_args().join(" "),
            "builder du"
        );
        assert_eq!(
            DockerCleanup::UnusedImages.command_line(),
            "docker image prune"
        );
        assert_eq!(
            DockerCleanup::BuildCache.command_line(),
            "docker builder prune"
        );
    }

    #[test]
    fn no_command_ever_forces_or_widens() {
        for c in DockerCleanup::ALL {
            assert_eq!(check_run_args(c.run_args()), Ok(()));
            for arg in c.run_args().iter().chain(c.preview_args()) {
                assert!(!FORBIDDEN_ARGS.contains(arg), "{arg}");
                assert!(!arg.starts_with('-') || *arg == "--filter" || *arg == "--format");
            }
        }
    }

    #[test]
    fn check_run_args_refuses_anything_else() {
        for args in [
            &["image", "prune", "-f"][..],
            &["image", "prune", "-a"],
            &["image", "prune", "--all"],
            &["builder", "prune", "--force"],
            &["builder", "prune", "--all"],
            &["volume", "prune"],
            &["system", "prune"],
            &["system", "prune", "--volumes"],
            &["container", "prune"],
            &["image", "rm", "aaa111"],
            &[],
        ] {
            assert_eq!(
                check_run_args(args),
                Err(DockerError::NotAllowed),
                "{args:?}"
            );
        }
    }

    #[test]
    fn image_preview_lists_dangling_images() {
        let runner = Recorded::answering(vec![ok(TWO_IMAGES)]);
        let plan = preview_docker(&runner, DockerCleanup::UnusedImages, now()).unwrap();
        assert_eq!(plan.items().len(), 2);
        assert_eq!(plan.total_bytes(), 1_500_000_000);
        let calls = runner.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].0,
            [
                "image",
                "ls",
                "--filter",
                "dangling=true",
                "--format",
                "json"
            ]
        );
        assert!(calls[0].1.is_empty());
    }

    #[test]
    fn cache_preview_lists_only_reclaimable_records() {
        let runner = Recorded::answering(vec![ok(BUILDER_DU)]);
        let plan = preview_docker(&runner, DockerCleanup::BuildCache, now()).unwrap();
        assert_eq!(plan.items().len(), 1);
        assert_eq!(plan.items()[0].id, "abc123");
        assert_eq!(plan.total_bytes(), 2_000_000_000);
    }

    #[test]
    fn preview_reports_docker_missing_or_stopped() {
        let runner = Recorded::answering(vec![Err(RunError::NotFound)]);
        assert_eq!(
            preview_docker(&runner, DockerCleanup::UnusedImages, now()),
            Err(DockerError::NotInstalled)
        );
        let runner = Recorded::answering(vec![failed(
            "Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?",
        )]);
        assert_eq!(
            preview_docker(&runner, DockerCleanup::BuildCache, now()),
            Err(DockerError::NotRunning)
        );
        let runner = Recorded::answering(vec![ok("garbage")]);
        assert_eq!(
            preview_docker(&runner, DockerCleanup::UnusedImages, now()),
            Err(DockerError::Unreadable)
        );
    }

    #[test]
    fn run_previews_again_then_prunes_with_yes_on_stdin() {
        let runner = Recorded::answering(vec![ok(TWO_IMAGES), ok(TWO_IMAGES), ok(PRUNE_OUT)]);
        let plan = preview_docker(&runner, DockerCleanup::UnusedImages, now()).unwrap();
        let ran = run_docker(&runner, plan, now() + Duration::from_secs(60)).unwrap();
        assert!(ran.success);
        assert_eq!(ran.reclaimed_bytes, Some(1_500_000_000));
        assert_eq!(ran.command_line, "docker image prune");
        assert!(ran.stdout.contains("Deleted Images"));
        let calls = runner.calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[1].0, calls[0].0, "the preview runs again first");
        assert_eq!(calls[2].0, ["image", "prune"]);
        assert_eq!(calls[2].1, b"y\n");
    }

    #[test]
    fn run_goes_ahead_when_docker_already_removed_some() {
        let runner = Recorded::answering(vec![ok(TWO_IMAGES), ok(ONE_IMAGE), ok(PRUNE_OUT)]);
        let plan = preview_docker(&runner, DockerCleanup::UnusedImages, now()).unwrap();
        assert!(run_docker(&runner, plan, now()).is_ok());
    }

    #[test]
    fn run_refuses_when_something_new_appeared() {
        let both = format!("{ONE_IMAGE}\n{NEW_IMAGE}\n");
        let runner = Recorded::answering(vec![ok(ONE_IMAGE), ok(&both)]);
        let plan = preview_docker(&runner, DockerCleanup::UnusedImages, now()).unwrap();
        assert_eq!(run_docker(&runner, plan, now()), Err(DockerError::Changed));
        assert_eq!(runner.calls().len(), 2, "the prune never ran");
    }

    #[test]
    fn run_refuses_new_reclaimable_cache() {
        let more = BUILDER_DU.replace("def456  false", "def456  true");
        let runner = Recorded::answering(vec![ok(BUILDER_DU), ok(&more)]);
        let plan = preview_docker(&runner, DockerCleanup::BuildCache, now()).unwrap();
        assert_eq!(run_docker(&runner, plan, now()), Err(DockerError::Changed));
        assert_eq!(runner.calls().len(), 2);
    }

    #[test]
    fn run_refuses_an_expired_or_future_plan() {
        let runner = Recorded::answering(vec![ok(TWO_IMAGES)]);
        let plan = preview_docker(&runner, DockerCleanup::UnusedImages, now()).unwrap();
        assert_eq!(
            run_docker(&runner, plan, now() + planner::PLAN_LIFETIME),
            Err(DockerError::Expired)
        );
        let runner = Recorded::answering(vec![ok(TWO_IMAGES)]);
        let plan = preview_docker(&runner, DockerCleanup::UnusedImages, now()).unwrap();
        assert_eq!(
            run_docker(&runner, plan, now() - Duration::from_secs(1)),
            Err(DockerError::Expired)
        );
        assert_eq!(runner.calls().len(), 1);
    }

    #[test]
    fn run_refuses_an_empty_preview() {
        let runner = Recorded::answering(vec![ok("")]);
        let plan = preview_docker(&runner, DockerCleanup::UnusedImages, now()).unwrap();
        assert!(plan.items().is_empty());
        assert_eq!(
            run_docker(&runner, plan, now()),
            Err(DockerError::NothingToClean)
        );
        assert_eq!(runner.calls().len(), 1);
    }

    #[test]
    fn run_reports_a_failed_prune() {
        let runner = Recorded::answering(vec![
            ok(TWO_IMAGES),
            ok(TWO_IMAGES),
            Ok(Output {
                success: false,
                stdout: String::new(),
                stderr: "Error response from daemon: a prune operation is already running".into(),
            }),
        ]);
        let plan = preview_docker(&runner, DockerCleanup::UnusedImages, now()).unwrap();
        let ran = run_docker(&runner, plan, now()).unwrap();
        assert!(!ran.success);
        assert_eq!(ran.reclaimed_bytes, None);
        assert!(ran.stderr.contains("already running"));
    }

    #[test]
    fn long_output_is_cut_on_a_character_boundary() {
        let long = "é".repeat(KEPT_OUTPUT_BYTES);
        let kept = cut(long);
        assert!(kept.len() <= KEPT_OUTPUT_BYTES + 20);
        assert!(kept.ends_with("[cut short]"));
    }

    #[test]
    fn ids_match_the_rules_allow_list() {
        for c in DockerCleanup::ALL {
            assert!(rules::commands::is_allowed(c.id()), "{}", c.id());
        }
    }

    /// The real runner against a stand-in `docker` script: arguments arrive
    /// one by one, and the prompt answer arrives on stdin.
    #[test]
    fn real_runner_passes_an_argument_array_and_stdin() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("log.txt");
        let script = dir.path().join("docker");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nfor a in \"$@\"; do echo \"arg:$a\" >> '{log}'; done\nread answer\necho \"stdin:$answer\" >> '{log}'\necho 'Total reclaimed space: 2kB'\n",
                log = log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let runner = RealDockerRunner::with_search_path(dir.path().as_os_str().to_owned());
        let out = runner
            .run(&["image", "prune"], b"y\n", Duration::from_secs(10))
            .unwrap();
        assert!(out.success);
        assert_eq!(reclaimed(&out.stdout), Some(2000));
        assert_eq!(
            std::fs::read_to_string(&log).unwrap(),
            "arg:image\narg:prune\nstdin:y\n"
        );
    }

    #[test]
    fn real_runner_stops_a_program_that_hangs() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("docker");
        std::fs::write(&script, "#!/bin/sh\nexec /bin/sleep 30\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let runner = RealDockerRunner::with_search_path(dir.path().as_os_str().to_owned());
        let started = Instant::now();
        assert_eq!(
            runner.run(&["builder", "du"], b"", Duration::from_millis(300)),
            Err(RunError::TimedOut)
        );
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn real_runner_without_docker_is_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let runner = RealDockerRunner::with_search_path(dir.path().as_os_str().to_owned());
        assert_eq!(
            runner.run(&["builder", "du"], b"", Duration::from_secs(5)),
            Err(RunError::NotFound)
        );
    }
}
