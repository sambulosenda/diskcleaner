import AppKit
import SwiftUI

// "Ask Roomy What This Is" in Finder's right-click menu (under Services or Quick Actions).
//
// Finder hands Roomy the chosen file or folder. Roomy asks the core what the last scan knows about
// that place and shows the answer. This only explains: nothing is ticked, planned or removed from
// here, and the core's answer holds no path. It is the one place the app passes a path to the core;
// cleanup still works by item ids only.

/// A question from Finder, and its answer once the core has one.
struct PlaceQuestion: Identifiable, Equatable {
    enum State: Equatable {
        /// Waiting for a scan to finish, or for the core's answer.
        case checking
        case answered(PlaceAnswer)
        /// The scan failed, so there is nothing to answer from.
        case unavailable
    }

    let id = UUID()
    /// The file or folder's name as Finder showed it. A single name, never a path.
    let name: String
    var state: State = .checking
}

/// The words of an answer. Pure, so tests can check them without a window.
enum PlaceText {
    static func title(name: String) -> String {
        "What is \u{201C}\(name)\u{201D}?"
    }

    /// The main sentence for an answer.
    static func lead(for answer: PlaceAnswer) -> String {
        switch answer.matched {
        case .item:
            return answer.item?.explain ?? notListed
        case .insideItem:
            guard let item = answer.item else { return notListed }
            return "This is part of \(item.title). \(item.explain)"
        case .holdsItems:
            return holds(answer)
        case .notListed:
            return notListed
        case .outsideHome:
            return "This is outside your home folder. Roomy only looks inside your home folder and never touches anything here."
        }
    }

    static let notListed =
        "Roomy has no note about this, so it leaves it alone. It's most likely one of your own files or part of an app."

    static let checking = "Checking what Roomy knows about this\u{2026}"
    static let unavailable = "Roomy couldn't finish its scan, so it can't say yet. Scan again and ask once more."
    static let footer = "Roomy only explains here. Nothing is removed."

    private static func holds(_ answer: PlaceAnswer) -> String {
        let things = answer.itemCount == 1 ? "1 thing" : "\(answer.itemCount) things"
        let lead = "This folder holds \(things) Roomy knows about."
        switch (answer.likelySafeBytes > 0, answer.reviewFirstBytes > 0) {
        case (true, true):
            return "\(lead) \(Format.bytes(answer.likelySafeBytes)) is likely safe to remove and "
                + "\(Format.bytes(answer.reviewFirstBytes)) is worth checking first."
        case (true, false):
            return "\(lead) \(Format.bytes(answer.likelySafeBytes)) is likely safe to remove."
        case (false, true):
            return "\(lead) \(Format.bytes(answer.reviewFirstBytes)) is worth checking first."
        case (false, false):
            return "\(lead) Roomy keeps all of it."
        }
    }
}

/// The answer, as a sheet over the main window.
struct PlaceAnswerSheet: View {
    @EnvironmentObject private var model: AppModel
    let question: PlaceQuestion

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(PlaceText.title(name: question.name))
                .font(.title3.weight(.semibold))
                .lineLimit(2)
                .truncationMode(.middle)
            content
            Text(PlaceText.footer)
                .font(.callout)
                .foregroundStyle(.secondary)
            HStack {
                if let item = shownItem {
                    Button("Show in the List") {
                        // A search left in the toolbar could hide the very item being shown.
                        model.searchText = ""
                        model.showItems(tier: item.tier == .keep ? nil : item.tier, select: item.id)
                        model.placeQuestion = nil
                    }
                }
                Spacer()
                Button("Done") { model.placeQuestion = nil }
                    .keyboardShortcut(.defaultAction)
            }
        }
        .padding(20)
        .frame(width: 440)
    }

    /// The item to offer in the list: only when the place is, or is inside, one.
    private var shownItem: ItemSummary? {
        guard case let .answered(answer) = question.state else { return nil }
        return answer.item
    }

    @ViewBuilder
    private var content: some View {
        switch question.state {
        case .checking:
            HStack(spacing: 8) {
                ProgressView().controlSize(.small)
                Text(PlaceText.checking).foregroundStyle(.secondary)
            }
        case .unavailable:
            Text(PlaceText.unavailable)
                .fixedSize(horizontal: false, vertical: true)
        case let .answered(answer):
            if let item = answer.item {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text(item.title).font(.body.weight(.medium))
                    TierLabel(tier: item.tier)
                    Spacer(minLength: 12)
                    Text(Format.bytes(item.sizeBytes)).font(.body.monospacedDigit())
                }
            }
            Text(PlaceText.lead(for: answer))
                .fixedSize(horizontal: false, vertical: true)
        }
    }
}

/// Receives Finder's "Ask Roomy What This Is". Named in Info.plist under NSServices.
@MainActor
final class FinderService: NSObject {
    /// Called with the chosen file or folder. Set once the main window exists.
    var onAsk: ((URL) -> Void)? {
        didSet {
            if let onAsk, let waiting {
                self.waiting = nil
                onAsk(waiting)
            }
        }
    }
    /// A question that arrived before the window was ready, as when Finder launches Roomy to ask.
    private var waiting: URL?

    /// The service's entry point. Reads the first file URL Finder put on the pasteboard.
    @objc func explainInRoomy(_ pboard: NSPasteboard, userData: String?, error: AutoreleasingUnsafeMutablePointer<NSString>) {
        let options: [NSPasteboard.ReadingOptionKey: Any] = [.urlReadingFileURLsOnly: true]
        guard let url = (pboard.readObjects(forClasses: [NSURL.self], options: options) as? [URL])?.first else {
            return
        }
        if let onAsk { onAsk(url) } else { waiting = url }
    }
}

/// Registers the Finder service when the app starts.
@MainActor
final class RoomyAppDelegate: NSObject, NSApplicationDelegate {
    let finder = FinderService()

    func applicationDidFinishLaunching(_ notification: Notification) {
        NSApp.servicesProvider = finder
    }
}
