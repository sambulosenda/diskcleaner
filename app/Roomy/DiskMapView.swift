import SwiftUI

/// Lays rectangles out as a treemap: each one's area matches its share of the total, and the shapes are
/// kept as close to squares as possible (the "squarified" method). Pure geometry, so it can be tested.
enum Treemap {
    /// One rectangle per value, in the same order, filling `rect`. Values should be largest first for
    /// the best shapes. A value of zero or less gets an empty rectangle.
    static func layout(_ values: [Double], in rect: CGRect) -> [CGRect] {
        var result = [CGRect](repeating: .zero, count: values.count)
        let total = values.reduce(0) { $0 + max($1, 0) }
        guard total > 0, rect.width > 0, rect.height > 0 else { return result }
        let scale = Double(rect.width) * Double(rect.height) / total
        let areas = values.map { max($0, 0) * scale }
        var free = rect
        var start = 0
        while start < areas.count {
            let side = Double(min(free.width, free.height))
            guard side > 0 else { break }
            // Grow the row while its worst shape keeps getting better.
            var end = start + 1
            var sum = areas[start]
            var smallest = areas[start]
            var largest = areas[start]
            var best = worstRatio(sum: sum, smallest: smallest, largest: largest, side: side)
            while end < areas.count {
                let area = areas[end]
                let nextSum = sum + area
                let nextSmallest = min(smallest, area)
                let nextLargest = max(largest, area)
                let ratio = worstRatio(sum: nextSum, smallest: nextSmallest, largest: nextLargest, side: side)
                if ratio > best { break }
                sum = nextSum
                smallest = nextSmallest
                largest = nextLargest
                best = ratio
                end += 1
            }
            // Place the row along the shorter side of the space that is left.
            let thickness = sum / side
            let alongLeft = free.width >= free.height
            var offset = 0.0
            for index in start..<end {
                let length = sum > 0 ? areas[index] / sum * side : 0
                if alongLeft {
                    result[index] = CGRect(x: Double(free.minX), y: Double(free.minY) + offset,
                                           width: thickness, height: length)
                } else {
                    result[index] = CGRect(x: Double(free.minX) + offset, y: Double(free.minY),
                                           width: length, height: thickness)
                }
                offset += length
            }
            if alongLeft {
                free = CGRect(x: Double(free.minX) + thickness, y: Double(free.minY),
                              width: max(Double(free.width) - thickness, 0), height: Double(free.height))
            } else {
                free = CGRect(x: Double(free.minX), y: Double(free.minY) + thickness,
                              width: Double(free.width), height: max(Double(free.height) - thickness, 0))
            }
            start = end
        }
        return result
    }

    /// How far from square the least square rectangle of a row would be. 1 is a perfect square.
    private static func worstRatio(sum: Double, smallest: Double, largest: Double, side: Double) -> Double {
        guard sum > 0, smallest > 0 else { return .infinity }
        let sideSquared = side * side
        let sumSquared = sum * sum
        return max(sideSquared * largest / sumSquared, sumSquared / (sideSquared * smallest))
    }
}

/// One block of the map: an item the scan found, or the many small items drawn as one block.
struct DiskMapTile: Identifiable, Equatable {
    let id: String
    let title: String
    /// The name that tells it apart from items with the same title (Developer mode only).
    let place: String?
    let bytes: UInt64
    /// `nil` for the block of smaller items, which mixes tiers.
    let tier: Tier?
    let category: Category?
    let explain: String
    /// The item to open in the list, or `nil` for the block of smaller items.
    let itemID: String?

    /// The title with its distinguishing name, for VoiceOver and tooltips.
    var spokenTitle: String {
        guard let place else { return title }
        return "\(title), \(place)"
    }
}

/// What the map shows, worked out from the scan's items.
enum DiskMapContent {
    /// The most blocks the map draws. More than this and the small ones can't be read or clicked.
    static let tileLimit = 40
    /// The id of the block that stands for every smaller item.
    static let smallerID = "roomy.map.smaller"

    /// The blocks, largest first: the biggest items one block each, then one block for all the rest.
    /// Items of zero size are left out.
    static func tiles(for items: [ItemSummary], limit: Int = tileLimit) -> [DiskMapTile] {
        let sized = items.filter { $0.sizeBytes > 0 }.sorted { $0.sizeBytes > $1.sizeBytes }
        let shown = sized.prefix(max(limit, 0))
        let rest = sized.dropFirst(shown.count)
        var tiles = shown.map { item in
            DiskMapTile(id: item.id, title: item.title, place: item.place, bytes: item.sizeBytes,
                        tier: item.tier, category: item.category, explain: item.explain, itemID: item.id)
        }
        if !rest.isEmpty {
            let bytes = rest.reduce(UInt64(0)) { sum, item in
                let (next, overflow) = sum.addingReportingOverflow(item.sizeBytes)
                return overflow ? UInt64.max : next
            }
            let noun = rest.count == 1 ? "smaller item" : "smaller items"
            tiles.append(DiskMapTile(
                id: smallerID, title: "\(rest.count) \(noun)", place: nil, bytes: bytes, tier: nil,
                category: nil,
                explain: "Everything else the scan found, each too small to draw by itself. They're all in the list.",
                itemID: nil
            ))
            tiles.sort { $0.bytes > $1.bytes }
        }
        return tiles
    }

    /// The line under the map: what it covers, and what it leaves out.
    static func caption(mapped: UInt64, overview: Overview) -> String {
        let rest = overview.scannedBytes > mapped ? overview.scannedBytes - mapped : 0
        let lead = "The map shows the \(Format.bytes(mapped)) Roomy recognised. Green is likely safe to remove, "
            + "orange is worth checking first, and grey is kept."
        guard rest > 0 else { return lead }
        return lead + " The other \(Format.bytes(rest)) in your home folder is your own files and apps."
    }

    /// The colour of a block. The tier is always named in words when a block is chosen.
    static func color(for tile: DiskMapTile) -> Color {
        tile.tier?.tint ?? .gray
    }
}

/// The Overview's map: everything the scan recognised as blocks sized by the space they take and
/// coloured by how safe they are to remove. Choosing a block explains it. Nothing is ticked or
/// removed here.
struct DiskMap: View {
    @EnvironmentObject private var model: AppModel
    let overview: Overview
    @State private var tiles: [DiskMapTile] = []
    @State private var isLoaded = false
    @State private var selectedID: String?
    /// The map's height. Wide enough windows give blocks room for their names.
    static let height: CGFloat = 320

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if !isLoaded {
                ProgressView()
                    .controlSize(.small)
                    .frame(maxWidth: .infinity, minHeight: 80)
            } else if tiles.isEmpty {
                Text("There's nothing to map yet.")
                    .foregroundStyle(.secondary)
            } else {
                GeometryReader { proxy in
                    let rects = Treemap.layout(tiles.map { Double($0.bytes) },
                                               in: CGRect(origin: .zero, size: proxy.size))
                    ZStack {
                        ForEach(Array(tiles.enumerated()), id: \.element.id) { index, tile in
                            let rect = rects[index]
                            DiskMapBlock(tile: tile, size: rect.size, isSelected: selectedID == tile.id) {
                                selectedID = selectedID == tile.id ? nil : tile.id
                            }
                            .frame(width: max(rect.width - 2, 0), height: max(rect.height - 2, 0))
                            .position(x: rect.midX, y: rect.midY)
                        }
                    }
                }
                .frame(height: Self.height)
                .accessibilityElement(children: .contain)
                .accessibilityLabel("Map of what the scan found")
                if let tile = tiles.first(where: { $0.id == selectedID }) {
                    DiskMapExplanation(tile: tile)
                } else {
                    Text("Choose a block to see what it is.")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
                Text(DiskMapContent.caption(mapped: mappedBytes, overview: overview))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .task(id: scanKey) {
            let items = await model.scannedItems()
            tiles = DiskMapContent.tiles(for: items)
            if let selectedID, !tiles.contains(where: { $0.id == selectedID }) {
                self.selectedID = nil
            }
            isLoaded = true
        }
    }

    /// Changes when a new scan's results arrive, so the map is drawn again.
    private var scanKey: String {
        "\(overview.scannedBytes)-\(overview.likelySafeBytes)-\(overview.reviewFirstBytes)"
    }

    private var mappedBytes: UInt64 {
        tiles.reduce(UInt64(0)) { sum, tile in
            let (next, overflow) = sum.addingReportingOverflow(tile.bytes)
            return overflow ? UInt64.max : next
        }
    }
}

/// One block of the map. A button, so it works with the keyboard and VoiceOver.
struct DiskMapBlock: View {
    let tile: DiskMapTile
    /// The block's size before the gap around it, which decides whether its name fits.
    let size: CGSize
    let isSelected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            ZStack(alignment: .topLeading) {
                RoundedRectangle(cornerRadius: 6, style: .continuous)
                    .fill(DiskMapContent.color(for: tile).opacity(isSelected ? 0.5 : 0.28))
                RoundedRectangle(cornerRadius: 6, style: .continuous)
                    .strokeBorder(isSelected ? Color.accentColor : DiskMapContent.color(for: tile).opacity(0.5),
                                  lineWidth: isSelected ? 2 : 1)
                if Self.showsName(in: size) {
                    VStack(alignment: .leading, spacing: 1) {
                        Text(tile.title)
                            .font(.caption.weight(.semibold))
                            .lineLimit(1)
                        Text(Format.bytes(tile.bytes))
                            .font(.caption2.monospacedDigit())
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                    .padding(6)
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .help("\(tile.spokenTitle), \(Format.bytes(tile.bytes))")
        .accessibilityLabel(Self.spokenLabel(for: tile))
        .accessibilityHint(tile.explain)
        .accessibilityAddTraits(isSelected ? .isSelected : [])
    }

    /// Whether a block is big enough to carry its name and size.
    static func showsName(in size: CGSize) -> Bool {
        size.width >= 84 && size.height >= 40
    }

    /// Name, size and tier in words, so colour is never the only signal.
    static func spokenLabel(for tile: DiskMapTile) -> String {
        let base = "\(tile.spokenTitle), \(Format.bytes(tile.bytes))"
        guard let tier = tile.tier else { return base }
        return "\(base), \(tier.label)"
    }
}

/// What the chosen block is, in the item's own plain words, with a way to open it in the list.
struct DiskMapExplanation: View {
    @EnvironmentObject private var model: AppModel
    let tile: DiskMapTile

    var body: some View {
        HStack(alignment: .top, spacing: 12) {
            if let category = tile.category {
                CategoryIcon(category: category)
            }
            VStack(alignment: .leading, spacing: 4) {
                HStack(alignment: .firstTextBaseline, spacing: 8) {
                    Text(tile.spokenTitle)
                        .font(.body.weight(.medium))
                    if let tier = tile.tier {
                        TierLabel(tier: tier)
                    }
                    Spacer(minLength: 12)
                    Text(Format.bytes(tile.bytes))
                        .font(.body.monospacedDigit())
                }
                Text(tile.explain)
                    .font(.callout)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
                Button(tile.itemID == nil ? "Show everything in the list" : "Show in the list") {
                    // Keep items aren't in a tier list the Overview opens, so show everything for them.
                    let tier = tile.tier == .keep ? nil : tile.tier
                    model.showItems(tier: tier, select: tile.itemID)
                }
                .buttonStyle(.link)
                .padding(.top, 2)
            }
        }
        .padding(12)
        .background(RoundedRectangle(cornerRadius: 8, style: .continuous).fill(Color.secondary.opacity(0.08)))
        .accessibilityElement(children: .contain)
    }
}
