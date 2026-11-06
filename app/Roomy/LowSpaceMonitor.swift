import AppKit
import Foundation
import UserNotifications

/// Shows the "space is low" notification. A protocol so tests can count alerts without macOS asking
/// for permission.
@MainActor
protocol LowSpaceNotifier: AnyObject {
    /// Asks macOS whether Roomy may show notifications, prompting the first time. Returns the answer.
    func requestPermission() async -> Bool
    /// Shows the notification that free space has dropped below the threshold.
    func notifyLowSpace(freeBytes: UInt64)
}

/// Watches free space on the startup disk for the menu bar item and the low-space notification.
///
/// Read only. It asks the core for the volume's free space (`disk_summary`, a `statfs` call) and lets
/// the core decide when to warn (`check_low_space`). It never starts a scan or a cleanup on its own;
/// the notification and the menu only open Roomy so the user can choose.
@MainActor
final class LowSpaceMonitor: ObservableObject {
    /// The latest free-space reading.
    enum Reading: Equatable {
        /// Not read yet.
        case unknown
        /// Free and total bytes on the startup disk.
        case read(free: UInt64, total: UInt64)
        /// The core could not read the disk. Nothing is shown as low and no alert fires.
        case failed
    }

    static let showInMenuBarKey = "roomy.lowSpace.showInMenuBar"
    static let alertsEnabledKey = "roomy.lowSpace.alertsEnabled"
    static let thresholdKey = "roomy.lowSpace.thresholdBytes"

    /// How often the live monitor reads free space, in seconds.
    static let liveInterval: TimeInterval = 60

    @Published private(set) var reading: Reading = .unknown
    /// Free space is below the threshold, by the core's last check.
    @Published private(set) var isLow = false
    /// Set when the user said no to notifications, so Settings can say how to turn them on.
    @Published private(set) var notificationsDenied = false

    /// Whether the free-space item shows in the menu bar.
    @Published var showInMenuBar: Bool {
        didSet { defaults.set(showInMenuBar, forKey: Self.showInMenuBarKey) }
    }
    /// Whether a notification is shown when space gets low.
    @Published var alertsEnabled: Bool {
        didSet {
            defaults.set(alertsEnabled, forKey: Self.alertsEnabledKey)
            if alertsEnabled && !oldValue { evaluate() }
        }
    }
    /// Warn when free space drops below this many bytes. Always one the core accepts.
    @Published var thresholdBytes: UInt64 {
        didSet {
            if thresholdBytes != oldValue {
                defaults.set(NSNumber(value: thresholdBytes), forKey: Self.thresholdKey)
                evaluate()
            }
        }
    }

    /// The thresholds Settings offers, from the core.
    let thresholdChoices: [UInt64]

    /// Opens Roomy's main window. Set by a view that has SwiftUI's `openWindow` action.
    var openMainWindow: (() -> Void)?

    private let readDisk: () async throws -> DiskSummary
    private let notifier: LowSpaceNotifier
    private let defaults: UserDefaults
    /// Whether the user has been warned since space last recovered. Kept by the core's check.
    private var warned = false
    private var loop: Task<Void, Never>?
    private var wakeObserver: NSObjectProtocol?

    init(
        readDisk: @escaping () async throws -> DiskSummary,
        notifier: LowSpaceNotifier,
        defaults: UserDefaults = .standard
    ) {
        self.readDisk = readDisk
        self.notifier = notifier
        self.defaults = defaults
        thresholdChoices = lowSpaceThresholdChoices()
        showInMenuBar = defaults.object(forKey: Self.showInMenuBarKey) as? Bool ?? true
        alertsEnabled = defaults.object(forKey: Self.alertsEnabledKey) as? Bool ?? true
        let stored = (defaults.object(forKey: Self.thresholdKey) as? NSNumber)?.uint64Value
        // A stored value Settings doesn't offer (from an older version, say) falls back to the default.
        if let stored, thresholdChoices.contains(stored) {
            thresholdBytes = stored
        } else {
            thresholdBytes = lowSpaceDefaultThreshold()
        }
    }

    /// The monitor the app uses: reads the startup disk through the core and posts real notifications.
    static func live() -> LowSpaceMonitor {
        // Under XCTest the app is only the test host: read nothing and post nothing.
        if ProcessInfo.processInfo.environment["XCTestConfigurationFilePath"] != nil {
            return LowSpaceMonitor(readDisk: readHomeVolume, notifier: SilentNotifier())
        }
        let notifier = UserNotificationsNotifier()
        let monitor = LowSpaceMonitor(readDisk: Self.readHomeVolume, notifier: notifier)
        notifier.onOpen = { [weak monitor] in monitor?.showRoomy() }
        monitor.start(interval: liveInterval)
        return monitor
    }

    /// Reads free space on the volume holding the home folder, off the main thread.
    nonisolated static func readHomeVolume() async throws -> DiskSummary {
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        return try await Task.detached(priority: .utility) {
            try diskSummary(path: home)
        }.value
    }

    /// Reads now, then every `interval` seconds and whenever the Mac wakes from sleep. The app's monitor
    /// lives as long as the app, so this is never stopped; tests call `refresh()` instead.
    func start(interval: TimeInterval) {
        loop?.cancel()
        loop = Task { [weak self] in
            while !Task.isCancelled {
                guard let self else { return }
                await self.refresh()
                try? await Task.sleep(nanoseconds: UInt64(interval * 1_000_000_000))
            }
        }
        if wakeObserver == nil {
            wakeObserver = NSWorkspace.shared.notificationCenter.addObserver(
                forName: NSWorkspace.didWakeNotification, object: nil, queue: .main
            ) { [weak self] _ in
                Task { await self?.refresh() }
            }
        }
    }

    /// Reads free space once and warns if the core says to.
    func refresh() async {
        do {
            let summary = try await readDisk()
            reading = .read(free: summary.freeBytes, total: summary.totalBytes)
        } catch {
            reading = .failed
        }
        evaluate()
    }

    /// Asks the core whether space is low and whether to notify, for the current reading and settings.
    private func evaluate() {
        guard case let .read(free, _) = reading else {
            isLow = false
            return
        }
        let check = checkLowSpace(freeBytes: free, thresholdBytes: thresholdBytes, warned: warned)
        isLow = check.isLow
        // With alerts off, `warned` stays as it was, so turning them on while space is low still warns.
        guard alertsEnabled else { return }
        warned = check.warned
        if check.shouldAlert {
            Task { await alert(freeBytes: free) }
        }
    }

    /// Shows the notification, asking for permission the first time it is needed.
    private func alert(freeBytes: UInt64) async {
        let allowed = await notifier.requestPermission()
        notificationsDenied = !allowed
        if allowed {
            notifier.notifyLowSpace(freeBytes: freeBytes)
        }
    }

    /// Brings Roomy to the front with its main window open.
    func showRoomy() {
        NSApp.activate(ignoringOtherApps: true)
        if let openMainWindow {
            openMainWindow()
        } else {
            NSApp.windows.first { $0.identifier?.rawValue == "main" }?.makeKeyAndOrderFront(nil)
        }
    }

    // MARK: Text

    /// The menu bar label, such as "42 GB". Empty until the first reading.
    var menuBarTitle: String {
        guard case let .read(free, _) = reading else { return "" }
        return Format.bytes(free)
    }

    /// The first line of the menu, such as "42 GB free of 500 GB".
    var summaryText: String {
        switch reading {
        case .unknown: return "Checking free space…"
        case .failed: return "Roomy can't read your disk's free space."
        case let .read(free, total): return "\(Format.bytes(free)) free of \(Format.bytes(total))"
        }
    }

    /// The second line of the menu when space is low, or `nil`.
    var lowSpaceText: String? {
        isLow ? "Less than \(Format.bytes(thresholdBytes)) left" : nil
    }
}

/// Never shows anything. Used while the app is the host for the test target.
@MainActor
final class SilentNotifier: LowSpaceNotifier {
    func requestPermission() async -> Bool { false }
    func notifyLowSpace(freeBytes: UInt64) {}
}

/// Posts real notifications through the User Notifications framework, and opens Roomy when one is
/// clicked.
@MainActor
final class UserNotificationsNotifier: NSObject, LowSpaceNotifier, UNUserNotificationCenterDelegate {
    /// Called when the user clicks the notification.
    var onOpen: (() -> Void)?

    private static let identifier = "roomy.lowSpace"
    private let center = UNUserNotificationCenter.current()

    override init() {
        super.init()
        center.delegate = self
    }

    func requestPermission() async -> Bool {
        (try? await center.requestAuthorization(options: [.alert, .sound])) ?? false
    }

    func notifyLowSpace(freeBytes: UInt64) {
        let content = UNMutableNotificationContent()
        content.title = "Your Mac is nearly full"
        content.body = "\(Format.bytes(freeBytes)) left. Open Roomy to see what's using space."
        content.sound = .default
        // The same identifier replaces an earlier alert instead of stacking a second one.
        let request = UNNotificationRequest(identifier: Self.identifier, content: content, trigger: nil)
        center.add(request)
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse,
        withCompletionHandler completionHandler: @escaping () -> Void
    ) {
        Task { @MainActor in
            self.onOpen?()
            completionHandler()
        }
    }

    /// Shows the alert even when Roomy is the frontmost app.
    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler: @escaping (UNNotificationPresentationOptions) -> Void
    ) {
        completionHandler([.banner, .sound])
    }
}
