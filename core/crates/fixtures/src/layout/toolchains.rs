//! More developer tools: Python (pip, uv, Poetry), Go, Homebrew, web tools
//! (Deno, pnpm, node-gyp, TypeScript, Electron, Playwright, Puppeteer),
//! Maven, Dart, and the Xcode, SwiftPM and Carthage caches. Each area also
//! has files next to the caches that no rule may match.

use super::KB;
use crate::builder::Builder;
use crate::paths;
use crate::FixtureError;

/// Builds the toolchains part of the fixture.
pub fn build(b: &mut Builder) -> Result<(), FixtureError> {
    python(b)?;
    go(b)?;
    web_tools(b)?;
    java_and_dart(b)?;
    apple(b)
}

/// pip and uv caches, a Python uv installed, Poetry's downloads and
/// environments, and a Poetry folder that is not an environment.
fn python(b: &mut Builder) -> Result<(), FixtureError> {
    b.bytes(&format!("{}/http-v2/a/b/c/body", paths::PIP_CACHE), 16 * KB)?;
    b.text(
        &format!("{}/CACHEDIR.TAG", paths::UV_CACHE),
        "Signature: 8a477f597d28d172789f06886806bc55\n",
    )?;
    b.bytes(
        &format!(
            "{}/wheels-v5/pypi/requests/requests-2.32.3.whl",
            paths::UV_CACHE
        ),
        16 * KB,
    )?;
    b.bytes(
        &format!(
            "{}/cpython-3.12.7-macos-aarch64-none/bin/python3",
            paths::UV_PYTHONS
        ),
        16 * KB,
    )?;
    let poetry = paths::POETRY_HOME;
    b.bytes(
        &format!("{poetry}/cache/repositories/PyPI/_http/a/b"),
        8 * KB,
    )?;
    b.bytes(
        &format!("{poetry}/artifacts/1a/2b/requests-2.32.3.whl"),
        16 * KB,
    )?;
    for venv in paths::POETRY_VIRTUALENVS {
        let dir = format!("{poetry}/virtualenvs/{venv}");
        b.text(
            &format!("{dir}/pyvenv.cfg"),
            "home = /usr/bin\nversion = 3.12.7\n",
        )?;
        b.bytes(&format!("{dir}/bin/python"), 8 * KB)?;
    }
    b.text(
        &format!("{}/todo.txt", paths::TRAP_POETRY_NOT_A_VENV),
        "Buy milk.\n",
    )
}

/// Go's build and module caches, an installed tool, and old GOPATH code.
fn go(b: &mut Builder) -> Result<(), FixtureError> {
    b.text(
        &format!("{}/README", paths::GO_BUILD_CACHE),
        "This directory holds cached build artifacts from the Go build system.\n",
    )?;
    b.bytes(&format!("{}/0a/0a1b2c-d", paths::GO_BUILD_CACHE), 16 * KB)?;
    let modules = paths::GO_MODULE_CACHE;
    b.bytes(
        &format!("{modules}/cache/download/golang.org/x/text/@v/v0.20.0.zip"),
        16 * KB,
    )?;
    b.text(
        &format!("{modules}/golang.org/x/text@v0.20.0/go.mod"),
        "module golang.org/x/text\n",
    )?;
    b.bytes(paths::GO_BIN_TOOL, 16 * KB)?;
    b.text(paths::GO_OWN_CODE, "package main\n\nfunc main() {}\n")
}

/// Homebrew's downloads and the web tool caches and browsers.
fn web_tools(b: &mut Builder) -> Result<(), FixtureError> {
    b.bytes(
        &format!(
            "{}/downloads/1a2b--wget-1.24.5.bottle.tar.gz",
            paths::HOMEBREW_CACHE
        ),
        16 * KB,
    )?;
    for cache in paths::WEB_TOOL_CACHES {
        b.bytes(&format!("{cache}/data_0"), 8 * KB)?;
    }
    for browser in paths::PLAYWRIGHT_BROWSERS {
        b.bytes(
            &format!("{}/{browser}/INSTALLATION_COMPLETE", paths::PLAYWRIGHT_HOME),
            KB,
        )?;
        b.bytes(
            &format!("{}/{browser}/browser.bin", paths::PLAYWRIGHT_HOME),
            16 * KB,
        )?;
    }
    b.text(
        &format!("{}/0a1b2c3d", paths::PLAYWRIGHT_LINKS),
        "/Users/sam/projects/a\n",
    )?;
    for browser in paths::PUPPETEER_BROWSERS {
        b.bytes(
            &format!(
                "{}/{browser}/mac_arm-131.0.6778.85/browser.bin",
                paths::PUPPETEER_HOME
            ),
            16 * KB,
        )?;
    }
    Ok(())
}

/// Maven's libraries and settings, and Dart's packages and a global tool.
fn java_and_dart(b: &mut Builder) -> Result<(), FixtureError> {
    b.bytes(
        &format!(
            "{}/org/slf4j/slf4j-api/2.0.16/slf4j-api-2.0.16.jar",
            paths::MAVEN_REPOSITORY
        ),
        16 * KB,
    )?;
    b.text(
        paths::MAVEN_SETTINGS,
        "<settings><servers><server><id>corp</id></server></servers></settings>\n",
    )?;
    b.bytes(
        &format!("{}/pub.dev/http-1.2.2/lib/http.dart", paths::PUB_HOSTED),
        8 * KB,
    )?;
    b.bytes(paths::PUB_GLOBAL_TOOL, 8 * KB)
}

/// Xcode's cache, preview data and key bindings, the simulator cache, and
/// the SwiftPM and Carthage caches next to SwiftPM's fingerprints.
fn apple(b: &mut Builder) -> Result<(), FixtureError> {
    b.bytes(&format!("{}/Cache.db", paths::XCODE_CACHE), 8 * KB)?;
    b.bytes(
        &format!("{}/Products/Weather.app/Weather", paths::XCODE_PREVIEWS),
        16 * KB,
    )?;
    b.text(paths::XCODE_KEY_BINDINGS, "<plist></plist>\n")?;
    b.bytes(
        &format!(
            "{}/dyld/24A335/dyld_sim_shared_cache_arm64e",
            paths::SIMULATOR_CACHES
        ),
        16 * KB,
    )?;
    b.bytes(
        &format!(
            "{}/repositories/swift-log-1a2b3c/packed",
            paths::SWIFTPM_CACHE
        ),
        16 * KB,
    )?;
    b.text(paths::SWIFTPM_FINGERPRINTS, "{}\n")?;
    b.bytes(
        &format!("{}/dependencies/Alamofire/5.9.1.zip", paths::CARTHAGE_CACHE),
        16 * KB,
    )
}
