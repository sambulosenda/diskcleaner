//! Which apps own the items each rule finds, so the executor can skip an
//! item while its app is open.
//!
//! Pure data. A program matches an [`OwnerApp`] when its executable's file
//! name is one of `programs`, or its path passes through one of `bundles`
//! (for example `/Applications/Xcode.app/Contents/MacOS/Xcode`).
//!
//! Rules with no entry here have no owner check. Leaving a rule out only
//! means one fewer reason to skip, never a reason to clean something. Very
//! common programs (such as `node`, `java` or `ruby`) are left out on
//! purpose: almost any developer has one running, so they would hide every
//! item without telling the user which app to quit.

/// An app that may be using the items of a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerApp {
    /// The name shown to the user, for example "Xcode".
    pub name: &'static str,
    /// Executable file names that count as this app running.
    pub programs: &'static [&'static str],
    /// App bundle folder names that count as this app running.
    pub bundles: &'static [&'static str],
    /// Name starts that count as this app running, for apps whose name
    /// carries a year or version (for example "Adobe Premiere Pro 2025").
    /// Matched against the executable's file name and every folder name in
    /// its path, ignoring ASCII case.
    pub prefixes: &'static [&'static str],
}

/// An installed app, named at run time, that the app uninstaller is
/// removing. Unlike [`OwnerApp`], which is a fixed table for cleanup rules,
/// this comes from the app's own bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppBundle {
    /// The app's name, shown to the user, for example "Slack".
    pub name: String,
    /// Where the app's bundle is, for example `/Applications/Slack.app`.
    /// Read only: used to tell whether a program inside it is running.
    pub bundle: std::path::PathBuf,
}

const XCODE: OwnerApp = OwnerApp {
    name: "Xcode",
    programs: &["Xcode", "xcodebuild"],
    bundles: &["Xcode.app", "Xcode-beta.app"],
    prefixes: &[],
};

const SIMULATOR: OwnerApp = OwnerApp {
    name: "Simulator",
    programs: &["Simulator"],
    bundles: &["Simulator.app"],
    prefixes: &[],
};

const ANDROID_STUDIO: OwnerApp = OwnerApp {
    name: "Android Studio",
    programs: &["studio"],
    bundles: &["Android Studio.app", "Android Studio Preview.app"],
    prefixes: &[],
};

const ANDROID_EMULATOR: OwnerApp = OwnerApp {
    name: "the Android emulator",
    programs: &[
        "emulator",
        "qemu-system-aarch64",
        "qemu-system-x86_64",
        "qemu-system-aarch64-headless",
        "qemu-system-x86_64-headless",
    ],
    bundles: &[],
    prefixes: &[],
};

const GRADLE: OwnerApp = OwnerApp {
    name: "Gradle",
    programs: &["gradle", "gradlew"],
    bundles: &[],
    prefixes: &[],
};

const SAFARI: OwnerApp = OwnerApp {
    name: "Safari",
    programs: &["Safari"],
    bundles: &["Safari.app", "Safari Technology Preview.app"],
    prefixes: &[],
};

const CHROME: OwnerApp = OwnerApp {
    name: "Google Chrome",
    programs: &["Google Chrome"],
    bundles: &["Google Chrome.app", "Google Chrome Canary.app"],
    prefixes: &[],
};

const FIREFOX: OwnerApp = OwnerApp {
    name: "Firefox",
    programs: &["firefox"],
    bundles: &[
        "Firefox.app",
        "Firefox Developer Edition.app",
        "Firefox Nightly.app",
    ],
    prefixes: &[],
};

const SPOTIFY: OwnerApp = OwnerApp {
    name: "Spotify",
    programs: &["Spotify"],
    bundles: &["Spotify.app"],
    prefixes: &[],
};

const SLACK: OwnerApp = OwnerApp {
    name: "Slack",
    programs: &["Slack"],
    bundles: &["Slack.app"],
    prefixes: &[],
};

const CURSOR: OwnerApp = OwnerApp {
    name: "Cursor",
    programs: &["Cursor"],
    bundles: &["Cursor.app"],
    prefixes: &[],
};

const VSCODE: OwnerApp = OwnerApp {
    name: "Visual Studio Code",
    programs: &["Code"],
    bundles: &[
        "Visual Studio Code.app",
        "Visual Studio Code - Insiders.app",
    ],
    prefixes: &[],
};

const JETBRAINS: OwnerApp = OwnerApp {
    name: "your JetBrains IDE",
    programs: &[
        "idea",
        "pycharm",
        "webstorm",
        "goland",
        "clion",
        "rider",
        "rubymine",
        "phpstorm",
        "datagrip",
        "rustrover",
        "dataspell",
        "aqua",
        "fleet",
    ],
    bundles: &[],
    prefixes: &[],
};

const CLAUDE_CODE: OwnerApp = OwnerApp {
    name: "Claude Code",
    programs: &["claude"],
    bundles: &[],
    prefixes: &[],
};

const CARGO: OwnerApp = OwnerApp {
    name: "Cargo",
    programs: &["cargo", "rustc"],
    bundles: &[],
    prefixes: &[],
};

const RUSTUP: OwnerApp = OwnerApp {
    name: "rustup",
    programs: &["rustup", "cargo", "rustc"],
    bundles: &[],
    prefixes: &[],
};

const IPHONE_BACKUP: OwnerApp = OwnerApp {
    name: "the iPhone backup",
    programs: &["AppleMobileBackup", "AppleMobileDeviceHelper"],
    bundles: &[],
    prefixes: &[],
};

const FINAL_CUT: OwnerApp = OwnerApp {
    name: "Final Cut Pro",
    programs: &["Final Cut Pro"],
    bundles: &["Final Cut Pro.app", "Final Cut Pro Trial.app"],
    prefixes: &[],
};

const ADOBE_VIDEO: OwnerApp = OwnerApp {
    name: "your Adobe video apps",
    programs: &["dynamiclinkmanager"],
    bundles: &[],
    prefixes: &[
        "Adobe Premiere Pro",
        "Adobe After Effects",
        "Adobe Media Encoder",
        "Adobe Audition",
        "Adobe Character Animator",
        "Adobe Prelude",
        "Adobe Premiere Rush",
    ],
};

const AFTER_EFFECTS: OwnerApp = OwnerApp {
    name: "After Effects",
    programs: &["aerendernode", "aerender"],
    bundles: &[],
    prefixes: &["Adobe After Effects"],
};

const OLLAMA: OwnerApp = OwnerApp {
    name: "Ollama",
    programs: &["ollama", "Ollama"],
    bundles: &["Ollama.app"],
    prefixes: &[],
};

const LM_STUDIO: OwnerApp = OwnerApp {
    name: "LM Studio",
    programs: &["LM Studio", "lms"],
    bundles: &["LM Studio.app"],
    prefixes: &[],
};

/// The apps that may be using the items `rule_id` finds. Empty if none.
pub fn owner_apps(rule_id: &str) -> &'static [OwnerApp] {
    match rule_id {
        "xcode.derived_data"
        | "xcode.archives"
        | "xcode.ios_device_support"
        | "xcode.watchos_device_support" => &[XCODE],
        "xcode.simulator_device" => &[XCODE, SIMULATOR],
        "android.gradle_caches" | "android.gradle_wrapper" => &[ANDROID_STUDIO, GRADLE],
        "android.system_image" | "android.emulator" => &[ANDROID_STUDIO, ANDROID_EMULATOR],
        "android.ndk" | "android.studio_cache" => &[ANDROID_STUDIO],
        "browser.safari_cache" => &[SAFARI],
        "browser.chrome_cache" => &[CHROME],
        "browser.firefox_cache" => &[FIREFOX],
        "spotify.cache" => &[SPOTIFY],
        "slack.cache" => &[SLACK],
        "cursor.cache" | "cursor.cached_data" | "cursor.extension_downloads" => &[CURSOR],
        "vscode.cache" | "vscode.cached_data" | "vscode.extension_downloads" => &[VSCODE],
        "jetbrains.cache" => &[JETBRAINS],
        "claude_code.conversations" | "claude_code.logs" => &[CLAUDE_CODE],
        "rust.cargo_registry_cache" | "rust.cargo_registry_src" => &[CARGO],
        "rust.toolchain" => &[RUSTUP],
        "ios.device_backup" => &[IPHONE_BACKUP],
        "ollama.models" => &[OLLAMA],
        "lm_studio.model" | "lm_studio.model_legacy" => &[LM_STUDIO],
        "final_cut.render_files" | "final_cut.proxy_media" => &[FINAL_CUT],
        "adobe.media_cache_files" | "adobe.media_cache_database" => &[ADOBE_VIDEO],
        "adobe.after_effects_disk_cache" => &[AFTER_EFFECTS],
        id if id.starts_with("rust.target") => &[CARGO],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_rules_have_owners_and_others_do_not() {
        assert_eq!(owner_apps("xcode.derived_data")[0].name, "Xcode");
        assert_eq!(owner_apps("rust.target_developer")[0].name, "Cargo");
        assert!(owner_apps("logs.user").is_empty());
        assert!(owner_apps("no.such.rule").is_empty());
        assert_eq!(
            owner_apps("final_cut.render_files")[0].name,
            "Final Cut Pro"
        );
        assert!(!owner_apps("adobe.media_cache_files")[0].prefixes.is_empty());
    }

    #[test]
    fn every_owner_can_be_matched() {
        for id in [
            "xcode.simulator_device",
            "android.emulator",
            "jetbrains.cache",
            "ollama.models",
            "lm_studio.model",
            "final_cut.proxy_media",
            "adobe.media_cache_database",
            "adobe.after_effects_disk_cache",
        ] {
            for app in owner_apps(id) {
                assert!(
                    !app.programs.is_empty() || !app.bundles.is_empty() || !app.prefixes.is_empty(),
                    "{}",
                    app.name
                );
            }
        }
    }
}
