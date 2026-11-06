# T10 — SwiftUI shell and Overview on mock data

**Goal:** the app's structure and the Overview screen, using mock data shaped like `docs/command-contract.md` (from T09; if T09 is not merged, use the draft types and note it).

**Build** following the Wireframe brief in `docs/PRD.md`:
- Window min 960 × 640; `NavigationSplitView` sidebar: Overview, Backups, Downloads and installers, Large files, Apps and caches, System Data, Cleanup, History. A Developer group (Projects, Xcode, Android, JavaScript, Docker) shown only in Developer mode.
- Toolbar: Scan button, search field, mode indicator.
- Overview: headline ("You can likely free 34 GB"), free space before and after, a horizontal category strip with legend, "Worth reviewing" list (5–8 rows), "Review and clean" button (disabled for now), footer with last scan time and unreadable count.
- Scanning state with progress and Cancel; empty state; error state.
- Settings window with a Mode picker (Simple / Developer).
- A `CoreClient` protocol with `MockCoreClient` so T11 can swap in the real one.

**Done when**
- Runs on macOS 13 and the current macOS; light and dark mode both correct.
- VoiceOver reads every control with a meaningful label; full keyboard navigation of the sidebar.
- Tier is always shown as text, never colour alone.
- Screenshots of each state attached to the PR.
