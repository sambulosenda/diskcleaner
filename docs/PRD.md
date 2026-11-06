# PRD: Mac Storage Cleaner for Everyone

Nov 6, 2026 · @sam

## Overview

A native macOS app that shows any Mac user what is filling their disk, explains every item in plain words, and reclaims space safely. It serves everyday users through a simple mode and developers through a Developer mode with deep toolchain analysers. Working name is TBD; this draft is v0.4.

| Field | Value |  |
| --- | --- | --- |
| Product | Mac storage cleaner for everyone (name TBD) |  |
| Owner | Sam Senda, product and engineering |  |
| Status | Draft |  |
| Platform | macOS 13+, Apple silicon and Intel |  |

| Version | Change |
| --- | --- |
| 0.4 | Decided: free scan plus 5 GB cleanup, short allow-list of runnable tool commands, storage cleaner ships before the screenshot app |
| 0.3 | Decided: SwiftUI on a Rust core, Android analyser in P0, closed source for now |
| 0.2 | Broadened from mobile developers to all Mac users, with Developer mode |
| 0.1 | First draft, mobile developers only |

## Executive summary

Most Mac users hit "Storage almost full" and cannot tell what is safe to delete; macOS shows a large "System Data" bar and little else. This app scans a Mac in under a minute, names what each item is (iPhone backups, Messages attachments, app leftovers, caches, old installers, developer toolchains), says whether it can be removed, and cleans only through a previewed, confirmed, Trash-first workflow.

It wins against consumer cleaners (CleanMyMac) on honesty and explanation, with no scare tactics or fake "junk" counts. It wins against disk visualisers (DaisyDisk) by explaining meaning, not just size. And it wins against developer tools (DiskPilot, DevCleaner) by covering every toolchain in one polished, notarised app. Revenue comes from a free scanner and a paid licence for cleanup and advanced analysers.

## Problem statement

Storage fills up in places users never see, macOS groups much of it as "System Data", and the tools that promise to help either show sizes without meaning or delete "junk" without explanation. Sizes below are approximate, from typical machines; validating them on real Macs is an open question.

| Who | Location or item | What it holds | Typical size | Safe to clear? |
| --- | --- | --- | --- | --- |
| Everyone | iPhone and iPad backups (`~/Library/Application Support/MobileSync`) | Local device backups, often for old devices | 5–100 GB | Old devices, after confirming |
| Everyone | Messages attachments | Photos, videos and files from chats | 2–50 GB | Review first |
| Everyone | Downloads and old installers (`.dmg`, `.pkg`, `.zip`) | Files already installed or used | 1–30 GB | Mostly yes, user picks |
| Everyone | Duplicate files and large forgotten files | Copies and one-off videos or exports | Varies widely | User picks the copy to keep |
| Everyone | App caches and leftovers from deleted apps | Browser, Spotify, Slack caches; containers of removed apps | 2–20 GB | Caches yes; leftovers review first |
| Creators | Final Cut, Logic, Adobe and video render caches | Render files, sound libraries, media caches | 5–200 GB | Caches yes; libraries review first |
| Developers | Xcode DerivedData, simulators, device support | Build output and iOS runtimes | 10–80 GB | Mostly yes |
| Developers | Android SDK images, AVDs, Gradle | Emulators and build caches | 10–60 GB | Unused ones, yes |
| Developers | Docker images, volumes, build cache | Containers and their disk image | 10–100 GB | Images and cache yes; volumes never automatically |
| Developers | `node_modules`, Python venvs, Rust `target`, package caches | Regenerable dependencies and builds | 5–50 GB | Inactive projects, yes |
| Developers | Local AI models (Ollama, LM Studio), AI-tool session data | Downloaded models, transcripts, logs | 5–100 GB | Review first |

Existing tools fall short in three ways. Consumer cleaners use alarming "junk" totals and opaque one-click cleans, which erodes trust. Disk visualisers show what is big but not what it is or whether it is safe. Developer tools cover one ecosystem each, so a user runs several tools plus terminal commands.

## Goals, non-goals and success metrics

The product succeeds if any Mac user understands their storage in one scan, reclaims meaningful space without losing anything they needed, and trusts it enough to pay.

**Goals**

- Reclaim at least 10 GB for everyday users and 20 GB for developers in the first session.
- Make every item understandable to a non-technical user in one sentence.
- Zero data-loss incidents caused by recommended cleanups.
- Feel like a first-class Mac app: signed, notarised, auto-updating, native UI.
- Reach a sustainable indie revenue line from paid licences.

**Non-goals for v1**

- Automatic or scheduled deletion; every removal is approved by the user.
- Inflated "junk" counts, RAM "optimisation", malware scanning or other scare features.
- Deleting inside Photos, Mail or Music libraries directly; the app points users to each app's own tools instead.
- Mac App Store distribution, since the sandbox blocks full-disk scanning.
- Windows or Linux versions.

**Success metrics (first 6 months after launch)**

| Metric | Target |
| --- | --- |
| Median space reclaimed in first session, everyday users | ≥ 10 GB |
| Median space reclaimed in first session, Developer mode | ≥ 20 GB |
| Scan time, 1 TB disk, Apple silicon | < 60 s |
| Free download to paid conversion | ≥ 4% |
| Paid licences sold | 1,500 |
| Data-loss reports confirmed as app-caused | 0 |
| Crash-free sessions | ≥ 99.5% |
| Users who run a second cleanup within 60 days | ≥ 40% |

## Target users and personas

The app serves three groups from one product: everyday users in a simple mode, and creators and developers through extra analysers they can switch on.

| Persona | Situation | What they need |
| --- | --- | --- |
| Everyday user (largest group) | 256 GB MacBook, "Storage almost full" warnings, iPhone backups, years of Downloads and Messages | A clear answer to "what can I delete?" with no jargon and no risk |
| Student or family Mac user | Shared or older Mac, limited budget | Free scan that shows the big wins; cleanup that is safe to hand to anyone |
| Creator (video, music, design) | Final Cut, Logic, Adobe apps with large render caches and libraries | Safe removal of render files and caches without touching projects or libraries |
| Developer (power users) | Xcode, Android Studio, Docker, Node, Python, Rust, AI tools | One tool that understands every toolchain and project, with commands shown |
| Small business or IT helper | Looks after several Macs for family, staff or clients | A trustworthy app with history to show what was removed and why |

## User stories

1. As an everyday user, I want one screen that says how much I can safely free up, so that I can fix "Storage almost full" without understanding folders.
2. As an everyday user, I want to see old iPhone and iPad backups with device name and date, so that I can remove backups for devices I no longer own.
3. As an everyday user, I want to find large files, duplicates and old installers, so that I can choose what to delete myself.
4. As an everyday user, I want to see what is in "System Data", so that the mystery bar finally makes sense.
5. As a creator, I want to clear render and media caches without touching my projects or libraries, so that I recover space before a big export.
6. As a developer, I want to see every toolchain's usage (Xcode, Android, Docker, Node, Python, Rust, AI tools) in one place, so that I stop running several cleanup tools.
7. As a developer, I want to clean dependencies and build output in projects I have not touched in 90 days, so that dormant work stops eating space.
8. As any user, I want every recommendation to say why it is safe and what could go wrong, so that I trust what I approve.
9. As any user, I want removals to go to the Trash with a full history, so that I can recover from a mistake.
10. As any user, I want a menu bar indicator that warns when free space is low, so that I act before things break.

## Functional requirements

P0 is required for launch, P1 is high value soon after, P2 is nice to have, P3 is future.

**Core (everyone)**

| ID | Requirement | Priority |
| --- | --- | --- |
| F1 | Parallel, read-only scan of the home folder; allocated size, no symlink following, hard links counted once, stays on one volume | P0 |
| F2 | Overview: one headline ("You can likely free 34 GB"), disk strip by category, top items to review | P0 |
| F3 | Plain-language explanation for every item: what it is, why it exists, whether it comes back | P0 |
| F4 | Classification rules as data, each item showing rule, confidence and source folder | P0 |
| F5 | Recommendations in three tiers (Likely safe, Review first, Keep), each with reason, risk and estimated space | P0 |
| F6 | Cleanup plan previewing every path; confirm before acting; Trash by default | P0 |
| F7 | History log of every action: paths, operation, space reclaimed, errors | P0 |
| F8 | Free scan and up to 5 GB of cleanup in total; paid licence unlocks unlimited cleanup and advanced analysers | P0 |
| F9 | "System Data" breakdown: Time Machine local snapshots, caches, logs, iOS updates, sleep image, explained | P1 |
| F10 | Menu bar item with free space and low-space alert | P1 |
| F11 | Scan diff: what grew or shrank since the last scan | P1 |
| F12 | Incremental rescans using FSEvents | P1 |

**Everyday analysers**

| ID | Requirement | Priority |
| --- | --- | --- |
| E1 | iPhone and iPad backups with device name, iOS version and date; delete old ones | P0 |
| E2 | Large files and old Downloads, sorted by size and last opened; size is never a reason to delete on its own | P0 |
| E3 | Old installers (`.dmg`, `.pkg`, `.ipsw`, archives already extracted) | P0 |
| E4 | App caches (browsers, Spotify, Slack, Teams, Zoom) and leftovers from deleted apps, leftovers review-only | P0 |
| E5 | Duplicate finder: size then SHA-256, APFS clones flagged, user picks the copy to keep, byte check before Trash | P1 |
| E6 | Messages attachments by size and conversation date, opened in Finder rather than deleted by the app | P1 |
| E7 | Photos, Mail and Music library sizes with guidance to each app's own cleanup tools | P1 |
| E8 | App uninstaller that removes an app and its support files together, previewed | P2 |

**Creator analysers**

| ID | Requirement | Priority |
| --- | --- | --- |
| C1 | Final Cut Pro render and proxy files per library, Logic and GarageBand sound library sizes | P1 |
| C2 | Adobe media cache and Premiere/After Effects disk caches | P1 |
| C3 | DaVinci Resolve and other render cache locations | P2 |

**Developer mode analysers**

| ID | Requirement | Priority |
| --- | --- | --- |
| D1 | Xcode: DerivedData per project, archives, iOS DeviceSupport by version, CoreSimulator devices and runtimes | P0 |
| D2 | JavaScript: `node_modules` per project with last activity, npm/pnpm/Yarn/Bun caches | P0 |
| D3 | Docker: images, containers, build cache and disk image, previewed actions only, never volumes | P0 |
| D4 | Android: system images by API level, AVDs, build-tools and NDKs, cross-checked against projects; Gradle caches and versions | P0 |
| D5 | React Native and Expo: Pods, native build folders, CocoaPods, Metro, Watchman and Expo caches | P1 |
| D6 | Python, Rust, Go, Java/Maven: venvs, `target`, module and package caches, toolchains | P1 |
| D7 | Editors and AI tools: VS Code, Cursor, JetBrains, Claude Code, Codex; Ollama and LM Studio models | P1 |
| D8 | Homebrew cache and old versions; Flutter | P2 |
| D9 | v1 runs a fixed allow-list of previewed tool commands: `xcrun simctl delete unavailable`, `docker image prune`, `docker builder prune`, `brew cleanup` and `sdkmanager --uninstall <package>`, each listing what it affects before the user confirms. Any other tool command is shown for the user to copy and run | P0 |
| D10 | Read-only MCP server so coding agents can ask where space went | P3 |
| D11 | Optional AI assistant over scan data, user's own API key, read-only | P3 |

## Safety model and non-functional requirements

Only one module may modify the filesystem, and every path it touches is validated twice. One destructive bug ends the product, so these rules are P0 and non-negotiable.

**Safety rules**

- Scanner, classifier, analysers and recommendation engine are read-only by construction; only the cleanup executor can move or delete.
- The UI sends recommendation IDs, never raw paths; paths come only from the engine's own recommendations.
- Each path is validated when the plan is built and again immediately before acting: absolute, inside the home folder, outside a protected list, no symlink in the path or any parent.
- Cleanup plans are single use and expire after 10 minutes.
- Trash is the default; permanent deletion is offered only when every selected item is regenerable.
- No shell string construction; tool commands run from a fixed allow-list with validated arguments.
- Personal content (Documents, Desktop, Photos, Mail, Messages and Music libraries, iCloud Drive) is never in the Likely safe tier; the app only lists it for the user to review, or hands off to the owning app.
- Developer secrets and state (source files, `.git`, keystores, signing certificates, `.env` files, Docker volumes, emulator snapshots) are never removed without an explicit Review-first confirmation.
- Integration tests run every cleanup policy against real temporary folders in CI.

**Non-functional requirements**

| Area | Requirement |
| --- | --- |
| Performance | Scan 1 TB / 2M files in under 60 s on M1; UI stays responsive during scans |
| Memory | Peak under 500 MB during a full scan |
| Privacy | No telemetry by default; opt-in anonymous crash reports only; no file contents read |
| Permissions | Works without Full Disk Access; guided onboarding if the user grants it |
| Distribution | Developer ID signed, notarised, hardened runtime, Sparkle auto-update |
| Compatibility | macOS 13 Ventura or later, universal binary |
| Accessibility | Full VoiceOver labels, keyboard navigation, Dynamic Type where supported |

## Technical architecture

A Rust core does all scanning and cleanup; a native SwiftUI app sits on top and can only call typed commands.

_[Diagram not exported: architecture · read-only engine, single write path. See the PRD doc in Claude.]_

The UI never handles raw paths, so a UI bug cannot delete the wrong folder. Chosen stack: Rust with rayon for the parallel scanner, UniFFI bindings to SwiftUI, SQLite for scan and cleanup history, Sparkle for updates, and FSEvents for incremental rescans in P1. Classification rules and analyser definitions ship as versioned data files so they can be updated without a new binary.

## Design and UX considerations

The app should feel like it shipped with macOS: calm, honest, and safe to hand to a parent or a senior engineer alike.

- Two modes from one app. Simple mode (default) shows plain categories: Backups, Downloads, Large files, Duplicates, Apps, Caches, System Data. Developer mode adds Projects, Xcode, Android, Docker, JavaScript, Python and more, and shows paths and commands.
- Onboarding asks one question, "Do you write code or make videos/music?", and turns on the matching analysers; users can change it later in Settings.
- Lead with the answer: the first screen after a scan says "You can likely free 34 GB" with the breakdown underneath.
- No scare tactics: no red warnings about "junk", no inflated totals, no countdowns; numbers shown are only what the app can actually free.
- Explanation before action: every row expands to show what the item is, why it exists, whether it comes back, and how to get it back.
- Native SwiftUI with standard Mac conventions: sidebar, toolbar, Settings window, keyboard shortcuts.
- Tier colour is secondary to text labels, so tiers read correctly for colour-blind users.
- The confirm sheet lists items and total size, with Trash selected by default and permanent delete behind a second toggle; Developer mode also shows exact paths.
- Empty and error states explain the next step, for example Full Disk Access not granted.

## Distribution, pricing and business model

Sell direct as a notarised download with a free scanner and a one-time paid licence; prices below are proposals to test, in GBP.

| Element | Proposal |
| --- | --- |
| Channel | Direct download from the product site plus Homebrew cask; not the Mac App Store |
| Payments | A merchant of record (Paddle or Lemon Squeezy) to handle VAT and licence keys |
| Free tier | Full scan, overview, explanations and recommendations; cleanup limited to 5 GB so users feel the value |
| Personal licence | £24 one-time, 2 Macs, everyday and creator analysers, 12 months of updates |
| Pro licence | £39 one-time, adds Developer mode; update renewal £15 a year |
| Family / team | £59 for 5 Macs (family) or £119 for 5 seats with Pro (teams) |
| Later channel | Setapp listing once the product is stable |
| Launch, everyone | Product Hunt, Mac-focused YouTubers and blogs, Reddit r/mac and r/macapps, a short "free 40 GB in 2 minutes" demo video |
| Launch, developers | Show HN, React Native, Expo and iOS dev communities, LinkedIn, X/Twitter |

The split lets the everyday product stay cheap and simple while developers pay more for the deeper analysers. The code stays closed source for now; open-sourcing the engine can be revisited if trust becomes the main barrier.

## Competitive landscape

"Honest and explained" is already a crowded pitch; the open gap is one app simple enough for anyone that also has deep developer and creator analysers, so positioning should lead with Developer mode.

| Group | Products | Strength | Gap this product fills |
| --- | --- | --- | --- |
| Mainstream all-in-one cleaners | CleanMyMac (MacPaw), [Sensei](https://cindori.com/reviews/best-mac-cleaner-apps-2026) (Cindori), MacCleaner Pro, MacKeeper, Cleaner One Pro, AVG Cleaner, Tenorshare Cleamio | Polished, broad: cleanup, uninstalling, duplicates, monitoring; CleanMyMac now offers subscription or one-time purchase | Shallow or no developer and creator toolchain knowledge; several use "junk" framing; MacKeeper carries reputation baggage |
| Disk visualisers | DaisyDisk, GrandPerspective, OmniDiskSweeper | Clear visual map of what is big | Show size, not meaning or safety; no recommendations |
| Free utilities | macOS Storage settings, OnyX, AppCleaner, Pearcleaner | Free and trusted for narrow jobs | Users must combine several; vague "System Data" |
| New indie "honest" cleaners | [CleanDisk](https://cacheclear-site.pages.dev/blog/cleandisk-vs-daisydisk-vs-cleanmymac/), [other CleanMyMac alternatives](https://diskcleaner-site.pages.dev/blog/cleanmymac-alternative) | Same pitch as ours: safety labels per item, 60-second scans, one-time purchase, criticism of CleanMyMac's background deletion | Consumer-only; no toolchain depth; same message, so harder to stand out on honesty alone |
| Developer tools | [DiskPilot](https://github.com/Devanshgoel-123/diskpilot), DevCleaner for Xcode, npkill | Deep in their ecosystem; DiskPilot has a strong safety model | Developer-only or single-ecosystem; DiskPilot is an unnotarised v0.4 |

Many "best Mac cleaner" roundups are published by vendors (MacPaw, Cindori, MacKeeper) or carry affiliate links, so rankings there are not neutral. Pricing and feature details should be checked on each product's own site before launch messaging. Sources as of November 2026: [Cindori review](https://cindori.com/reviews/best-mac-cleaner-apps-2026), [Macworld roundup](https://www.macworld.com/article/673271/best-mac-cleaner-tested-compared.html), [Toolradar free cleaners](https://toolradar.com/blog/best-free-mac-cleaners).

## Risks and mitigations

The biggest risk is a cleanup that destroys something a user needed; the second is trying to serve everyone and pleasing no one.

| Risk | Impact | Mitigation |
| --- | --- | --- |
| A recommendation deletes needed data (a photo, a backup, a keystore) | Severe: trust and reputation lost | Personal content never Likely safe, protected-path list, Trash default, CI tests per policy, public beta before launch |
| Broad scope dilutes the product and slows delivery | Late, unfocused launch | Ship everyday core plus Xcode, JS, Docker and Android analysers first; other analysers follow as data updates |
| Simple mode confuses non-technical users | Low conversion, refunds | Usability tests with 5 non-technical users before launch; plain-language copy review |
| Competing with CleanMyMac's marketing budget | Low awareness | Win on honesty and explanation; creator and developer communities as early adopters |
| Toolchain and app layouts change with updates | Wrong sizes or missed items | Rules as versioned data, shipped via update; test matrix per major macOS and Xcode release |
| Full Disk Access prompts confuse users | Drop-off at onboarding | Work fully without FDA; explain each prompt before it appears |
| Solo-maker bandwidth split with other products | Slow delivery | Tight P0 scope; share the Rust core and release pipeline with other Mac apps |

## Timeline and milestones

Built with AI agents and one human reviewer, public launch is roughly 10–16 weeks away; the range depends mostly on how many hours a week Sam can review and test.

_[Diagram not exported: roadmap · 5 phases, 4 gates. See the PRD doc in Claude.]_

Phase durations are first guesses for agent-built work and will be revised after the first two weeks. A failed gate extends its phase rather than moving to the next.

## Open questions

- [x] UI stack: SwiftUI front end on a Rust core via UniFFI.
- [x] P0 developer analysers: Xcode, JS, Docker and Android.
- [x] Source model: closed source for now.
- [x] Free tier: full scan plus up to 5 GB of cleanup.
- [x] Tool commands: v1 runs a short allow-list (requirement D9) and shows the rest.
- [x] Ship order: storage cleaner ships before the Mac screenshot app and builds the shared release pipeline.
- [ ] Product name and domain.
- [ ] Final prices for Personal and Pro; validate with the waitlist.
- [ ] Typical reclaimable sizes: measure on 10–20 real Macs (everyday and developer) before launch messaging.

# Developer handoff

## Team and workstreams

The app is built by AI coding agents in Claude Code, with Sam as product owner and the only human reviewer; Sam's review and testing hours set the pace, not the number of agents.

| Workstream | Run as | Owns | Review level |
| --- | --- | --- | --- |
| Rust core | Agent session on its own branch | Scanner, classifier, rules engine, UniFFI bindings, command contract | Standard: tests pass, Sam skims the diff |
| Rust analysers | Agent session on its own branch | Everyday, Xcode, Android, JS, Docker analysers; tool-command allow-list | Standard, plus Sam signs off every rule file |
| Safety and platform | Agent session on its own branch | Planner, safety gate, executor, history, licensing, CI, release pipeline | Strict: Sam reviews every line |
| SwiftUI app | Agent session on its own branch | All screens, onboarding, both modes, accessibility | Standard, plus Sam tests on a real Mac |
| Sam | Human | Scope, task specs, test approval, rule sign-off, signing credentials, payments account, real-Mac testing, merges | — |

**Working rules for agents**

- Every session starts from `CLAUDE.md` at the repo root, which holds the architecture, safety rules and conventions.
- One task spec per session, from `docs/tasks/`, each with its own done-criteria; an agent does not start a second task until Sam merges the first.
- Safety tests and the protected-path list live in files agents are told never to edit; a change there needs Sam to make it by hand.
- Start with two agent sessions in parallel, and add more only if review keeps up.
- Every change lands through a pull request that Sam merges; agents never push to `main`.

## Acceptance criteria (P0)

A P0 requirement is done only when every criterion below passes in CI or on the reference test Macs.

| ID | Done when |
| --- | --- |
| F1 | Scans 1 TB / 2M files in under 60 s on an M1 with 8 GB RAM; peak memory under 500 MB; total within 1% of `du -sk` on the same tree; symlink loops and other volumes are never followed |
| F2 | Headline figure equals the sum of Likely safe items exactly; overview renders within 1 s of scan completion |
| F3 | Every item that appears in the UI has a non-empty explanation of at most 2 sentences, reviewed by Sam; no item shows only a path |
| F4 | Each rule declares id, category, tier, confidence and explanation; a rule file that fails schema validation blocks the build |
| F5 | Every recommendation shows reason, risk, estimated space and tier; personal-content paths never appear as Likely safe |
| F6 | Confirm sheet lists every item and the total; nothing changes on disk before confirm; Trash is preselected; plans expire after 10 minutes |
| F7 | Every executed action is in History with timestamp, items, operation, bytes reclaimed and any error; History survives app restart and update |
| F8 | Without a licence, cleanup stops at a cumulative 5 GB with a clear upgrade prompt; with a licence, no limit; scans are never limited |
| E1 | Lists every device backup with device name, iOS version, date and size, matching Finder's backup list |
| E2 | Shows the 500 largest files with size and last-opened date; none are ever preselected |
| E3 | Finds `.dmg`, `.pkg`, `.ipsw` and archives in Downloads, with size and age |
| E4 | Recognises caches for the 10 most common apps (browsers, Spotify, Slack, Teams, Zoom and others); app leftovers are always Review first |
| D1 | Matches Xcode's DerivedData and simulator lists; flags DerivedData for deleted projects; shows DeviceSupport grouped by iOS version |
| D2 | Finds every `node_modules` under the scan root with last-activity date from the package manifest |
| D3 | Matches `docker system df` totals; volumes are listed but never offered for removal; works when Docker is absent or stopped |
| D4 | Lists system images by API level and AVDs with last-used date; flags SDK packages no scanned project's Gradle file references |
| D9 | Each allow-listed command lists affected items first, runs only after confirm, and records its full output in History |

## Protected paths

The safety gate enforces two lists in code, not in rule files, so no rule update can loosen them. Hard-blocked paths are refused by the executor outright; review-only paths can be acted on only by an item the user selected individually in the Review first tier.

**Hard-blocked (executor refuses, no override)**

- Anything outside the user's home folder, including `/System`, `/Library`, `/usr`, `/bin`, `/private` and `/Applications` (apps are removed only through the uninstaller in P2).
- The home folder itself and any top-level home folder: `~/Desktop`, `~/Documents`, `~/Downloads`, `~/Pictures`, `~/Movies`, `~/Music`, `~/Library` (contents can be targeted, the folders themselves never).
- `~/Library/Keychains`, `~/Library/Mail`, `~/Library/Messages` (the database), `~/Library/Mobile Documents` (iCloud Drive), `~/Library/CloudStorage`.
- Photos, Music and TV libraries: any `*.photoslibrary`, `*.musiclibrary`, `*.tvlibrary` bundle.
- `~/.ssh`, `~/.gnupg`, `~/.aws`, `~/.config` credential files, `~/.docker/config.json`, `~/.netrc`.
- Any `.git` folder, and any folder that contains one, except named build output inside it (`node_modules`, `build`, `target`, `DerivedData`).
- Signing material: `~/Library/MobileDevice/Provisioning Profiles`, any `*.keystore`, `*.jks`, `*.p12`, `*.mobileprovision`, `*.p8`.
- Docker volumes, by identifier, in every Docker action.

**Review-only (never Likely safe, never preselected)**

- iPhone and iPad backups.
- Messages attachments (`~/Library/Messages/Attachments`), shown and opened in Finder only.
- Files in Downloads, Desktop and Documents, including large files and duplicates.
- App leftovers in `~/Library/Containers` and `~/Library/Application Support` for apps no longer installed.
- Android AVDs and emulator snapshots; Xcode archives.
- Any `.env` file, and AI-tool session history (Claude Code, Codex, Cursor).
- Final Cut, Logic and Adobe libraries (caches inside them can be Likely safe).

Adding to either list needs Sam's sign-off; removing from either list needs Sam's sign-off plus a passing test proving the change is safe.

## Classification rule schema

Rules are TOML files shipped with the app and updatable via Sparkle; the engine validates each file against this schema at build time and at load time, and rejects any rule that fails.

| Field | Type | Required | Meaning |
| --- | --- | --- | --- |
| `id` | string, dotted | Yes | Stable unique id, e.g. `xcode.derived_data` |
| `version` | integer | Yes | Bumped on any change; History records which version acted |
| `match.path` | glob, relative to home | Yes | Where the item lives, e.g. `Library/Developer/Xcode/DerivedData/*` |
| `match.requires` | list of globs | No | Sibling files that must exist for the match, e.g. `info.plist` |
| `category` | enum | Yes | Cache, Build, Developer tools, Docker, App data, Logs, Downloads, User data, Backups, System, Unknown |
| `tier` | enum | Yes | `likely_safe`, `review_first` or `keep` |
| `confidence` | enum | Yes | `high`, `medium`, `low`; `low` can never be `likely_safe` |
| `regenerable` | boolean | Yes | Whether permanent delete may be offered |
| `mode` | enum | Yes | `simple` (everyone) or `developer` |
| `title` | string | Yes | Short display name |
| `explain` | string, ≤ 2 sentences | Yes | What it is and why it exists, in plain words |
| `risk` | string | Yes | What could go wrong if removed |
| `restore` | string | No | How it comes back, e.g. "Rebuilds on next Xcode build" |
| `action` | enum | Yes | `trash`, `command` or `show_only` |
| `command` | string, allow-list id | If `action = command` | One of the five D9 commands |
| `per_item` | boolean | No | Treat each child folder as its own item (one per project) |

```toml
[[rule]]
id = "xcode.derived_data"
version = 1
match.path = "Library/Developer/Xcode/DerivedData/*"
category = "Build"
tier = "likely_safe"
confidence = "high"
regenerable = true
mode = "developer"
per_item = true
title = "Xcode build data"
explain = "Temporary files Xcode creates while building a project."
risk = "The next build of this project will take longer."
restore = "Rebuilds automatically on the next Xcode build."
action = "trash"

[[rule]]
id = "ios.device_backup"
version = 1
match.path = "Library/Application Support/MobileSync/Backup/*"
match.requires = ["Info.plist"]
category = "Backups"
tier = "review_first"
confidence = "high"
regenerable = false
mode = "simple"
per_item = true
title = "iPhone or iPad backup"
explain = "A full local backup of one of your devices."
risk = "If this device has no iCloud backup, its data could be lost for good."
action = "trash"

[[rule]]
id = "ios.simulators_unavailable"
version = 1
match.path = "Library/Developer/CoreSimulator/Devices/*"
category = "Developer tools"
tier = "likely_safe"
confidence = "high"
regenerable = true
mode = "developer"
title = "Unavailable iOS simulators"
explain = "Simulators for iOS versions no longer installed in Xcode."
risk = "None; they cannot run without their iOS runtime."
action = "command"
command = "xcrun_simctl_delete_unavailable"
```

The schema will grow (for example size thresholds or age conditions); any new field must default to the safer behaviour.

## Licensing spec

Licensing runs through the merchant of record's licence API (Paddle or Lemon Squeezy, decided in week 1) and must never block scanning or reading History.

| Area | Behaviour |
| --- | --- |
| Tiers | Free, Personal, Pro, Family (5 Macs, Personal features), Team (5 seats, Pro features) |
| Activation | User pastes a key or clicks a link from the receipt; app calls the activation API once and stores a signed licence token in the Keychain |
| Device limit | Personal and Pro: 2 Macs. A third activation shows the existing devices and lets the user deactivate one |
| Offline use | Token is verified locally; app re-checks online every 30 days; if it cannot reach the server it keeps working for 30 more days, then reverts to Free until it can check |
| Free 5 GB | Cumulative bytes reclaimed are stored in the Keychain, survive reinstall, and count only completed cleanups; a cleanup that would cross 5 GB is allowed to finish the selected item, then further cleanup prompts upgrade |
| Feature gates | Free and Personal: simple mode analysers. Pro: Developer mode analysers and tool commands. Gates are checked in the Rust core, not only the UI |
| Updates | Licence includes 12 months of updates; after that the app keeps working on the last eligible version and Sparkle offers renewal at £15 |
| Refunds | A refunded licence is revoked at the next online check |
| Privacy | The only data sent is the licence key, a random device id and app version; no scan data ever |

## Test fixtures and the zero-data-loss suite

Every cleanup rule and every allow-listed command needs a passing fixture test before it can merge; the suite runs on every pull request on a macOS CI runner.

**Fixtures** are generated folder trees in a temporary home, built by a fixture script so they stay small and reproducible:

- Everyday: device backups with real `Info.plist` structure, Downloads with installers and archives, app caches, a fake Photos library bundle, Messages attachments.
- Xcode: DerivedData for two existing and one deleted project, DeviceSupport for three iOS versions, simulator devices including unavailable ones, an archive.
- Android: SDK with three system images, two AVDs, build-tools and NDK versions, plus two projects whose `build.gradle` references only some of them; `~/.gradle` with several wrapper versions.
- JS: three projects with `node_modules`, one with a manifest untouched for 120 days; npm, pnpm and Yarn caches.
- Docker: tests run against a mocked Docker CLI that returns recorded `docker system df` and `docker image ls` output.
- Traps: symlinks pointing outside the home folder and into protected paths, a hard-linked file, a `.git` folder inside a build directory, a `.env` file, a keystore, a folder on a second mounted disk image.

**Suite assertions**

- After every cleanup, every file outside the selected items is byte-identical to before (checksummed tree diff).
- No path on either protected list is ever moved or deleted, even when a test rule deliberately targets it.
- Symlinks are never followed; the hard-linked file is counted once; the second volume is never entered.
- Trash moves are recoverable: each item restored from the Trash matches its original checksum.
- An expired plan, a reused plan and a plan whose path changed after preview are all refused.
- Licence gates hold when the UI is bypassed and core commands are called directly.

A failing assertion blocks release; there is no override flag.

## First two weeks: kickoff checklist

By the end of week 2 there should be a signed, notarised build that scans a real Mac and shows results, even if cleanup does nothing yet.

**Sam**

- [ ] Free disk space on the build Mac (at least 50 GB) before any agent work.
- [ ] Create the repo in the Claude Code project; add `CLAUDE.md` and `docs/tasks/` from the handoff files.
- [ ] Apple Developer ID certificate and notarisation credentials set up locally (never given to agents as plain text).
- [ ] Choose Paddle or Lemon Squeezy.
- [ ] Review and approve the safety test task (T03) before any executor work starts.
- [ ] Low-fi wireframes from the Wireframe brief, or ask an agent to build clickable SwiftUI mock screens from it.
- [ ] Landing page and waitlist live; send the measurement script to 10–20 Macs.
- [ ] Pick a codename for the bundle ID until the product name is chosen.

**Agent tasks, weeks 1–2** (specs in `docs/tasks/`)

- [ ] T01 Repo skeleton: Rust workspace, SwiftUI app, build scripts.
- [ ] T02 UniFFI bridge with one command, end to end.
- [ ] T03 Protected-path list and safety gate tests (tests only, no executor).
- [ ] T04 Fixture generator script.
- [ ] T05 Read-only scanner meeting F1.
- [ ] T06 Rule schema, loader and validator.
- [ ] T07 First 20 rules (Xcode, JS, everyday).
- [ ] T08 Classifier joining scan results to rules.
- [ ] T09 Command contract: scan, overview, list items.
- [ ] T10 SwiftUI shell and Overview on mock data.
- [ ] T11 Overview wired to the real core.
- [ ] T12 CI on a macOS runner.
- [ ] T13 Signing, notarisation and Sparkle pipeline (Sam runs the credential steps).

## Wireframe brief

Five screens cover the P0 flow: onboarding, Overview, the item list with its detail panel, the confirm sheet, and History. Each needs a low-fi wireframe in simple mode and Developer mode, plus the empty, loading and error states listed.

**Window layout (all screens)**

Standard Mac window, minimum 960 × 640. Left sidebar, main content area, and a toolbar holding Scan, a search field and a mode indicator. The sidebar in simple mode lists Overview, Backups, Downloads and installers, Large files, Apps and caches, System Data, then Cleanup and History. Developer mode adds a Developer group with Projects, Xcode, Android, JavaScript and Docker.

**1. Onboarding (first launch only)**

- Screen 1, "See what's filling your Mac": one sentence on what the app does and the promise that nothing is deleted without approval, with a Continue button.
- Screen 2, privacy: everything stays on this Mac, no account, sizes only. Explains that macOS may ask for folder access and that saying no is fine.
- Screen 3, "Do you write code or make videos or music?" with three choices: Not really, I make videos or music, I write code. This sets the mode and is changeable in Settings. A Start scan button.
- States: the scan starts immediately and moves to the Overview loading state.

**2. Overview**

- Top: headline "You can likely free 34 GB", with free space before and after shown as two numbers.
- Below it, a single horizontal disk strip split by category, with recoverable space shaded inside each segment and a legend underneath.
- "Worth reviewing": the 5–8 biggest recommendations as rows (icon, title, one-line explanation, size, tier label), each opening its detail.
- A primary button "Review and clean" opening Cleanup with Likely safe items preselected.
- Footer: last scan time, items not measured because of permissions, and a link to grant Full Disk Access.
- Developer mode: an extra card "Developer tools: 41 GB" broken down by toolchain.
- States: scanning (progress bar, live count of GB found, cancel button); first-ever empty state; scan error.

**3. Item list and detail panel (category screens and Cleanup)**

- List on the left two-thirds: checkbox, icon, title, size, tier label (Likely safe, Review first, Keep), and last-used date where known. Sortable by size and date; filter by tier.
- Keep items show no checkbox. Review first items are never preselected.
- Detail panel on the right third, for the selected row: title, size, "What it is", "Why it's here", "If you remove it" (the risk), "How it comes back", and Reveal in Finder.
- Developer mode adds the path, the rule id and confidence, and the tool command if the item uses one.
- Bottom bar: "12 items selected · 18.4 GB" with a Review cleanup button.
- Free tier: the bottom bar shows "3.1 GB of your 5 GB free cleanup left" and an Upgrade link.
- States: category empty ("Nothing here takes much space"), no access (with an explanation and a grant-access link).

**4. Confirm sheet (modal)**

- Title "Move 12 items to the Trash?" with the total size.
- A scrollable list of every item with its size, grouped by tier. Developer mode shows full paths and any commands that will run, with their affected items.
- Options: Move to Trash, preselected. Delete permanently appears only if every item is regenerable, behind a second toggle with a warning line.
- Buttons: Cancel and Move to Trash, or "Run and clean" if commands are included.
- After confirm: per-item progress with a Stop button, then a result summary showing space freed, any items skipped and why, and "How to get things back".
- States: plan expired ("Things changed since you reviewed; review again"); free-tier limit reached part way through.

**5. History**

- A table, newest first: date, what was cleaned (a summary such as "Xcode build data, 4 projects"), space freed, method (Trash, permanent or command), and status.
- Selecting a row shows each item, its size and result. Developer mode also shows paths and full command output.
- An "Open Trash" shortcut for recent Trash moves.
- Empty state: "Nothing cleaned yet."

**Design principles to hold across screens:** labels carry meaning, never colour alone; no alarming red for anything except a real error; numbers shown are only what can actually be freed; every destructive button says exactly what it will do.
