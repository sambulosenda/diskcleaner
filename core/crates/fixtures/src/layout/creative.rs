//! Video app files: a Final Cut Pro library with render and proxy files,
//! the Adobe media cache and After Effects' disk cache.

use super::KB;
use crate::builder::Builder;
use crate::paths;
use crate::FixtureError;

/// Builds the creative part of the fixture.
pub fn build(b: &mut Builder) -> Result<(), FixtureError> {
    final_cut(b)?;
    adobe(b)
}

/// A Final Cut library: the project data stays, render and proxy files can go.
fn final_cut(b: &mut Builder) -> Result<(), FixtureError> {
    let lib = paths::FCP_LIBRARY;
    b.bytes(&format!("{lib}/Settings.plist"), 2 * KB)?;
    for event in paths::FCP_EVENTS {
        let e = format!("{lib}/{event}");
        b.bytes(&format!("{e}/CurrentVersion.fcpevent"), 8 * KB)?;
        b.bytes(&format!("{e}/Original Media/clip.mov"), 48 * KB)?;
        b.bytes(
            &format!("{e}/Render Files/High Quality Media/render-0001.mov"),
            32 * KB,
        )?;
        b.bytes(
            &format!("{e}/Transcoded Media/Proxy Media/clip.mov"),
            16 * KB,
        )?;
    }
    Ok(())
}

/// Adobe's shared media cache and After Effects' disk cache.
fn adobe(b: &mut Builder) -> Result<(), FixtureError> {
    b.bytes(
        &format!("{}/clip.mov 48000.cfa", paths::ADOBE_MEDIA_CACHE_FILES),
        24 * KB,
    )?;
    b.bytes(
        &format!("{}/clip.mov 48000.pek", paths::ADOBE_MEDIA_CACHE_FILES),
        4 * KB,
    )?;
    b.bytes(
        &format!("{}/Media Cache Database", paths::ADOBE_MEDIA_CACHE_DB),
        8 * KB,
    )?;
    for v in paths::AFTER_EFFECTS_VERSIONS {
        b.bytes(
            &format!("{}/{v}/Disk Cache/0001.htrn", paths::AFTER_EFFECTS_CACHE),
            16 * KB,
        )?;
    }
    Ok(())
}
