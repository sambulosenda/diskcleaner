//! Everyday files: iPhone backups, Downloads, app caches, Photos, Messages.

use super::KB;
use crate::builder::Builder;
use crate::content::{plist, Plist};
use crate::paths;
use crate::FixtureError;

/// Builds the everyday part of the fixture.
pub fn build(b: &mut Builder) -> Result<(), FixtureError> {
    backups(b)?;
    downloads(b)?;
    caches(b)?;
    photos(b)?;
    messages(b)?;
    documents(b)
}

/// Two iPhone or iPad backups, each with a valid `Info.plist`.
fn backups(b: &mut Builder) -> Result<(), FixtureError> {
    let devices = [
        (
            paths::BACKUP_DEVICE_IDS[0],
            "Sam's iPhone",
            "iPhone14,2",
            "18.1",
            "2026-10-30T08:15:00Z",
        ),
        (
            paths::BACKUP_DEVICE_IDS[1],
            "Family iPad",
            "iPad13,1",
            "17.6.1",
            "2025-12-02T19:40:00Z",
        ),
    ];
    for (id, name, product, version, date) in devices {
        let dir = format!("{}/{id}", paths::MOBILESYNC_BACKUP);
        let info = plist(&[
            ("Device Name", Plist::Str(name)),
            ("Display Name", Plist::Str(name)),
            ("Product Type", Plist::Str(product)),
            ("Product Version", Plist::Str(version)),
            ("Target Identifier", Plist::Str(id)),
            ("Unique Identifier", Plist::Str(id)),
            ("Last Backup Date", Plist::Date(date)),
        ]);
        b.text(&format!("{dir}/Info.plist"), &info)?;
        b.bytes(&format!("{dir}/Manifest.db"), 16 * KB)?;
        b.bytes(&format!("{dir}/Manifest.plist"), 2 * KB)?;
        b.bytes(&format!("{dir}/Status.plist"), KB / 2)?;
        b.bytes(
            &format!("{dir}/3d/3d0d7e5fb2ce288813306e4d4636395e047a3d28"),
            24 * KB,
        )?;
        b.bytes(
            &format!("{dir}/a1/a1b2c3d4e5f60718293a4b5c6d7e8f9012345678"),
            12 * KB,
        )?;
    }
    Ok(())
}

/// Downloads with a disk image, an installer package, a zip and a normal file.
fn downloads(b: &mut Builder) -> Result<(), FixtureError> {
    b.bytes(paths::DOWNLOAD_DMG, 96 * KB)?;
    b.bytes(paths::DOWNLOAD_PKG, 64 * KB)?;
    b.bytes(paths::DOWNLOAD_ZIP, 48 * KB)?;
    b.text(
        paths::DOWNLOAD_NORMAL,
        "Agenda\n- Budget review\n- Holiday rota\n",
    )
}

/// Three app cache folders with a few files each.
fn caches(b: &mut Builder) -> Result<(), FixtureError> {
    for id in paths::CACHE_APP_IDS {
        let dir = format!("{}/{id}", paths::CACHES);
        b.bytes(&format!("{dir}/Cache.db"), 32 * KB)?;
        b.bytes(&format!("{dir}/fsCachedData/0A1B2C3D"), 20 * KB)?;
        b.bytes(&format!("{dir}/fsCachedData/4E5F6071"), 8 * KB)?;
    }
    Ok(())
}

/// A Photos library bundle (hard-blocked as a whole).
fn photos(b: &mut Builder) -> Result<(), FixtureError> {
    let lib = paths::PHOTOS_LIBRARY;
    b.bytes(&format!("{lib}/database/Photos.sqlite"), 40 * KB)?;
    b.bytes(
        &format!("{lib}/originals/0/0A1B2C3D-IMG_0001.heic"),
        64 * KB,
    )?;
    b.bytes(
        &format!("{lib}/originals/1/1B2C3D4E-IMG_0002.heic"),
        64 * KB,
    )?;
    b.bytes(
        &format!("{lib}/resources/derivatives/0/0A1B2C3D_1_105_c.jpeg"),
        16 * KB,
    )?;
    b.dir(&format!("{lib}/private"))
}

/// Messages attachments (review-only) and the Messages database beside them.
fn messages(b: &mut Builder) -> Result<(), FixtureError> {
    let att = paths::MESSAGES_ATTACHMENTS;
    b.bytes(
        &format!("{att}/0a/10/7C1D2E3F-AAAA-4BBB-8CCC-000000000001/IMG_2041.heic"),
        56 * KB,
    )?;
    b.bytes(
        &format!("{att}/3f/15/7C1D2E3F-AAAA-4BBB-8CCC-000000000002/voice-note.caf"),
        24 * KB,
    )?;
    b.bytes("Library/Messages/chat.db", 32 * KB)
}

/// Personal documents (the Documents symlink trap points here).
fn documents(b: &mut Builder) -> Result<(), FixtureError> {
    b.text(
        "Documents/Letters/Landlord.txt",
        "Dear Ms Patel,\nThe boiler is fixed, thank you.\n",
    )?;
    b.bytes("Documents/Taxes/2025 return.pdf", 28 * KB)?;
    b.bytes("Desktop/Screenshot 2026-10-01 at 09.12.33.png", 36 * KB)
}
