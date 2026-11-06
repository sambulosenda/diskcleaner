//! Rust, Claude Code and code editors: Cargo and rustup downloads, `target`
//! build folders, Claude Code conversations and logs, and the caches of
//! Cursor, VS Code and a JetBrains IDE. Each area also has files next to
//! the caches that no rule may match.

use super::KB;
use crate::builder::Builder;
use crate::paths;
use crate::FixtureError;

/// Builds the developer tools part of the fixture.
pub fn build(b: &mut Builder) -> Result<(), FixtureError> {
    cargo(b)?;
    rustup(b)?;
    rust_projects(b)?;
    claude_code(b)?;
    editors(b)
}

/// Cargo's download cache, unpacked crates, an installed tool, a sign-in
/// file and a git checkout.
fn cargo(b: &mut Builder) -> Result<(), FixtureError> {
    let index = "index.crates.io-6f17d22bba15001f";
    b.bytes(
        &format!(
            "{}/{index}/serde-1.0.210.crate",
            paths::CARGO_REGISTRY_CACHE
        ),
        24 * KB,
    )?;
    b.text(
        &format!(
            "{}/{index}/serde-1.0.210/src/lib.rs",
            paths::CARGO_REGISTRY_SRC
        ),
        "//! Serde\npub mod de;\npub mod ser;\n",
    )?;
    b.bytes(paths::CARGO_BIN, 32 * KB)?;
    b.text(
        paths::CARGO_CREDENTIALS,
        "[registry]\ntoken = \"fake-token-for-tests\"\n",
    )?;
    let checkout = paths::CARGO_GIT_CHECKOUT;
    b.text(&format!("{checkout}/.git/HEAD"), "ref: refs/heads/main\n")?;
    b.text(
        &format!("{checkout}/Cargo.toml"),
        "[package]\nname = \"tokio\"\n",
    )
}

/// Two rustup toolchains and rustup's settings.
fn rustup(b: &mut Builder) -> Result<(), FixtureError> {
    for name in paths::RUSTUP_TOOLCHAIN_NAMES {
        let dir = format!("{}/{name}", paths::RUSTUP_TOOLCHAINS);
        b.text(
            &format!("{dir}/lib/rustlib/components"),
            "rustc-x86_64\ncargo-x86_64\nrust-std-x86_64\n",
        )?;
        b.bytes(&format!("{dir}/bin/rustc"), 32 * KB)?;
    }
    b.text(
        paths::RUSTUP_SETTINGS,
        "default_toolchain = \"stable-aarch64-apple-darwin\"\nversion = \"12\"\n",
    )
}

/// One Rust project per code folder, each with its `target` folder, plus
/// two `target` folders that are not Cargo's.
fn rust_projects(b: &mut Builder) -> Result<(), FixtureError> {
    for project in paths::RUST_PROJECTS {
        b.text(
            &format!("{project}/Cargo.toml"),
            "[package]\nname = \"example\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )?;
        b.text(&format!("{project}/src/main.rs"), "fn main() {}\n")?;
        let target = format!("{project}/target");
        b.text(
            &format!("{target}/CACHEDIR.TAG"),
            "Signature: 8a477f597d28d172789f06886806bc55\n",
        )?;
        b.text(
            &format!("{target}/.rustc_info.json"),
            "{\"rustc_fingerprint\":1}\n",
        )?;
        b.bytes(&format!("{target}/debug/example"), 32 * KB)?;
        b.bytes(
            &format!("{target}/debug/deps/libserde-0a1b2c.rlib"),
            16 * KB,
        )?;
    }
    // The Developer project is a git repository; its target folder is the
    // allowed build-output child.
    b.text(
        &format!("{}/.git/HEAD", paths::RUST_PROJECTS[1]),
        "ref: refs/heads/main\n",
    )?;
    b.text(
        &format!("{}/Q3 targets.txt", paths::TRAP_PLAIN_TARGET),
        "Grow repeat orders by 10%.\n",
    )?;
    let half = paths::TRAP_HALF_TARGET;
    b.text(
        &format!("{half}/CACHEDIR.TAG"),
        "Signature: 8a477f597d28d172789f06886806bc55\n",
    )?;
    b.bytes(&format!("{half}/report.bin"), 8 * KB)
}

/// Claude Code conversations for one project, its settings, and its logs.
fn claude_code(b: &mut Builder) -> Result<(), FixtureError> {
    let conv = paths::CLAUDE_CONVERSATIONS;
    b.text(
        &format!("{conv}/0b1c2d3e-4f50-4a6b-8c7d-9e0f1a2b3c4d.jsonl"),
        "{\"type\":\"user\",\"message\":\"Add a test\"}\n",
    )?;
    b.text(paths::CLAUDE_SETTINGS, "{\"model\":\"default\"}\n")?;
    b.text(
        &format!("{}/CLAUDE.md", paths::CLAUDE_HOME),
        "Use British English.\n",
    )?;
    b.bytes(
        &format!(
            "{}/-Users-sam-Developer-roomy-core/mcp-logs-github/2026-11-01.txt",
            paths::CLAUDE_LOGS
        ),
        12 * KB,
    )
}

/// The three cache folders of Cursor and VS Code, Cursor's chat database,
/// VS Code's settings, and one JetBrains IDE cache.
fn editors(b: &mut Builder) -> Result<(), FixtureError> {
    for app in [paths::CURSOR_DATA, paths::VSCODE_DATA] {
        for folder in paths::EDITOR_CACHE_FOLDERS {
            b.bytes(&format!("{app}/{folder}/data_0"), 16 * KB)?;
        }
        b.text(&format!("{app}/User/settings.json"), "{}\n")?;
    }
    b.bytes(paths::CURSOR_CHAT_DB, 24 * KB)?;
    b.bytes(
        &format!("{}/caches/content.dat", paths::JETBRAINS_CACHE),
        16 * KB,
    )
}
