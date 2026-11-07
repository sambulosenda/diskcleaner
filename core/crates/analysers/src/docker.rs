//! Docker: how much space Docker's own data uses, read with the read-only
//! query `docker system df`, plus the size of Docker Desktop's disk image.
//!
//! Nothing here changes anything. The two clean-ups Roomy can run
//! (`docker image prune` and `docker builder prune`, see
//! `docs/safety/allowed-commands.md`) live in `executor::commands`; this
//! module also holds the pure parsers that the executor uses to read their
//! previews, so that both crates read Docker's output the same way.
//!
//! Docker prints sizes rounded for people ("1.2GB"), so every byte count
//! here is an estimate, close to what `docker system df` shows.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::process;
pub use crate::process::{Output, RunError};

/// How long the read-only query may take before Roomy gives up on it.
pub const QUERY_TIMEOUT: Duration = Duration::from_secs(20);

/// Where Docker Desktop keeps its disk image, relative to the home folder.
pub const DISK_IMAGE: &str = "Library/Containers/com.docker.docker/Data/vms/0/data/Docker.raw";

/// The folders searched for the `docker` program, in order.
///
/// A Mac app starts with a short `PATH` that holds none of the places Docker
/// installs its command-line tool, so Roomy names them itself (OrbStack
/// puts its copy in `~/.orbstack/bin`). `~/.docker/bin`
/// is where Docker Desktop puts it when it is set up without admin rights.
pub fn search_path(home: &Path) -> OsString {
    let folders: [PathBuf; 7] = [
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        home.join(".docker/bin"),
        home.join(".orbstack/bin"),
        PathBuf::from("/Applications/Docker.app/Contents/Resources/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
    ];
    std::env::join_paths(folders.iter()).unwrap_or_default()
}

/// One kind of Docker data, as one row of `docker system df`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    /// How many there are.
    pub count: u64,
    /// How many are in use (by a container, or for build cache, by a build).
    pub active: u64,
    /// Bytes they use, as Docker estimates it.
    pub size_bytes: u64,
    /// Bytes Docker says could be freed.
    pub reclaimable_bytes: u64,
}

/// Everything `docker system df` reports.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DockerUsage {
    /// Images.
    pub images: Usage,
    /// Containers, running or stopped.
    pub containers: Usage,
    /// Volumes. Roomy only ever shows these; it never removes them.
    pub volumes: Usage,
    /// Build cache.
    pub build_cache: Usage,
}

/// What Roomy could find out about Docker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockerState {
    /// No `docker` program in any of the usual places.
    NotInstalled,
    /// The `docker` program is there but Docker itself is not running.
    NotRunning,
    /// The query failed for another reason, in plain words.
    Unavailable {
        /// Why, with no path.
        reason: String,
    },
    /// The numbers.
    Ready(DockerUsage),
}

/// Runs the read-only `docker system df` query. A trait so that tests can
/// answer with recorded output instead of a real Docker.
pub trait DockerQuery {
    /// Runs `docker system df --format '{{json .}}'`.
    fn system_df(&self) -> Result<Output, RunError>;
}

/// The real `docker` program, found through [`search_path`].
#[derive(Debug, Clone)]
pub struct RealDocker {
    path: OsString,
}

impl RealDocker {
    /// Looks for `docker` in the usual folders for `home`.
    pub fn for_home(home: &Path) -> Self {
        Self {
            path: search_path(home),
        }
    }

    /// Looks for `docker` in exactly these folders (a `PATH`-style list).
    /// For tests that put a stand-in `docker` in a temporary folder.
    pub fn with_search_path(path: OsString) -> Self {
        Self { path }
    }
}

impl DockerQuery for RealDocker {
    fn system_df(&self) -> Result<Output, RunError> {
        // Arguments are an array, never a shell line. `{{json .}}` prints one
        // JSON object per row and works on every Docker version still in use.
        let mut command = Command::new("docker");
        command
            .args(["system", "df", "--format", "{{json .}}"])
            .env("PATH", &self.path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        process::wait_with_timeout(command, QUERY_TIMEOUT, "Docker")
    }
}

/// Asks Docker how much space it uses. Never fails: every problem becomes
/// a [`DockerState`] the app can explain.
pub fn usage(query: &dyn DockerQuery) -> DockerState {
    match query.system_df() {
        Err(RunError::NotFound) => DockerState::NotInstalled,
        Err(RunError::TimedOut) => DockerState::Unavailable {
            reason: "Docker took too long to answer. Try again in a moment.".to_string(),
        },
        Err(RunError::Other(reason)) => DockerState::Unavailable { reason },
        Ok(out) if !out.success => failure_state(&out.stderr),
        Ok(out) => match parse_system_df(&out.stdout) {
            Ok(usage) => DockerState::Ready(usage),
            Err(e) => {
                tracing::warn!(error = %e, "docker system df output not understood");
                DockerState::Unavailable {
                    reason: concat!(brand::name!(), " could not read Docker's answer.").to_string(),
                }
            }
        },
    }
}

/// Whether Docker's error says that Docker itself is not running.
///
/// Docker CLI 28 and later say "failed to connect to the docker API at
/// unix://…; check if the path is correct and if the daemon is running";
/// older versions say "Cannot connect to the Docker daemon … Is the docker
/// daemon running?". Both, and a socket that is missing or refusing
/// connections, mean Docker is not running.
pub fn is_not_running(stderr: &str) -> bool {
    let lower = stderr.to_ascii_lowercase();
    let socket_gone = lower.contains("docker.sock")
        && (lower.contains("no such file or directory") || lower.contains("connection refused"));
    lower.contains("cannot connect to the docker daemon")
        || lower.contains("is the docker daemon running")
        || lower.contains("failed to connect to the docker api")
        || lower.contains("if the daemon is running")
        || lower.contains("docker desktop is not running")
        || lower.contains("error during connect")
        || socket_gone
}

/// Docker's own error, in one short line the app can show: the first line
/// of stderr that says something, with every path replaced by "…" so no
/// path reaches the app, and cut to 160 characters. `None` if Docker
/// printed nothing.
pub fn error_line(stderr: &str) -> Option<String> {
    let line = stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.to_ascii_lowercase().starts_with("warning"))
        .or_else(|| stderr.lines().map(str::trim).find(|l| !l.is_empty()))?;
    let line = redact_quoted_paths(line);
    let cleaned: Vec<String> = line
        .split_whitespace()
        .map(|word| {
            if word.contains('/') {
                "…".to_string()
            } else {
                word.to_string()
            }
        })
        .collect();
    let mut text = cleaned.join(" ");
    if text.chars().count() > 160 {
        text = text.chars().take(159).collect::<String>() + "…";
    }
    Some(text)
}

/// Replaces every double-quoted span holding a `/` with `"…"`, so a quoted
/// path with spaces in it ("/Users/Alice Smith") is removed whole.
fn redact_quoted_paths(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(start) = rest.find('"') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('"') {
            Some(end) => {
                let inner = &after[..end];
                if inner.contains('/') {
                    out.push_str("\"…\"");
                } else {
                    out.push('"');
                    out.push_str(inner);
                    out.push('"');
                }
                rest = &after[end + 1..];
            }
            None => {
                // An unclosed quote: drop the rest if it holds a path.
                if after.contains('/') {
                    out.push_str("\"…");
                } else {
                    out.push('"');
                    out.push_str(after);
                }
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// "Docker said: …" for an error Roomy doesn't recognise, or `fallback`
/// if Docker printed nothing.
pub fn explain_failure(stderr: &str, fallback: &str) -> String {
    match error_line(stderr) {
        Some(line) => format!("Docker said: {line}"),
        None => fallback.to_string(),
    }
}

/// The state for a query that exited with an error.
fn failure_state(stderr: &str) -> DockerState {
    if is_not_running(stderr) {
        DockerState::NotRunning
    } else {
        tracing::warn!(stderr = %stderr.trim(), "docker system df failed");
        DockerState::Unavailable {
            reason: explain_failure(
                stderr,
                concat!("Docker stopped with an error but didn't say why, so ", brand::name!(), " could not read its sizes."),
            ),
        }
    }
}

/// Allocated bytes of Docker Desktop's disk image, if there is one. The
/// file is sparse, so this is the space it really takes, not its length.
/// Reads metadata only and never follows a symlink.
pub fn disk_image_bytes(home: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::symlink_metadata(home.join(DISK_IMAGE)).ok()?;
    meta.file_type()
        .is_file()
        .then(|| meta.blocks().saturating_mul(512))
}

/// Docker's output could not be understood. Every parser fails closed: an
/// unknown shape is an error, never a guess.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unexpected Docker output: {0}")]
pub struct ParseError(pub String);

/// Reads a size the way Docker prints it: "0B", "512kB", "1.2GB",
/// "3.4GiB". Decimal units are powers of 1000, binary ones of 1024.
/// Anything after the first space (such as "(52%)") is ignored.
pub fn parse_size(text: &str) -> Result<u64, ParseError> {
    let word = text.split_whitespace().next().unwrap_or("");
    let split = word
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(word.len());
    let (number, unit) = word.split_at(split);
    let value: f64 = number
        .parse()
        .map_err(|_| ParseError(format!("size {text:?}")))?;
    let scale: f64 = match unit {
        "B" | "" => 1.0,
        "kB" | "KB" | "k" => 1e3,
        "MB" | "M" => 1e6,
        "GB" | "G" => 1e9,
        "TB" | "T" => 1e12,
        "PB" => 1e15,
        "KiB" => 1024.0,
        "MiB" => 1024.0 * 1024.0,
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        "TiB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return Err(ParseError(format!("size unit {unit:?}"))),
    };
    let bytes = (value * scale).round();
    if !bytes.is_finite() || bytes < 0.0 {
        return Err(ParseError(format!("size {text:?}")));
    }
    // `as` saturates for floats too large for u64, which is what we want.
    Ok(bytes as u64)
}

/// Reads a whole number field such as "5". Docker sometimes prints "N/A"
/// for counts it does not track; that reads as 0.
fn parse_count(text: &str) -> Result<u64, ParseError> {
    let t = text.trim();
    if t.eq_ignore_ascii_case("n/a") || t.is_empty() {
        return Ok(0);
    }
    t.parse().map_err(|_| ParseError(format!("count {text:?}")))
}

/// One JSON line, read into string fields only. Docker's `{{json .}}`
/// output is a flat object of strings, so this small reader is enough and
/// avoids a JSON dependency. Anything else is an error.
fn parse_flat_json(line: &str) -> Result<Vec<(String, String)>, ParseError> {
    let bad = || ParseError(format!("json line {line:?}"));
    let mut chars = line.trim().chars().peekable();
    let mut fields = Vec::new();
    if chars.next() != Some('{') {
        return Err(bad());
    }
    loop {
        skip_spaces(&mut chars);
        match chars.peek() {
            Some('}') => {
                chars.next();
                break;
            }
            Some('"') => {}
            _ => return Err(bad()),
        }
        let key = read_string(&mut chars).ok_or_else(bad)?;
        skip_spaces(&mut chars);
        if chars.next() != Some(':') {
            return Err(bad());
        }
        skip_spaces(&mut chars);
        let value = match chars.peek() {
            Some('"') => read_string(&mut chars).ok_or_else(bad)?,
            // A bare number, true, false or null: keep its text.
            Some(_) => {
                let mut v = String::new();
                while let Some(&c) = chars.peek() {
                    if c == ',' || c == '}' || c.is_whitespace() {
                        break;
                    }
                    if c == '{' || c == '[' {
                        return Err(bad());
                    }
                    v.push(c);
                    chars.next();
                }
                v
            }
            None => return Err(bad()),
        };
        fields.push((key, value));
        skip_spaces(&mut chars);
        match chars.next() {
            Some(',') => continue,
            Some('}') => break,
            _ => return Err(bad()),
        }
    }
    skip_spaces(&mut chars);
    if chars.next().is_some() {
        return Err(bad());
    }
    Ok(fields)
}

fn skip_spaces(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while chars.peek().is_some_and(|c| c.is_whitespace()) {
        chars.next();
    }
}

/// Reads a JSON string, starting at its opening quote.
fn read_string(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) -> Option<String> {
    if chars.next() != Some('"') {
        return None;
    }
    let mut out = String::new();
    loop {
        match chars.next()? {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'r' => out.push('\r'),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{c}'),
                'u' => {
                    let hex: String = (0..4).filter_map(|_| chars.next()).collect();
                    let code = u32::from_str_radix(&hex, 16).ok()?;
                    // Docker escapes <, > and & this way; surrogates become U+FFFD.
                    out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                }
                _ => return None,
            },
            c => out.push(c),
        }
    }
}

/// The value of `key` in a parsed line, or an error naming it.
fn field<'a>(fields: &'a [(String, String)], key: &str) -> Result<&'a str, ParseError> {
    fields
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_str())
        .ok_or_else(|| ParseError(format!("missing field {key:?}")))
}

/// Reads the output of `docker system df --format '{{json .}}'`: one line
/// per kind of data. Every one of the four kinds must be there exactly once.
pub fn parse_system_df(stdout: &str) -> Result<DockerUsage, ParseError> {
    let mut usage = DockerUsage::default();
    let mut seen = [false; 4];
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let fields = parse_flat_json(line)?;
        let row = Usage {
            count: parse_count(field(&fields, "TotalCount")?)?,
            active: parse_count(field(&fields, "Active")?)?,
            size_bytes: parse_size(field(&fields, "Size")?)?,
            reclaimable_bytes: parse_size(field(&fields, "Reclaimable")?)?,
        };
        let (slot, index) = match field(&fields, "Type")? {
            "Images" => (&mut usage.images, 0),
            "Containers" => (&mut usage.containers, 1),
            "Local Volumes" => (&mut usage.volumes, 2),
            "Build Cache" => (&mut usage.build_cache, 3),
            other => return Err(ParseError(format!("unknown type {other:?}"))),
        };
        if seen[index] {
            return Err(ParseError("a type is listed twice".to_string()));
        }
        seen[index] = true;
        *slot = row;
    }
    if seen.iter().all(|s| *s) {
        Ok(usage)
    } else {
        Err(ParseError("a type is missing".to_string()))
    }
}

/// One unused ("dangling") image, as `docker image prune` would remove it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DanglingImage {
    /// Docker's short image id.
    pub id: String,
    /// Bytes, as Docker estimates it.
    pub size_bytes: u64,
    /// When it was made, in Docker's words ("3 weeks ago").
    pub created: String,
}

/// Reads the output of `docker image ls --filter dangling=true --format json`.
pub fn parse_dangling_images(stdout: &str) -> Result<Vec<DanglingImage>, ParseError> {
    let mut images = Vec::new();
    for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
        let fields = parse_flat_json(line)?;
        let id = field(&fields, "ID")?.to_string();
        if !is_docker_id(&id) {
            return Err(ParseError(format!("image id {id:?}")));
        }
        images.push(DanglingImage {
            id,
            size_bytes: parse_size(field(&fields, "Size")?)?,
            created: field(&fields, "CreatedSince").unwrap_or("").to_string(),
        });
    }
    Ok(images)
}

/// One build cache record from `docker builder du`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheRecord {
    /// The record's id, without Docker's trailing `*` marker.
    pub id: String,
    /// Whether Docker could free it.
    pub reclaimable: bool,
    /// Bytes.
    pub size_bytes: u64,
    /// When a build last used it, in Docker's words.
    pub last_used: String,
}

/// What `docker builder du` reports.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BuilderDu {
    /// Every record listed.
    pub records: Vec<CacheRecord>,
    /// The "Reclaimable:" total.
    pub reclaimable_bytes: u64,
}

/// Reads the table `docker builder du` prints:
///
/// ```text
/// ID                          RECLAIMABLE   SIZE      LAST ACCESSED
/// m3hqlq4tjoh2c5ncjsqxsxbv1*  true          1.188GB   2 weeks ago
/// Shared:         0B
/// Private:        1.188GB
/// Reclaimable:    1.188GB
/// Total:          1.188GB
/// ```
///
/// With no cache at all, Docker may print only the totals, or nothing.
pub fn parse_builder_du(stdout: &str) -> Result<BuilderDu, ParseError> {
    let mut du = BuilderDu::default();
    let mut saw_reclaimable = false;
    for line in stdout.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if line.starts_with("ID ") {
            continue;
        }
        if let Some(rest) = line.strip_prefix("Reclaimable:") {
            du.reclaimable_bytes = parse_size(rest)?;
            saw_reclaimable = true;
            continue;
        }
        if ["Shared:", "Private:", "Total:"]
            .iter()
            .any(|p| line.starts_with(p))
        {
            continue;
        }
        let mut words = line.split_whitespace();
        let (Some(id), Some(flag), Some(size)) = (words.next(), words.next(), words.next()) else {
            return Err(ParseError(format!("cache line {line:?}")));
        };
        let id = id.trim_end_matches('*').to_string();
        if !is_docker_id(&id) {
            return Err(ParseError(format!("cache id {id:?}")));
        }
        let reclaimable = match flag {
            "true" => true,
            "false" => false,
            _ => return Err(ParseError(format!("cache line {line:?}"))),
        };
        du.records.push(CacheRecord {
            id,
            reclaimable,
            size_bytes: parse_size(size)?,
            last_used: words.collect::<Vec<_>>().join(" "),
        });
    }
    if !saw_reclaimable && !du.records.is_empty() {
        return Err(ParseError("no Reclaimable total".to_string()));
    }
    Ok(du)
}

/// Whether `id` looks like a Docker image or cache id: letters and digits
/// only (with an optional `sha256:` prefix), 1 to 128 characters.
pub fn is_docker_id(id: &str) -> bool {
    let bare = id.strip_prefix("sha256:").unwrap_or(id);
    !bare.is_empty() && bare.len() <= 128 && bare.chars().all(|c| c.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recorded from Docker Desktop 4.x on macOS.
    const SYSTEM_DF: &str = r#"{"Active":"2","Reclaimable":"9.618GB (78%)","Size":"12.33GB","TotalCount":"14","Type":"Images"}
{"Active":"1","Reclaimable":"1.2MB (50%)","Size":"2.4MB","TotalCount":"3","Type":"Containers"}
{"Active":"1","Reclaimable":"512.3MB (40%)","Size":"1.28GB","TotalCount":"4","Type":"Local Volumes"}
{"Active":"0","Reclaimable":"3.1GB","Size":"3.1GB","TotalCount":"57","Type":"Build Cache"}
"#;

    struct Recorded(Result<Output, RunError>);
    impl DockerQuery for Recorded {
        fn system_df(&self) -> Result<Output, RunError> {
            self.0.clone()
        }
    }

    fn ok(stdout: &str) -> Recorded {
        Recorded(Ok(Output {
            success: true,
            stdout: stdout.to_string(),
            stderr: String::new(),
        }))
    }

    fn failed(stderr: &str) -> Recorded {
        Recorded(Ok(Output {
            success: false,
            stdout: String::new(),
            stderr: stderr.to_string(),
        }))
    }

    #[test]
    fn sizes_read_like_docker_prints_them() {
        assert_eq!(parse_size("0B"), Ok(0));
        assert_eq!(parse_size("512kB"), Ok(512_000));
        assert_eq!(parse_size("1.2GB (52%)"), Ok(1_200_000_000));
        assert_eq!(parse_size("12.33GB"), Ok(12_330_000_000));
        assert_eq!(parse_size("2KiB"), Ok(2048));
        assert_eq!(parse_size("1.5MiB"), Ok(1_572_864));
        assert!(parse_size("lots").is_err());
        assert!(parse_size("12XB").is_err());
        assert!(parse_size("").is_err());
        assert!(parse_size("-1GB").is_err());
    }

    #[test]
    fn system_df_reads_all_four_rows() {
        let usage = parse_system_df(SYSTEM_DF).unwrap();
        assert_eq!(
            usage.images,
            Usage {
                count: 14,
                active: 2,
                size_bytes: 12_330_000_000,
                reclaimable_bytes: 9_618_000_000
            }
        );
        assert_eq!(usage.containers.count, 3);
        assert_eq!(usage.volumes.size_bytes, 1_280_000_000);
        assert_eq!(usage.build_cache.reclaimable_bytes, 3_100_000_000);
    }

    #[test]
    fn system_df_refuses_missing_doubled_or_unknown_rows() {
        let three: String = SYSTEM_DF.lines().take(3).collect::<Vec<_>>().join("\n");
        assert!(parse_system_df(&three).is_err());
        let doubled = format!("{SYSTEM_DF}{}", SYSTEM_DF.lines().next().unwrap());
        assert!(parse_system_df(&doubled).is_err());
        let unknown = SYSTEM_DF.replace("Build Cache", "Something");
        assert!(parse_system_df(&unknown).is_err());
        assert!(parse_system_df("json\njson\n").is_err());
        assert!(parse_system_df("").is_err());
    }

    #[test]
    fn usage_maps_every_failure_to_a_state() {
        assert!(matches!(usage(&ok(SYSTEM_DF)), DockerState::Ready(_)));
        assert_eq!(
            usage(&Recorded(Err(RunError::NotFound))),
            DockerState::NotInstalled
        );
        assert_eq!(
            usage(&failed(
                "Cannot connect to the Docker daemon at unix:///Users/me/.docker/run/docker.sock. Is the docker daemon running?"
            )),
            DockerState::NotRunning
        );
        assert!(matches!(
            usage(&failed("permission denied")),
            DockerState::Unavailable { .. }
        ));
        assert!(matches!(
            usage(&Recorded(Err(RunError::TimedOut))),
            DockerState::Unavailable { .. }
        ));
        assert!(matches!(
            usage(&ok("not json")),
            DockerState::Unavailable { .. }
        ));
    }

    #[test]
    fn newer_docker_not_running_messages_are_recognised() {
        // Docker CLI 28 and later.
        assert!(is_not_running(
            "failed to connect to the docker API at unix:///Users/me/.docker/run/docker.sock; check if the path is correct and if the daemon is running: dial unix /Users/me/.docker/run/docker.sock: connect: no such file or directory"
        ));
        assert!(is_not_running(
            "dial unix /Users/me/.docker/run/docker.sock: connect: connection refused"
        ));
        // OrbStack's socket, exactly as Sam's Mac printed it with OrbStack closed.
        assert!(is_not_running(
            "failed to connect to the docker API at unix:///Users/sam/.orbstack/run/docker.sock; check if the path is correct and if the daemon is running: dial unix /Users/sam/.orbstack/run/docker.sock: connect: no such file or directory"
        ));
        assert!(!is_not_running("permission denied"));
    }

    #[test]
    fn unknown_errors_are_shown_in_dockers_words_without_paths() {
        let state = usage(&failed(
            "WARNING: something minor\nerror: context \"colima\" not found: open /Users/me/.docker/contexts/meta/x/meta.json: no such file\n",
        ));
        let DockerState::Unavailable { reason } = state else {
            panic!("expected unavailable");
        };
        assert!(
            reason.starts_with("Docker said: error: context"),
            "{reason}"
        );
        assert!(!reason.contains('/'), "{reason}");
        assert_eq!(error_line("  \n"), None);
        let spaced =
            error_line(r#"open "/Users/Alice Smith/.docker/config.json": permission denied"#)
                .unwrap();
        assert_eq!(spaced, r#"open "…": permission denied"#);
        assert!(!spaced.contains("Smith"));
        assert_eq!(
            error_line(r#"context "colima" not found"#).unwrap(),
            r#"context "colima" not found"#
        );
        assert!(!error_line(r#"open "/Users/Alice Smith"#)
            .unwrap()
            .contains("Smith"));
        assert!(error_line(&"x".repeat(500)).unwrap().chars().count() <= 160);
    }

    #[test]
    fn unavailable_reasons_hold_no_path() {
        let state = usage(&failed("open /Users/me/.docker/config.json: denied"));
        let DockerState::Unavailable { reason } = state else {
            panic!("expected unavailable");
        };
        assert!(!reason.contains('/'));
    }

    #[test]
    fn dangling_images_are_read() {
        let out = r#"{"Containers":"N/A","CreatedAt":"2026-10-01 10:00:00 +0100 BST","CreatedSince":"5 weeks ago","Digest":"<none>","ID":"a1b2c3d4e5f6","Repository":"<none>","SharedSize":"N/A","Size":"1.05GB","Tag":"<none>","UniqueSize":"N/A","VirtualSize":"1.05GB"}
{"CreatedSince":"2 days ago","ID":"0f9e8d7c6b5a","Repository":"<none>","Size":"80.2MB","Tag":"<none>"}
"#;
        let images = parse_dangling_images(out).unwrap();
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].id, "a1b2c3d4e5f6");
        assert_eq!(images[0].size_bytes, 1_050_000_000);
        assert_eq!(images[0].created, "5 weeks ago");
        assert_eq!(parse_dangling_images(""), Ok(vec![]));
    }

    #[test]
    fn dangling_images_refuse_odd_ids_and_shapes() {
        assert!(parse_dangling_images(r#"{"ID":"../x","Size":"1B"}"#).is_err());
        assert!(parse_dangling_images(r#"{"ID":"abc","Size":"1B","X":{"a":"b"}}"#).is_err());
        assert!(parse_dangling_images(r#"{"ID":"abc"}"#).is_err());
        assert!(parse_dangling_images("json").is_err());
    }

    #[test]
    fn builder_du_reads_records_and_total() {
        let out = "ID                                              RECLAIMABLE     SIZE            LAST ACCESSED
m3hqlq4tjoh2c5ncjsqxsxbv1*                      true            1.188GB         2 weeks ago
z9y8x7w6v5u4t3s2r1q0p9o8n                       false           20.5MB          5 minutes ago
Shared:         0B
Private:        1.208GB
Reclaimable:    1.188GB
Total:          1.208GB
";
        let du = parse_builder_du(out).unwrap();
        assert_eq!(du.records.len(), 2);
        assert_eq!(du.records[0].id, "m3hqlq4tjoh2c5ncjsqxsxbv1");
        assert!(du.records[0].reclaimable);
        assert_eq!(du.records[0].last_used, "2 weeks ago");
        assert!(!du.records[1].reclaimable);
        assert_eq!(du.reclaimable_bytes, 1_188_000_000);
    }

    #[test]
    fn builder_du_with_no_cache() {
        assert_eq!(parse_builder_du(""), Ok(BuilderDu::default()));
        assert_eq!(
            parse_builder_du("Reclaimable:\t0B\nTotal:\t0B\n"),
            Ok(BuilderDu::default())
        );
    }

    #[test]
    fn builder_du_refuses_lines_it_does_not_know() {
        assert!(parse_builder_du("something odd here\nReclaimable: 0B\n").is_err());
        assert!(parse_builder_du("abc maybe 1GB now\nReclaimable: 1GB\n").is_err());
        assert!(parse_builder_du("abc true 1GB now\n").is_err());
    }

    #[test]
    fn docker_ids_are_plain() {
        assert!(is_docker_id("a1b2c3"));
        assert!(is_docker_id("sha256:a1b2c3"));
        assert!(!is_docker_id(""));
        assert!(!is_docker_id("a b"));
        assert!(!is_docker_id("-f"));
        assert!(!is_docker_id("../etc"));
    }

    #[test]
    fn search_path_includes_the_users_docker_folder() {
        let path = search_path(Path::new("/Users/me"));
        let folders: Vec<PathBuf> = std::env::split_paths(&path).collect();
        assert!(folders.contains(&PathBuf::from("/Users/me/.docker/bin")));
        assert!(folders.contains(&PathBuf::from("/opt/homebrew/bin")));
    }

    #[test]
    fn disk_image_size_is_read_without_following_links() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(disk_image_bytes(home.path()), None);
        let file = home.path().join(DISK_IMAGE);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, vec![1u8; 64 * 1024]).unwrap();
        assert!(disk_image_bytes(home.path()).unwrap() >= 64 * 1024);
    }

    #[test]
    fn disk_image_symlink_is_ignored() {
        let home = tempfile::tempdir().unwrap();
        let target = home.path().join("big");
        std::fs::write(&target, vec![1u8; 4096]).unwrap();
        let file = home.path().join(DISK_IMAGE);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&target, &file).unwrap();
        assert_eq!(disk_image_bytes(home.path()), None);
    }

    /// The real runner, against a stand-in `docker` script in a temp folder.
    #[test]
    fn real_docker_runs_the_query_with_an_argument_array() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("args.txt");
        let script = dir.path().join("docker");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nfor a in \"$@\"; do echo \"$a\" >> '{}'; done\nprintf '%s\\n' '{}'\n",
                log.display(),
                SYSTEM_DF
                    .lines()
                    .collect::<Vec<_>>()
                    .join("'\nprintf '%s\\n' '")
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let docker = RealDocker::with_search_path(dir.path().as_os_str().to_owned());
        let state = usage(&docker);
        assert_eq!(
            state,
            DockerState::Ready(parse_system_df(SYSTEM_DF).unwrap())
        );
        let args = std::fs::read_to_string(&log).unwrap();
        assert_eq!(args, "system\ndf\n--format\n{{json .}}\n");
    }

    #[test]
    fn real_docker_missing_is_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let docker = RealDocker::with_search_path(dir.path().as_os_str().to_owned());
        assert_eq!(usage(&docker), DockerState::NotInstalled);
    }
}
