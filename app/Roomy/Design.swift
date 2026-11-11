import AppKit
import SwiftUI

// Roomy's small set of shapes, gaps and motion. Every screen draws from here, so a card on the
// Overview, a note in a sheet and a block on the map look like parts of one app. Add a value here
// only when none of these fits; don't write a new number in a view.

/// Corner radii. Three sizes, all drawn with the continuous curve macOS itself uses.
enum Radius {
    /// Thin things: the disk bar, legend swatches.
    static let small: CGFloat = 4
    /// Things inside a card or a list: tiles, map blocks, inline notes, highlighted rows.
    static let medium: CGFloat = 8
    /// Cards and panels that sit on the window itself.
    static let large: CGFloat = 12

    static func shape(_ radius: CGFloat) -> RoundedRectangle {
        RoundedRectangle(cornerRadius: radius, style: .continuous)
    }
}

/// Gaps and padding, on a 4 point grid.
enum Space {
    /// Between a title and the line under it.
    static let xxs: CGFloat = 2
    /// Between closely related lines, and above and below a list row.
    static let xs: CGFloat = 4
    /// Between an icon and its text, and between neighbouring controls.
    static let s: CGFloat = 8
    /// Between the parts of a row or a small group.
    static let m: CGFloat = 12
    /// Inside bars and cards, and the window's side margin for lists.
    static let l: CGFloat = 16
    /// Inside sheets and large cards.
    static let xl: CGFloat = 20
    /// Between the sections of a screen.
    static let xxl: CGFloat = 24
    /// Around a whole screen's content.
    static let screen: CGFloat = 32
}

/// Motion. Three speeds, all short, and none at all when Reduce Motion is on in System Settings.
enum Motion {
    /// A pointer highlight or a tick: over before you notice it.
    static let quick = Animation.easeOut(duration: 0.12)
    /// Something opening or closing in place: a disclosure, a bar under a list, a sheet changing step.
    static let standard = Animation.easeInOut(duration: 0.2)
    /// A panel sliding in or out: settles without a bounce.
    static let settle = Animation.spring(response: 0.32, dampingFraction: 0.9)

    /// Whether the user asked macOS for less motion.
    static var isReduced: Bool { NSWorkspace.shared.accessibilityDisplayShouldReduceMotion }

    /// `withAnimation`, unless Reduce Motion is on, when the change just happens.
    static func animate(_ animation: Animation = standard, _ change: () -> Void) {
        if isReduced {
            change()
        } else {
            withAnimation(animation, change)
        }
    }
}

/// Animates changes of `value`, unless Reduce Motion is on.
private struct MotionModifier<Value: Equatable>: ViewModifier {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let animation: Animation
    let value: Value

    func body(content: Content) -> some View {
        content.animation(reduceMotion ? nil : animation, value: value)
    }
}

extension View {
    /// Animates changes of `value` with one of `Motion`'s speeds, unless Reduce Motion is on.
    func motion<Value: Equatable>(_ animation: Animation = Motion.standard, value: Value) -> some View {
        modifier(MotionModifier(animation: animation, value: value))
    }

    /// A quiet filled card that sits on the window: "Ready to clean", "Found so far".
    func cardBackground() -> some View {
        background(Radius.shape(Radius.large).fill(Color.primary.opacity(0.05)))
    }

    /// A group of rows with a hairline edge: "Worth a look", "Since your last scan".
    func rowGroupBackground() -> some View {
        background(Radius.shape(Radius.large).fill(Color(nsColor: .controlBackgroundColor)))
            .overlay(Radius.shape(Radius.large).strokeBorder(Color(nsColor: .separatorColor)))
            .clipShape(Radius.shape(Radius.large))
    }

    /// A tinted note inside a card, a list or a sheet.
    func noteBackground(_ tint: Color = .secondary, opacity: Double = 0.1) -> some View {
        background(Radius.shape(Radius.medium).fill(tint.opacity(opacity)))
    }

    /// Marks a title as a heading, so VoiceOver users can jump between the parts of a screen.
    func heading() -> some View {
        accessibilityAddTraits(.isHeader)
    }
}

/// A short word in a pill beside a title: "Open", "Check this", "Not used". The word carries the
/// meaning; the tint only backs it up.
struct Badge: View {
    let text: String
    var tint: Color = .secondary

    var body: some View {
        Text(text)
            .font(.caption.weight(.medium))
            .lineLimit(1)
            .padding(.horizontal, Space.s)
            .padding(.vertical, Space.xxs)
            .background(Capsule().fill(tint.opacity(0.15)))
    }
}

/// How wide the detail panel beside a list is: a third of the window, within sensible limits.
enum DetailPane {
    static let minWidth: CGFloat = 280
    static let maxWidth: CGFloat = 400

    static func width(in total: CGFloat) -> CGFloat {
        min(max(total / 3, minWidth), maxWidth)
    }
}

/// Says something through VoiceOver when the screen changes by itself: a scan finishing, a review
/// moving to its next step. Does nothing when VoiceOver is off.
enum Announce {
    static func say(_ text: String) {
        guard !text.isEmpty, let window = NSApp.keyWindow ?? NSApp.mainWindow else { return }
        NSAccessibility.post(element: window, notification: .announcementRequested, userInfo: [
            .announcement: text,
            .priority: NSAccessibilityPriorityLevel.high.rawValue,
        ])
    }
}

// MARK: - Review sheets

/// The one frame every review sheet uses: a title, the content, a divider and a row of buttons.
/// The main cleanup review, the Docker review and the Tool clean-ups review all sit in it, so
/// reviewing looks and works the same wherever it happens.
struct ReviewSheet<Content: View, Footer: View>: View {
    let title: String
    let content: () -> Content
    let footer: () -> Footer

    init(title: String, @ViewBuilder content: @escaping () -> Content, @ViewBuilder footer: @escaping () -> Footer) {
        self.title = title
        self.content = content
        self.footer = footer
    }

    static var size: CGSize { CGSize(width: 560, height: 480) }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(title)
                .font(.title2.weight(.semibold))
                .heading()
                .padding([.horizontal, .top], Space.xl)
                .padding(.bottom, Space.m)
            content()
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            Divider()
            HStack(spacing: Space.s) {
                footer()
            }
            .padding(Space.l)
        }
        .frame(width: Self.size.width, height: Self.size.height)
    }
}

/// A spinner and a line of text, centred in a review sheet.
struct ReviewWorking: View {
    let text: String

    var body: some View {
        VStack(spacing: Space.m) {
            ProgressView()
            Text(text)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
        }
        .padding(Space.xl)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityElement(children: .combine)
    }
}

/// The lines above a review's list: what would happen, an optional quiet note, and an optional
/// warning for something that can't be undone.
struct ReviewSummary: View {
    let headline: String
    var note: String?
    var warning: String?

    var body: some View {
        VStack(alignment: .leading, spacing: Space.xs) {
            Text(headline)
                .font(.headline)
                .heading()
            if let note {
                Text(note)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if let warning {
                Label {
                    Text(warning)
                        .fixedSize(horizontal: false, vertical: true)
                } icon: {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(.orange)
                        .accessibilityHidden(true)
                }
                .font(.callout)
            }
        }
        .padding(.horizontal, Space.xl)
    }
}

/// The mark in front of a review row's title.
enum ReviewMarker {
    /// Would be moved or removed, or was.
    case included
    /// Stays where it is.
    case excluded
    /// No mark: the list's own selection says which row is chosen.
    case none
}

/// One row of a review list: a mark for whether it is included, a title, lines of detail, a size.
struct ReviewRow<Detail: View>: View {
    let marker: ReviewMarker
    let title: String
    /// Identifiers such as Docker's image ids read better in fixed-width type.
    let monospacedTitle: Bool
    /// `nil` leaves the size out.
    let sizeBytes: UInt64?
    let detail: () -> Detail

    init(marker: ReviewMarker = .included, title: String, monospacedTitle: Bool = false, sizeBytes: UInt64?,
         @ViewBuilder detail: @escaping () -> Detail) {
        self.marker = marker
        self.title = title
        self.monospacedTitle = monospacedTitle
        self.sizeBytes = sizeBytes
        self.detail = detail
    }

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: Space.s) {
            switch marker {
            case .included:
                Image(systemName: "checkmark.circle.fill")
                    .foregroundStyle(Color.green)
                    .accessibilityHidden(true)
            case .excluded:
                Image(systemName: "minus.circle")
                    .foregroundStyle(Color.secondary)
                    .accessibilityHidden(true)
            case .none:
                EmptyView()
            }
            VStack(alignment: .leading, spacing: Space.xxs) {
                Text(title)
                    .font(monospacedTitle ? .system(.body, design: .monospaced) : .body.weight(.medium))
                    .lineLimit(monospacedTitle ? 1 : nil)
                    .truncationMode(.middle)
                detail()
            }
            Spacer(minLength: Space.m)
            if let sizeBytes {
                Text(Format.bytes(sizeBytes))
                    .font(.body.monospacedDigit())
            }
        }
        .padding(.vertical, Space.xs)
        .accessibilityElement(children: .combine)
    }
}

/// A line of secondary detail in a review row.
struct ReviewDetailLine: View {
    let text: String

    init(_ text: String) {
        self.text = text
    }

    var body: some View {
        Text(text)
            .font(.callout)
            .foregroundStyle(.secondary)
            .fixedSize(horizontal: false, vertical: true)
    }
}

/// "Roomy will run: …" under a review list, for clean-ups a tool does itself.
struct ReviewCommandLine: View {
    let text: String

    var body: some View {
        Text(text)
            .font(.system(.callout, design: .monospaced))
            .foregroundStyle(.secondary)
            .lineLimit(2)
            .truncationMode(.middle)
            .textSelection(.enabled)
            .padding(.horizontal, Space.xl)
            .padding(.bottom, Space.m)
    }
}

/// The top of a sheet after something finished well: a green tick, the result in large type, and a
/// line underneath.
struct FinishPanel: View {
    let headline: String
    var detail: String?

    var body: some View {
        HStack(alignment: .center, spacing: Space.m) {
            Image(systemName: "checkmark.circle.fill")
                .font(.system(size: 34))
                .foregroundStyle(.green)
                .accessibilityHidden(true)
            VStack(alignment: .leading, spacing: Space.xxs) {
                Text(headline)
                    .font(.title.weight(.bold))
                    .heading()
                if let detail {
                    Text(detail)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(Space.l)
        .frame(maxWidth: .infinity, alignment: .leading)
        .noteBackground(.green)
        .accessibilityElement(children: .combine)
    }
}
