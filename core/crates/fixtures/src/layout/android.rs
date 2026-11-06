//! Android: SDK parts, AVDs, two projects and Gradle wrapper downloads.

use super::KB;
use crate::builder::{Builder, HOME_TOKEN};
use crate::paths;
use crate::FixtureError;

/// Builds the Android part of the fixture.
pub fn build(b: &mut Builder) -> Result<(), FixtureError> {
    sdk(b)?;
    avds(b)?;
    projects(b)?;
    gradle(b)
}

/// System images for API 33, 34 and 35, two build-tools versions, one NDK.
fn sdk(b: &mut Builder) -> Result<(), FixtureError> {
    let sdk = paths::ANDROID_SDK;
    for api in paths::ANDROID_SYSTEM_IMAGE_APIS {
        let dir = format!("{sdk}/system-images/android-{api}/google_apis/arm64-v8a");
        b.text(
            &format!("{dir}/source.properties"),
            &format!("AndroidVersion.ApiLevel={api}\nSystemImage.TagId=google_apis\n"),
        )?;
        b.bytes(&format!("{dir}/system.img"), 64 * KB)?;
    }
    for version in paths::ANDROID_BUILD_TOOLS {
        let dir = format!("{sdk}/build-tools/{version}");
        b.text(
            &format!("{dir}/source.properties"),
            &format!("Pkg.Revision={version}\n"),
        )?;
        b.bytes(&format!("{dir}/aapt2"), 24 * KB)?;
    }
    let ndk = format!("{sdk}/ndk/{}", paths::ANDROID_NDK);
    b.text(
        &format!("{ndk}/source.properties"),
        &format!("Pkg.Revision = {}\n", paths::ANDROID_NDK),
    )?;
    b.bytes(
        &format!("{ndk}/toolchains/llvm/prebuilt/darwin-x86_64/bin/clang"),
        48 * KB,
    )?;
    b.text(
        &format!("{sdk}/platforms/android-34/source.properties"),
        "AndroidVersion.ApiLevel=34\n",
    )
}

/// Two AVDs, each a `.avd` folder plus an `.ini` that points at it.
fn avds(b: &mut Builder) -> Result<(), FixtureError> {
    let apis = [34, 33];
    for (name, api) in paths::ANDROID_AVD_NAMES.iter().zip(apis) {
        let avd = paths::ANDROID_AVD;
        // The emulator writes an absolute path, so this file holds the home path.
        let ini = format!(
            "avd.ini.encoding=UTF-8\npath={HOME_TOKEN}/{avd}/{name}.avd\npath.rel=avd/{name}.avd\ntarget=android-{api}\n"
        );
        b.text_with_home(&format!("{avd}/{name}.ini"), &ini)?;
        let dir = format!("{avd}/{name}.avd");
        b.text(
            &format!("{dir}/config.ini"),
            &format!("image.sysdir.1=system-images/android-{api}/google_apis/arm64-v8a/\n"),
        )?;
        b.bytes(&format!("{dir}/userdata-qemu.img"), 64 * KB)?;
        b.bytes(&format!("{dir}/snapshots/default_boot/ram.bin"), 32 * KB)?;
    }
    Ok(())
}

/// Two projects whose `build.gradle` files reference API 34 only.
fn projects(b: &mut Builder) -> Result<(), FixtureError> {
    for project in paths::ANDROID_PROJECTS {
        b.text(&format!("{project}/settings.gradle"), "include ':app'\n")?;
        b.text(
            &format!("{project}/build.gradle"),
            "plugins {\n    id 'com.android.application' version '8.5.0' apply false\n}\n",
        )?;
        b.text(
            &format!("{project}/app/build.gradle"),
            concat!(
                "android {\n",
                "    compileSdk 34\n",
                "    defaultConfig {\n",
                "        minSdk 26\n",
                "        targetSdk 34\n",
                "    }\n",
                "}\n",
            ),
        )?;
        b.bytes(
            &format!("{project}/app/build/outputs/apk/debug/app-debug.apk"),
            40 * KB,
        )?;
        b.bytes(
            &format!("{project}/.gradle/8.7/executionHistory/executionHistory.bin"),
            8 * KB,
        )?;
    }
    Ok(())
}

/// Three Gradle wrapper distributions.
fn gradle(b: &mut Builder) -> Result<(), FixtureError> {
    let hashes = [
        "5t9huq95ubn472n8rpzujfbqh",
        "6o1ggb6v1yvblwdtahcvjvtlh",
        "6gdy1pgp427xkqcjbxw3ylt6h",
    ];
    for (version, hash) in paths::GRADLE_VERSIONS.iter().zip(hashes) {
        let dir = format!("{}/{version}/{hash}", paths::GRADLE_DISTS);
        b.bytes(&format!("{dir}/{version}.zip"), 48 * KB)?;
        b.text(&format!("{dir}/{version}.zip.ok"), "")?;
    }
    Ok(())
}
