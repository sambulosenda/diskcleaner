//! Xcode: DerivedData, iOS DeviceSupport, simulators and an archive.

use super::KB;
use crate::builder::{Builder, HOME_TOKEN};
use crate::content::{plist, Plist};
use crate::paths;
use crate::FixtureError;

/// Builds the Xcode part of the fixture.
pub fn build(b: &mut Builder) -> Result<(), FixtureError> {
    projects(b)?;
    derived_data(b)?;
    device_support(b)?;
    simulators(b)?;
    archive(b)
}

/// The two Xcode projects that live DerivedData folders point at.
fn projects(b: &mut Builder) -> Result<(), FixtureError> {
    b.text(
        &format!("{}/project.pbxproj", paths::XCODE_PROJECTS[0]),
        "// !$*UTF8*$!\n{ archiveVersion = 1; objectVersion = 77; }\n",
    )?;
    b.text(
        &format!("{}/contents.xcworkspacedata", paths::XCODE_PROJECTS[1]),
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Workspace version = \"1.0\">\n</Workspace>\n",
    )?;
    b.text("Developer/Weather/Weather/App.swift", "import SwiftUI\n")?;
    b.text("Developer/Notes/Notes/App.swift", "import SwiftUI\n")
}

/// Two DerivedData folders whose workspace exists, one whose workspace does not.
fn derived_data(b: &mut Builder) -> Result<(), FixtureError> {
    let folders = [
        (paths::DERIVED_DATA_LIVE_A, paths::XCODE_PROJECTS[0]),
        (paths::DERIVED_DATA_LIVE_B, paths::XCODE_PROJECTS[1]),
        (paths::DERIVED_DATA_ORPHAN, paths::XCODE_MISSING_PROJECT),
    ];
    for (dir, workspace) in folders {
        // Xcode writes the absolute path, so this file holds the home path.
        let workspace_path = format!("{HOME_TOKEN}/{workspace}");
        let info = plist(&[
            ("LastAccessedDate", Plist::Date("2026-09-20T14:03:11Z")),
            ("WorkspacePath", Plist::Str(&workspace_path)),
        ]);
        b.text_with_home(&format!("{dir}/info.plist"), &info)?;
        b.bytes(
            &format!("{dir}/Build/Intermediates.noindex/App.build/Objects.o"),
            48 * KB,
        )?;
        b.bytes(
            &format!("{dir}/Build/Products/Debug-iphonesimulator/App.app/App"),
            32 * KB,
        )?;
        b.bytes(
            &format!("{dir}/Index.noindex/DataStore/v5/units/unit-0001"),
            8 * KB,
        )?;
        b.dir(&format!("{dir}/Logs/Build"))?;
    }
    Ok(())
}

/// iOS DeviceSupport folders for three versions.
fn device_support(b: &mut Builder) -> Result<(), FixtureError> {
    for version in paths::IOS_DEVICE_SUPPORT_VERSIONS {
        let dir = format!("{}/{version}", paths::IOS_DEVICE_SUPPORT);
        b.text(
            &format!("{dir}/Info.plist"),
            &plist(&[("DSVersion", Plist::Str(version))]),
        )?;
        b.bytes(
            &format!("{dir}/Symbols/System/Library/Frameworks/UIKit.framework/UIKit"),
            40 * KB,
        )?;
        b.bytes(&format!("{dir}/Symbols/usr/lib/dyld"), 12 * KB)?;
    }
    Ok(())
}

/// Three simulator devices; the third one's runtime is gone, so it is unavailable.
fn simulators(b: &mut Builder) -> Result<(), FixtureError> {
    let devices = [
        (paths::SIM_DEVICE_IDS[0], "iPhone 16", "iOS-18-1", true),
        (
            paths::SIM_DEVICE_IDS[1],
            "iPad Air 11-inch (M2)",
            "iOS-18-1",
            true,
        ),
        (paths::SIM_DEVICE_IDS[2], "iPhone 12", "iOS-15-5", false),
    ];
    for (udid, name, runtime, available) in devices {
        let dir = format!("{}/{udid}", paths::SIM_DEVICES);
        let runtime_id = format!("com.apple.CoreSimulator.SimRuntime.{runtime}");
        let mut entries = vec![
            ("UDID", Plist::Str(udid)),
            ("name", Plist::Str(name)),
            ("runtime", Plist::Str(&runtime_id)),
            (
                "deviceType",
                Plist::Str("com.apple.CoreSimulator.SimDeviceType.iPhone-16"),
            ),
            ("state", Plist::Int(1)),
            ("isAvailable", Plist::Bool(available)),
        ];
        if !available {
            entries.push(("availabilityError", Plist::Str("runtime profile not found")));
        }
        b.text(&format!("{dir}/device.plist"), &plist(&entries))?;
        b.bytes(
            &format!("{dir}/data/Library/Caches/com.apple.mobile.installd.staging/blob"),
            16 * KB,
        )?;
        b.bytes(
            &format!("{dir}/data/Containers/Data/Application/AAAA0001/Documents/app.db"),
            8 * KB,
        )?;
    }
    b.text(
        &format!("{}/device_set.plist", paths::SIM_DEVICES),
        &plist(&[("Version", Plist::Int(2))]),
    )
}

/// One Xcode archive (review-only).
fn archive(b: &mut Builder) -> Result<(), FixtureError> {
    let dir = paths::XCODE_ARCHIVE;
    let info = plist(&[
        ("ArchiveVersion", Plist::Int(2)),
        ("CreationDate", Plist::Date("2026-03-14T10:42:00Z")),
        ("Name", Plist::Str("Weather")),
        ("SchemeName", Plist::Str("Weather")),
    ]);
    b.text(&format!("{dir}/Info.plist"), &info)?;
    b.bytes(
        &format!("{dir}/Products/Applications/Weather.app/Weather"),
        48 * KB,
    )?;
    b.bytes(
        &format!("{dir}/dSYMs/Weather.app.dSYM/Contents/Resources/DWARF/Weather"),
        32 * KB,
    )
}
