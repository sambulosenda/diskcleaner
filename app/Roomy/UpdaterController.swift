import Combine
import Foundation
import Sparkle

/// Owns Sparkle, which checks for and installs new versions of Roomy (task T13).
///
/// The updater only starts when the app was built with a feed URL and a Sparkle public key in its
/// Info.plist. Development builds have an empty key, so "Check for Updates…" stays disabled and
/// Sparkle never shows an error at launch or during tests. `scripts/release.sh` builds with the key.
final class UpdaterController: ObservableObject {
    /// Whether "Check for Updates…" can be used right now. False while a check is running.
    @Published private(set) var canCheckForUpdates = false

    /// True when this build is set up for updates and Sparkle has started.
    let isEnabled: Bool

    private let controller: SPUStandardUpdaterController
    /// Sparkle holds its delegate weakly, so we keep it alive here.
    private let feedDelegate: FeedDelegate

    init(bundle: Bundle = .main) {
        let delegate = FeedDelegate()
        feedDelegate = delegate
        controller = SPUStandardUpdaterController(
            startingUpdater: false,
            updaterDelegate: delegate,
            userDriverDelegate: nil
        )
        isEnabled = Self.isConfigured(info: bundle.infoDictionary ?? [:])
        guard isEnabled else { return }
        controller.updater.publisher(for: \.canCheckForUpdates)
            .assign(to: &$canCheckForUpdates)
        controller.startUpdater()
    }

    /// Asks Sparkle to check now and show the result to the user.
    func checkForUpdates() {
        guard isEnabled else { return }
        controller.checkForUpdates(nil)
    }

    /// True when the Info.plist has a non-empty feed URL and Sparkle public key.
    static func isConfigured(info: [String: Any]) -> Bool {
        let feed = (info["SUFeedURL"] as? String)?.trimmingCharacters(in: .whitespaces) ?? ""
        let key = (info["SUPublicEDKey"] as? String)?.trimmingCharacters(in: .whitespaces) ?? ""
        return !feed.isEmpty && !key.isEmpty && !key.hasPrefix("$(")
    }
}

/// Lets a tester point Sparkle at an appcast on this Mac, for the "test update from a local
/// appcast" check in docs/release.md:
///
///     open /Applications/Roomy.app --args -RoomyTestFeedURL http://127.0.0.1:8000/appcast.xml
///
/// Only loopback addresses are accepted, so the override can never send the app to another server.
/// Every update must still carry a valid EdDSA signature from Sam's key and the same Developer ID,
/// whatever feed it came from.
final class FeedDelegate: NSObject, SPUUpdaterDelegate {
    /// The user-defaults key (also settable as a launch argument) for the local test feed.
    static let testFeedKey = "RoomyTestFeedURL"

    func feedURLString(for updater: SPUUpdater) -> String? {
        Self.loopbackFeed(UserDefaults.standard.string(forKey: Self.testFeedKey))
    }

    /// Returns `value` if it is an http(s) URL on this Mac (127.0.0.1, ::1 or localhost), else nil.
    /// Nil makes Sparkle use the feed URL from the Info.plist.
    static func loopbackFeed(_ value: String?) -> String? {
        guard let value,
              let components = URLComponents(string: value),
              let scheme = components.scheme?.lowercased(),
              scheme == "http" || scheme == "https",
              components.user == nil, components.password == nil,
              let host = components.host?.lowercased(),
              ["127.0.0.1", "::1", "[::1]", "localhost"].contains(host)
        else { return nil }
        return value
    }
}
