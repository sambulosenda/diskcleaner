#!/usr/bin/env python3
"""Protected files check for Roomy (task T12).

Fails (exit 1) if a pull request changes a protected path and does not
carry the `sam-approved` label.

Protected paths are:
* every path listed under "## Files you must NEVER edit" in CLAUDE.md, read
  from the base branch so a PR cannot shorten the list it is checked by, and
* the extra paths in `ci/protected-extra.txt` (the CI itself), also read
  from the base branch.

A pattern ending in `/**` protects everything under that folder. Any other
pattern is an exact path. Matching ignores case. A renamed file counts as changing both its old and
new path.

Usage:
  python3 ci/protected_files.py <base_checkout> <changed_files.txt> <labels.json>

`changed_files.txt` holds one path per line. `labels.json` is the PR's
`labels` array from the GitHub event.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

APPROVAL_LABEL = "sam-approved"
SECTION_HEADING = "## Files you must NEVER edit"
EXTRA_FILE = "ci/protected-extra.txt"
LIST_ITEM = re.compile(r"^\s*-\s+`([^`]+)`")


def patterns_from_claude_md(text: str) -> list[str]:
    """Return the backticked paths listed under the never-edit heading.

    Raises SystemExit if the heading or its list is missing, so a reworded
    CLAUDE.md fails closed instead of protecting nothing.
    """
    lines = text.splitlines()
    try:
        start = next(i for i, line in enumerate(lines) if line.strip().startswith(SECTION_HEADING))
    except StopIteration:
        raise SystemExit(f"protected files: heading {SECTION_HEADING!r} not found in CLAUDE.md")
    patterns = []
    for line in lines[start + 1 :]:
        if line.startswith("## "):
            break
        match = LIST_ITEM.match(line)
        if match:
            patterns.append(match.group(1).strip())
    if not patterns:
        raise SystemExit("protected files: no paths listed under the never-edit heading in CLAUDE.md")
    return patterns


def patterns_from_extra(text: str) -> list[str]:
    """Return the non-comment, non-blank lines of the extra list."""
    return [line.strip() for line in text.splitlines() if line.strip() and not line.strip().startswith("#")]


def load_patterns(base: Path) -> list[str]:
    """All protected patterns, from the base branch checkout at `base`."""
    patterns = patterns_from_claude_md((base / "CLAUDE.md").read_text(encoding="utf-8"))
    extra = base / EXTRA_FILE
    if extra.is_file():
        patterns += patterns_from_extra(extra.read_text(encoding="utf-8"))
    return patterns


def is_protected(path: str, pattern: str) -> bool:
    """True if `path` falls under `pattern` (`dir/**` or an exact path).

    Matching ignores case, because macOS file systems usually do: a PR
    adding `claude.md` would overwrite `CLAUDE.md` on a Mac checkout.
    """
    path = path.strip().lstrip("/").lower()
    pattern = pattern.strip().lstrip("/").lower()
    if pattern.endswith("/**"):
        folder = pattern[: -len("**")]
        return path.startswith(folder) or path == folder.rstrip("/")
    return path == pattern


def protected_changes(changed: list[str], patterns: list[str]) -> list[tuple[str, str]]:
    """Each (changed path, pattern) pair where the path is protected."""
    hits = []
    for path in changed:
        for pattern in patterns:
            if is_protected(path, pattern):
                hits.append((path, pattern))
                break
    return hits


def main(argv: list[str]) -> int:
    if len(argv) != 4:
        print(__doc__)
        return 2
    base, changed_file, labels_file = Path(argv[1]), Path(argv[2]), Path(argv[3])
    patterns = load_patterns(base)
    changed = [line for line in changed_file.read_text(encoding="utf-8").splitlines() if line.strip()]
    labels = {label.get("name") for label in json.loads(labels_file.read_text(encoding="utf-8"))}

    hits = protected_changes(changed, patterns)
    if not hits:
        print(f"Protected files check passed: none of {len(changed)} changed path(s) is protected.")
        return 0
    print(f"This PR changes {len(hits)} protected path(s):")
    for path, pattern in hits:
        print(f"  {path}   (protected by `{pattern}`)")
    if APPROVAL_LABEL in labels:
        print(f"\nThe PR has the `{APPROVAL_LABEL}` label, so the check passes.")
        return 0
    print(
        f"\nThese files are owned by Sam (CLAUDE.md, 'Files you must NEVER edit'). "
        f"The check passes only once Sam adds the `{APPROVAL_LABEL}` label by hand."
    )
    return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
