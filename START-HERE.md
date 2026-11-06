# Start here (for Sam)

## One-time setup
1. Free at least 50 GB on the build Mac.
2. In your Claude Code project, create a new repo (or use the project's repo) and add everything in this folder at the repo root, keeping the paths: `CLAUDE.md`, `.claude/settings.json`, `docs/`, `scripts/`.
3. Replace `docs/PRD.md` with the PRD downloaded as Markdown.
4. Replace `CODENAME` in `CLAUDE.md` and `docs/tasks/T01-repo-skeleton.md` with your chosen codename.
5. Commit to `main` yourself. From now on, agents only open PRs.
6. Check `.claude/settings.json` works in your Claude Code version (try asking an agent to edit `CLAUDE.md`; it should be refused). Adjust the syntax if your version differs.

## First session prompt (paste into Claude Code)
> Read CLAUDE.md and docs/tasks/README.md. Then read docs/tasks/T01-repo-skeleton.md and do only that task. Work on branch task/T01-repo-skeleton. Run all checks listed under "Done when", then open a pull request with the description format from CLAUDE.md. Do not start any other task.

## Every later session
> Read CLAUDE.md, then docs/tasks/TXX-<name>.md, and do only that task on branch task/TXX-<name>. Open a PR when every "Done when" item passes.

## Your review checklist for each PR
- Does it do only the task? Anything extra → ask for it to be removed.
- Did any test get weaker, skipped or deleted? → reject.
- Any filesystem write, delete, rename or process spawn outside `executor`? → reject.
- New dependencies listed and justified?
- For rules: wording plain, tier sensible, evidence linked?
- Pull the branch and run it yourself for anything touching the app.

## Order
T01 → (T02 + T03) → (T04 + T06) → (T05 + T07) → (T08 + T10) → (T09 + T12) → T11 → T13. Do not start any cleanup or executor work until T03 is merged.
