import SwiftUI

/// The weekly note: once a week, if the last scan found space worth freeing, the menu bar panel says
/// so in one quiet line. It is not a notification and makes no sound. Roomy never cleans by itself;
/// the note only offers to open the usual review.
enum WeeklyNote {
    /// Whether the note is on, in the app's settings. On unless the user turns it off.
    static let enabledKey = "roomy.weeklyNote.enabled"
    /// When the user last answered a note, or when Roomy first counted from.
    static let answeredKey = "roomy.weeklyNote.answeredAt"

    /// How long Roomy stays quiet after a note is answered.
    static let interval: TimeInterval = 7 * 24 * 60 * 60
    /// Less than this is not worth a note.
    static let worthMentioning: UInt64 = 1_000_000_000

    /// Whether a note should show now. Only for Likely safe space: Review first items need the user's
    /// own judgement, so they are never used to prompt.
    static func isDue(summary: LastScanSummary?, enabled: Bool, answeredAt: Date?, now: Date) -> Bool {
        guard enabled, let summary, summary.likelySafeBytes >= worthMentioning, let answeredAt else {
            return false
        }
        return now.timeIntervalSince(answeredAt) >= interval
    }

    /// The note's words, such as "5 GB is ready to clean, as of Monday."
    static func text(for summary: LastScanSummary, now: Date = Date()) -> String {
        let when = now.timeIntervalSince(summary.scannedAt) < 60
            ? "just now" : Format.relative(summary.scannedAt, now: now)
        return "\(Format.bytes(summary.likelySafeBytes)) is ready to clean, as of \(when). Have a look when it suits you."
    }
}

/// Decides when the weekly note shows, and remembers the user's answer.
@MainActor
final class WeeklyNoteModel: ObservableObject {
    /// How often the running app checks whether a note is due, in seconds.
    static let checkInterval: TimeInterval = 60 * 60

    /// Whether the weekly note is on.
    @Published var isEnabled: Bool {
        didSet {
            defaults.set(isEnabled, forKey: WeeklyNote.enabledKey)
            refresh()
        }
    }
    /// The note to show now, or `nil`.
    @Published private(set) var note: String?

    private let defaults: UserDefaults
    private let now: () -> Date
    private var loop: Task<Void, Never>?

    init(defaults: UserDefaults = .standard, now: @escaping () -> Date = { Date() }) {
        self.defaults = defaults
        self.now = now
        isEnabled = defaults.object(forKey: WeeklyNote.enabledKey) as? Bool ?? true
        refresh()
    }

    /// The model the app uses: checks now and then once an hour while Roomy runs.
    static func live() -> WeeklyNoteModel {
        let model = WeeklyNoteModel()
        model.start(interval: checkInterval)
        return model
    }

    /// Checks every `interval` seconds. The app's model lives as long as the app.
    func start(interval: TimeInterval) {
        loop?.cancel()
        loop = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: UInt64(interval * 1_000_000_000))
                guard let self else { return }
                self.refresh()
            }
        }
    }

    /// Works out again whether a note is due. Reads the app's settings only.
    func refresh() {
        let current = now()
        // The first week starts when Roomy is first used, so a new user isn't prompted straight away.
        if defaults.object(forKey: WeeklyNote.answeredKey) == nil {
            defaults.set(current.timeIntervalSince1970, forKey: WeeklyNote.answeredKey)
        }
        let answeredAt = Date(timeIntervalSince1970: defaults.double(forKey: WeeklyNote.answeredKey))
        let summary = LastScanSummary.load(from: defaults)
        let due = WeeklyNote.isDue(summary: summary, enabled: isEnabled, answeredAt: answeredAt, now: current)
        let next = due ? summary.map { WeeklyNote.text(for: $0, now: current) } : nil
        if next != note { note = next }
    }

    /// The user opened the review or said "Not this week": stay quiet for a week.
    func answer() {
        defaults.set(now().timeIntervalSince1970, forKey: WeeklyNote.answeredKey)
        refresh()
    }
}

/// The note as a row of the menu bar panel.
struct WeeklyNoteRow: View {
    let text: String
    let review: () -> Void
    let notNow: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: Space.s) {
            Text(text)
                .font(.callout)
                .fixedSize(horizontal: false, vertical: true)
            HStack(spacing: Space.s) {
                Button("Review in Roomy\u{2026}", action: review)
                Button("Not This Week", action: notNow)
                    .buttonStyle(.borderless)
            }
            .controlSize(.small)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, Space.l)
        .padding(.vertical, Space.s)
        .background(Tier.likelySafe.tint.opacity(0.12))
        .accessibilityElement(children: .contain)
    }
}

/// The weekly note's part of Settings.
struct WeeklyNoteSettingsSection: View {
    @ObservedObject var weeklyNote: WeeklyNoteModel

    var body: some View {
        VStack(alignment: .leading, spacing: Space.s) {
            Text("Weekly note")
                .font(.headline)
                .heading()
            Toggle("Tell me once a week when there's space to free", isOn: $weeklyNote.isEnabled)
            Text("A quiet line in the menu bar panel, not a notification. Roomy never removes anything by itself.")
                .font(.callout)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}
