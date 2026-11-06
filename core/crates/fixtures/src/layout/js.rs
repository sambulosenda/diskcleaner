//! JavaScript: three projects with `node_modules`, and package manager caches.

use super::KB;
use crate::builder::Builder;
use crate::paths;
use crate::FixtureError;

/// Builds the JavaScript part of the fixture.
pub fn build(b: &mut Builder) -> Result<(), FixtureError> {
    projects(b)?;
    projects_elsewhere(b)?;
    caches(b)
}

/// Projects a, b and c. Project c's `package.json` is 120 days old.
fn projects(b: &mut Builder) -> Result<(), FixtureError> {
    for (project, name) in
        paths::JS_PROJECTS
            .iter()
            .zip(["shop-site", "admin-panel", "old-prototype"])
    {
        b.text(
            &format!("{project}/package.json"),
            &format!(
                "{{\n  \"name\": \"{name}\",\n  \"version\": \"1.0.0\",\n  \"dependencies\": {{\n    \"left-pad\": \"1.3.0\"\n  }}\n}}\n"
            ),
        )?;
        b.text(&format!("{project}/index.js"), "console.log('hello');\n")?;
        let modules = format!("{project}/node_modules");
        b.text(
            &format!("{modules}/left-pad/package.json"),
            "{ \"name\": \"left-pad\", \"version\": \"1.3.0\" }\n",
        )?;
        b.bytes(&format!("{modules}/left-pad/index.js"), 4 * KB)?;
        b.bytes(&format!("{modules}/.bin/tool"), 2 * KB)?;
        b.bytes(&format!("{modules}/.package-lock.json"), 3 * KB)?;
    }
    b.set_age_days(paths::JS_STALE_PACKAGE_JSON, paths::JS_STALE_AGE_DAYS)
}

/// Projects outside `projects/`: one in Developer with a package nested
/// inside its `node_modules`, one in a monorepo under code, and one in
/// Documents.
fn projects_elsewhere(b: &mut Builder) -> Result<(), FixtureError> {
    let all = paths::JS_PROJECTS_ELSEWHERE
        .iter()
        .chain([paths::JS_PROJECT_ON_DESKTOP].iter());
    for project in all {
        b.text(
            &format!("{project}/package.json"),
            "{ \"name\": \"site\", \"version\": \"1.0.0\" }\n",
        )?;
        b.bytes(&format!("{project}/node_modules/left-pad/index.js"), 4 * KB)?;
        b.bytes(
            &format!("{project}/node_modules/.package-lock.json"),
            3 * KB,
        )?;
    }
    b.bytes(
        &format!("{}/inner/index.js", paths::JS_NESTED_MODULES),
        2 * KB,
    )
}

/// npm, npx, pnpm and Yarn caches.
fn caches(b: &mut Builder) -> Result<(), FixtureError> {
    b.bytes(
        &format!(
            "{}/_cacache/content-v2/sha512/ab/cd/abcdef0123",
            paths::NPM_CACHE
        ),
        24 * KB,
    )?;
    b.bytes(
        &format!("{}/_cacache/index-v5/12/34/1234abcd", paths::NPM_CACHE),
        2 * KB,
    )?;
    b.text(
        &format!("{}/package.json", paths::NPX_TOOL),
        "{ \"dependencies\": { \"create-site\": \"1.0.0\" } }\n",
    )?;
    b.bytes(
        &format!("{}/node_modules/create-site/index.js", paths::NPX_TOOL),
        12 * KB,
    )?;
    b.bytes(
        &format!(
            "{}/store/v3/files/00/0011223344-index.json",
            paths::PNPM_STORE
        ),
        2 * KB,
    )?;
    b.bytes(
        &format!("{}/store/v3/files/ff/ffeeddccbb", paths::PNPM_STORE),
        20 * KB,
    )?;
    b.bytes(
        &format!(
            "{}/v6/npm-left-pad-1.3.0-abc123/node_modules/left-pad/index.js",
            paths::YARN_CACHE
        ),
        4 * KB,
    )?;
    b.text(&format!("{}/v6/.tmp/.keep", paths::YARN_CACHE), "")?;
    b.bytes(
        &format!("{}/left-pad@1.3.0@@@1/index.js", paths::BUN_CACHE),
        4 * KB,
    )?;
    for cache in paths::EXPO_CACHES {
        b.bytes(&format!("{}/{cache}/entry-0001", paths::EXPO_HOME), 6 * KB)?;
    }
    b.text(paths::EXPO_STATE, "{ \"auth\": null }\n")
}
