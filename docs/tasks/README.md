# Agent task specs

One task per Claude Code session. Start every session with:

> Read CLAUDE.md, then read docs/tasks/TXX-*.md and do only that task. Work on branch task/TXX-<name>. Open a PR when done.

| Task | Title | Workstream | Depends on | Review |
|---|---|---|---|---|
| T01 | Repo skeleton | Core | — | Standard |
| T02 | UniFFI bridge, one command end to end | Core | T01 | Standard |
| T03 | Protected paths and safety tests (tests only) | Safety | T01 | **Sam writes or approves line by line** |
| T04 | Fixture generator | Safety | T01 | Strict |
| T05 | Read-only scanner | Core | T01, T04 | Standard |
| T06 | Rule schema, loader, validator | Core | T01 | Standard |
| T07 | First 20 rules | Analysers | T06 | Sam signs off every rule |
| T08 | Classifier | Core | T05, T06 | Standard |
| T09 | Command contract: scan, overview, list items | Core | T02, T08 | Standard |
| T10 | SwiftUI shell and Overview on mock data | App | T01 | Standard + real-Mac check |
| T11 | Overview wired to the real core | App | T09, T10 | Standard + real-Mac check |
| T12 | CI on a macOS runner | Platform | T01 | Strict |
| T13 | Signing, notarisation, Sparkle pipeline | Platform | T11, T12 | Strict; Sam handles credentials |

Suggested parallel pairs (two sessions at a time): T01 alone → (T02, T03) → (T04, T06) → (T05, T07) → (T08, T10) → (T09, T12) → (T11) → (T13).

No executor or cleanup code is in weeks 1–2. That starts only after T03 is merged and Sam has approved it.
