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
};

const SIMULATOR: OwnerApp = OwnerApp {
    name: "Simulator",
    programs: &["Simulator"],
    bundles: &["Simulator.app"],
};

const ANDROID_STUDIO: OwnerApp = OwnerApp {
    name: "Android Studio",
    programs: &["studio"],
    bundles: &["Android Studio.app", "Android Studio Preview.app"],
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
};

const GRADLE: OwnerApp = OwnerApp {
    name: "Gradle",
    programs: &["gradle", "gradlew"],
    bundles: &[],
};

const SAFARI: OwnerApp = OwnerApp {
    name: "Safari",
    programs: &["Safari"],
    bundles: &["Safari.app", "Safari Technology Preview.app"],
};

const CHROME: OwnerApp = OwnerApp {
    name: "Google Chrome",
    programs: &["Google Chrome"],
    bundles: &["Google Chrome.app", "Google Chrome Canary.app"],
};

const FIREFOX: OwnerApp = OwnerApp {
    name: "Firefox",
    programs: &["firefox"],
    bundles: &[
        "Firefox.app",
        "Firefox Developer Edition.app",
        "Firefox Nightly.app",
    ],
};

const SPOTIFY: OwnerApp = OwnerApp {
    name: "Spotify",
    programs: &["Spotify"],
    bundles: &["Spotify.app"],
};

const SLACK: OwnerApp = OwnerApp {
    name: "Slack",
    programs: &["Slack"],
    bundles: &["Slack.app"],
};

const CURSOR: OwnerApp = OwnerApp {
    name: "Cursor",
    programs: &["Cursor"],
    bundles: &["Cursor.app"],
};

const VSCODE: OwnerApp = OwnerApp {
    name: "Visual Studio Code",
    programs: &["Code"],
    bundles: &[
        "Visual Studio Code.app",
        "Visual Studio Code - Insiders.app",
    ],
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
};

const CLAUDE_CODE: OwnerApp = OwnerApp {
    name: "Claude Code",
    programs: &["claude"],
    bundles: &[],
};

const CARGO: OwnerApp = OwnerApp {
    name: "Cargo",
    programs: &["cargo", "rustc"],
    bundles: &[],
};

const RUSTUP: OwnerApp = OwnerApp {
    name: "rustup",
    programs: &["rustup", "cargo", "rustc"],
    bundles: &[],
};

const IPHONE_BACKUP: OwnerApp = OwnerApp {
    name: "the iPhone backup",
    programs: &["AppleMobileBackup", "AppleMobileDeviceHelper"],
    bundles: &[],
};

const OLLAMA: OwnerApp = OwnerApp {
    name: "Ollama",
    programs: &["ollama", "Ollama"],
    bundles: &["Ollama.app"],
};

const LM_STUDIO: OwnerApp = OwnerApp {
    name: "LM Studio",
    programs: &["LM Studio", "lms"],
    bundles: &["LM Studio.app"],
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
    }

    #[test]
    fn every_owner_can_be_matched() {
        for id in [
            "xcode.simulator_device",
            "android.emulator",
            "jetbrains.cache",
            "ollama.models",
            "lm_studio.model",
        ] {
            for app in owner_apps(id) {
                assert!(
                    !app.programs.is_empty() || !app.bundles.is_empty(),
                    "{}",
                    app.name
                );
            }
        }
    }
}
