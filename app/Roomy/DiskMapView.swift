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
    /// `nil` only for a block of smaller items that mixes tiers.
    let tier: Tier?
    let category: Category?
    let explain: String
    /// The item to open in the list, or `nil` for the block of smaller items.
    let itemID: String?
    /// How many items the block stands for: 1, or more for the block of smaller items.
    var count: Int = 1

    /// The title with its distinguishing name, for VoiceOver and tooltips.
    var spokenTitle: String {
        guard let place else { return title }
        return "\(title), \(place)"
    }
}

/// One side of the map: every block of one tier.
struct DiskMapGroup: Equatable, Identifiable {
    let tier: Tier
    /// Sum of the blocks' sizes.
    let bytes: UInt64
    let tiles: [DiskMapTile]

    var id: Tier { tier }
}

/// What the map shows, worked out from the scan's items.
enum DiskMapContent {
    /// The most blocks one side of the map draws. More than this and the small ones can't be read.
    static let tileLimit = 18
    /// The id of a block that stands for every smaller item, when those mix tiers.
    static let smallerID = "roomy.map.smaller"
    /// The smallest share of its side an item can have and still get its own block. Smaller than this
    /// and the block has no room for a name.
    static let smallestShare = 1.0 / 120
    /// The least width a side gets, as a share of the map, so the smaller side can still be read.
    static let narrowestSide = 0.24

    /// The id of the shared block for one tier's smaller items.
    static func smallerID(for tier: Tier?) -> String {
        guard let tier else { return smallerID }
        return "\(smallerID).\(tier.label)"
    }

    /// The map's sides: Likely safe first, then Review first, each only if it has anything. Kept
    /// items, such as everything else in Documents, are not drawn: nobody can act on them.
    static func groups(for items: [ItemSummary], limit: Int = tileLimit) -> [DiskMapGroup] {
        [Tier.likelySafe, .reviewFirst].compactMap { tier in
            let tiles = tiles(for: items.filter { $0.tier == tier }, limit: limit)
            guard !tiles.isEmpty else { return nil }
            return DiskMapGroup(tier: tier, bytes: sum(tiles.map(\.bytes)), tiles: tiles)
        }
    }

    /// How wide each side is, for sides of `bytes` sharing `width` with `gap` between them. Widths
    /// follow the sizes, but no side gets less than `narrowestSide` of the map.
    static func widths(for bytes: [UInt64], in width: CGFloat, gap: CGFloat) -> [CGFloat] {
        guard bytes.count > 1 else { return bytes.map { _ in max(width, 0) } }
        let room = max(width - gap * CGFloat(bytes.count - 1), 0)
        let total = bytes.reduce(0.0) { $0 + Double($1) }
        guard total > 0 else { return bytes.map { _ in room / CGFloat(bytes.count) } }
        // Lift the narrow sides to the least share, then take that from the others in proportion.
        var shares = bytes.map { Double($0) / total }
        let lifted = shares.map { max($0, narrowestSide) }
        let extra = lifted.reduce(0, +) - 1
        let givers = zip(shares, lifted).filter { $0.0 > narrowestSide }.reduce(0.0) { $0 + $1.1 - narrowestSide }
        shares = lifted.map { share in
            guard share > narrowestSide, givers > 0 else { return share }
            return share - extra * (share - narrowestSide) / givers
        }
        return shares.map { room * CGFloat($0) }
    }

    /// The blocks for `items`, largest first: the biggest items one block each, then one block for
    /// all the rest. Kept items and items of zero size are left out.
    static func tiles(for items: [ItemSummary], limit: Int = tileLimit) -> [DiskMapTile] {
        let sized = items.filter { $0.sizeBytes > 0 && $0.tier != .keep }
            .sorted { $0.sizeBytes > $1.sizeBytes }
        let total = sized.reduce(0.0) { $0 + Double($1.sizeBytes) }
        // An item too small a share of the map would get a block with no room for its name, so it
        // joins the rest.
        let shown = sized.prefix(max(limit, 0)).prefix { Double($0.sizeBytes) >= total * smallestShare }
        let rest = sized.dropFirst(shown.count)
        var tiles = shown.map { item in
            DiskMapTile(id: item.id, title: item.title, place: item.place, bytes: item.sizeBytes,
                        tier: item.tier, category: item.category, explain: item.explain, itemID: item.id)
        }
        if !rest.isEmpty {
            let tiers = Set(rest.map(\.tier))
            // Last, whatever its size: a catch-all shouldn't be the first block the eye lands on.
            tiles.append(smaller(count: rest.count, bytes: sum(rest.map(\.sizeBytes)),
                                 tier: tiers.count == 1 ? tiers.first : nil))
        }
        return tiles
    }

    /// The shared block that stands for `count` smaller items.
    static func smaller(count: Int, bytes: UInt64, tier: Tier?) -> DiskMapTile {
        let noun = count == 1 ? "smaller item" : "smaller items"
        return DiskMapTile(id: smallerID(for: tier), title: "\(count) \(noun)", place: nil, bytes: bytes,
                           tier: tier, category: nil, explain: smallerExplanation(for: tier), itemID: nil,
                           count: count)
    }

    /// `tiles` as they can really be drawn in `size`: an item whose block would come out too small to
    /// carry its name joins the shared block, so no item is drawn as a blank block. The shared block
    /// stays last.
    static func fitted(_ tiles: [DiskMapTile], in size: CGSize) -> [DiskMapTile] {
        var current = tiles
        // Each pass moves at least one item into the shared block, so this ends.
        for _ in 0..<tiles.count {
            let rects = Treemap.layout(current.map { Double($0.bytes) }, in: CGRect(origin: .zero, size: size))
            let blank = current.indices.filter { current[$0].itemID != nil && !DiskMapBlock.showsName(in: rects[$0].size) }
            guard !blank.isEmpty else { break }
            let moved = blank.map { current[$0] }
            let shared = current.first { $0.itemID == nil }
            var kept = current.enumerated().filter { !blank.contains($0.offset) && $0.element.itemID != nil }.map(\.element)
            let tiers = Set(moved.map(\.tier) + (shared.map { [$0.tier] } ?? []))
            kept.append(smaller(count: moved.count + (shared?.count ?? 0),
                                bytes: sum(moved.map(\.bytes) + [shared?.bytes ?? 0]),
                                tier: tiers.count == 1 ? (tiers.first ?? nil) : nil))
            current = kept
        }
        return current
    }

    /// What the shared block says when chosen.
    static func smallerExplanation(for tier: Tier?) -> String {
        let lead: String
        switch tier {
        case .likelySafe?: lead = "Everything else that is likely safe to remove"
        case .reviewFirst?: lead = "Everything else worth checking first"
        default: lead = "Everything else you can clean or check"
        }
        return "\(lead), each too small to draw by itself. They're all in the list."
    }

    /// The line under the map: what it covers, and what it leaves out.
    static func caption(mapped: UInt64, overview: Overview) -> String {
        let rest = overview.scannedBytes > mapped ? overview.scannedBytes - mapped : 0
        let lead = "The map shows the \(Format.bytes(mapped)) you can clean or check."
        guard rest > 0 else { return lead }
        return lead + " The other \(Format.bytes(rest)) in your home folder is your own files and apps, "
            + "which Roomy keeps."
    }

    /// The colour of a block. The tier is always named in words too.
    static func color(for tile: DiskMapTile) -> Color {
        tile.tier?.tint ?? .gray
    }

    /// Adds sizes without ever wrapping round.
    static func sum(_ sizes: [UInt64]) -> UInt64 {
        sizes.reduce(UInt64(0)) { sum, size in
            let (next, overflow) = sum.addingReportingOverflow(size)
            return overflow ? UInt64.max : next
        }
    }
}

/// The map as a section of the Overview, with its heading. Open by default: it is the quickest way to
/// see what is big and how safe it is to remove.
struct DiskMapSection: View {
    let overview: Overview

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .firstTextBaseline, spacing: 10) {
                Text("The map")
                    .font(.headline)
                    .accessibilityAddTraits(.isHeader)
                Text("Bigger blocks take more space")
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            DiskMap(overview: overview)
        }
    }
}

/// The Overview's map: everything the user can clean or check, as blocks sized by the space they take,
/// in one side per tier. Choosing a block explains it. Nothing is ticked or removed here.
struct DiskMap: View {
    @EnvironmentObject private var model: AppModel
    let overview: Overview
    @State private var groups: [DiskMapGroup] = []
    @State private var isLoaded = false
    /// The chosen block, as it was drawn: a side can fold more items into its shared block than
    /// `groups` holds (see `DiskMapContent.fitted`).
    @State private var selected: DiskMapTile?
    /// The height of the blocks. Wide enough windows give blocks room for their names.
    static let height: CGFloat = 300
    /// The gap between the two sides.
    static let gap: CGFloat = 14

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if !isLoaded {
                ProgressView()
                    .controlSize(.small)
                    .frame(maxWidth: .infinity, minHeight: 80)
            } else if groups.isEmpty {
                Text("Nothing to clean or check right now, so there's nothing to map.")
                    .foregroundStyle(.secondary)
            } else {
                GeometryReader { proxy in
                    let widths = DiskMapContent.widths(for: groups.map(\.bytes), in: proxy.size.width, gap: Self.gap)
                    HStack(alignment: .top, spacing: Self.gap) {
                        ForEach(Array(groups.enumerated()), id: \.element.id) { index, group in
                            DiskMapSide(group: group, selected: $selected)
                                .frame(width: widths[index])
                        }
                    }
                }
                .frame(height: Self.height + DiskMapSide.headingHeight)
                .accessibilityElement(children: .contain)
                .accessibilityLabel("Map of what the scan found")
                if let tile = selected {
                    DiskMapExplanation(tile: tile)
                } else {
                    Text("Choose a block to see what it is.")
                        .font(.callout)
                        .foregroundStyle(.secondary)
                }
                Text(DiskMapContent.caption(mapped: DiskMapContent.sum(groups.map(\.bytes)), overview: overview))
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .task(id: scanKey) {
            let items = await model.scannedItems()
            groups = DiskMapContent.groups(for: items)
            // A new scan: what the chosen block said may no longer be true.
            selected = nil
            isLoaded = true
        }
    }

    /// Changes when a new scan's results arrive, so the map is drawn again.
    private var scanKey: String {
        "\(overview.scannedBytes)-\(overview.likelySafeBytes)-\(overview.reviewFirstBytes)"
    }
}

/// One side of the map: the tier in words with its total, then its blocks.
struct DiskMapSide: View {
    let group: DiskMapGroup
    @Binding var selected: DiskMapTile?
    /// The room the heading takes above the blocks.
    static let headingHeight: CGFloat = 24

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 5) {
                Image(systemName: group.tier.symbol)
                    .foregroundStyle(group.tier.tint)
                    .accessibilityHidden(true)
                Text(group.tier.label)
                    .fontWeight(.semibold)
                Text(Format.bytes(group.bytes))
                    .foregroundStyle(.secondary)
                    .monospacedDigit()
            }
            .font(.caption)
            .lineLimit(1)
            .frame(height: Self.headingHeight, alignment: .top)
            .accessibilityElement(children: .combine)
            .accessibilityAddTraits(.isHeader)
            GeometryReader { proxy in
                let tiles = DiskMapContent.fitted(group.tiles, in: proxy.size)
                let rects = Treemap.layout(tiles.map { Double($0.bytes) },
                                           in: CGRect(origin: .zero, size: proxy.size))
                ZStack {
                    ForEach(Array(tiles.enumerated()), id: \.element.id) { index, tile in
                        let rect = rects[index]
                        DiskMapBlock(tile: tile, size: rect.size, isSelected: selected?.id == tile.id) {
                            selected = selected?.id == tile.id ? nil : tile
                        }
                        .frame(width: max(rect.width - 3, 0), height: max(rect.height - 3, 0))
                        .position(x: rect.midX, y: rect.midY)
                    }
                }
            }
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
                // A neutral card with only a hint of the tier's colour, and the colour itself in the
                // outline. A stronger wash turns muddy on a dark window, and a map full of Review first
                // items should not read as an alarm.
                RoundedRectangle(cornerRadius: 7, style: .continuous)
                    .fill(Color.primary.opacity(0.05))
                RoundedRectangle(cornerRadius: 7, style: .continuous)
                    .fill(DiskMapContent.color(for: tile).opacity(isSelected ? 0.3 : 0.09))
                RoundedRectangle(cornerRadius: 7, style: .continuous)
                    .strokeBorder(isSelected ? Color.primary.opacity(0.9) : DiskMapContent.color(for: tile).opacity(0.55),
                                  lineWidth: isSelected ? 2 : 1)
                if Self.showsName(in: size) {
                    VStack(alignment: .leading, spacing: 1) {
                        Text(tile.title)
                            .font(.caption.weight(.semibold))
                            .lineLimit(Self.titleLines(in: size))
                            .multilineTextAlignment(.leading)
                        // The name that tells it apart from blocks with the same title, when there is room.
                        if let place = tile.place, Self.showsPlace(in: size) {
                            Text(place)
                                .font(.caption2)
                                .lineLimit(1)
                                .truncationMode(.tail)
                        }
                        Text(Format.bytes(tile.bytes))
                            .font(.caption2.monospacedDigit())
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                    .foregroundStyle(.primary)
                    .padding(7)
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
        size.width >= 64 && size.height >= 36
    }

    /// How many lines the title may take: two when the block is tall enough, so a narrow block can
    /// still say "Android emulator" in full.
    static func titleLines(in size: CGSize) -> Int {
        size.height >= 84 ? 2 : 1
    }

    /// Whether a block also has room for the name that tells it apart.
    static func showsPlace(in size: CGSize) -> Bool {
        size.width >= 64 && size.height >= 54
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
                    // A search left in the toolbar could hide the very item being shown.
                    model.searchText = ""
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
