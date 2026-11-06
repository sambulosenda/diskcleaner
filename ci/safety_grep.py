#!/usr/bin/env python3
"""Safety grep for Roomy (task T12).

Fails (exit 1) if code outside the `executor` crate could delete, move or
trash files, or start other programs, and if anything anywhere builds a
shell command line.

Rules, from CLAUDE.md safety rules 1 and 6 and docs/tasks/T12-ci.md:

* Rust outside `core/crates/executor/` must not contain `remove_file`,
  `remove_dir` (which also covers `remove_dir_all`), `fs::rename`, a bare
  `rename(` call, `unlink`, `rmdir`, `trashItem`, `Command::new` or an
  aliased `Command` import.
* The one exception: `analysers` may call `Command::new("<program>")` when
  the exact file and program are listed in
  `ci/allowed-commands-readonly.txt`. The program must be a string literal.
* Swift in `app/` is UI only, so it must not contain `trashItem`,
  `removeItem`, `moveItem`, `unlink`, `rmdir`, `Process(` or `NSTask`.
  The one exception: a test file in `app/RoomyTests/` may make a call
  listed for that exact file in `ci/allowed-swift-test-calls.txt`.
* Anywhere in the repo (code, scripts, workflows): no `sh -c`, `bash -c`
  or `zsh -c`, and no `Command::new` that starts a shell, not even in the
  executor.

Comments count as matches on purpose. A comment that names a forbidden
call is rare and easy to reword; a parser that skips comments is a place
for a real call to hide.

Usage: python3 ci/safety_grep.py [repo_root]
"""

from __future__ import annotations

import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

EXECUTOR_DIR = "core/crates/executor/"
ANALYSERS_DIR = "core/crates/analysers/"
ALLOW_FILE = "ci/allowed-commands-readonly.txt"
SWIFT_TEST_ALLOW_FILE = "ci/allowed-swift-test-calls.txt"
SWIFT_TEST_DIR = "app/RoomyTests/"

# This CI folder is not scanned: its scripts and tests must spell out the
# patterns they look for. `ci/**` is on the protected list, so changes there
# need the `sam-approved` label.
SKIP_PREFIXES = ("ci/",)

# Files checked for shell command lines.
SHELL_SCAN_SUFFIXES = {".rs", ".swift", ".sh", ".bash", ".zsh", ".py", ".yml", ".yaml", ".toml", ".json"}

RUST_FORBIDDEN = [
    (re.compile(r"remove_file"), "deletes a file"),
    (re.compile(r"remove_dir"), "deletes a folder"),
    (re.compile(r"fs::rename"), "moves a file"),
    (re.compile(r"(?<![\w.:])rename\s*\("), "moves a file (bare rename call)"),
    (re.compile(r"\bunlink"), "deletes a file"),
    (re.compile(r"\brmdir"), "deletes a folder"),
    (re.compile(r"trashItem"), "moves a file to the Trash"),
    (re.compile(r"\bCommand\s+as\s+\w+"), "aliased Command import hides Command::new"),
]

COMMAND_NEW = re.compile(r"Command::new\s*\(")
COMMAND_NEW_LITERAL = re.compile(r'Command::new\s*\(\s*"([^"\\]*)"\s*\)')

SWIFT_FORBIDDEN = [
    (re.compile(r"trashItem"), "moves a file to the Trash"),
    (re.compile(r"removeItem"), "deletes a file"),
    (re.compile(r"\bmoveItem"), "moves a file"),
    (re.compile(r"\bunlink"), "deletes a file"),
    (re.compile(r"\brmdir"), "deletes a folder"),
    (re.compile(r"\bProcess\s*\("), "starts a program"),
    (re.compile(r"\bNSTask\b"), "starts a program"),
]

SHELL_STRING = re.compile(r"\b(?:sh|bash|zsh|dash|fish)\s+-c\b")
SHELL_PROGRAMS = {"sh", "bash", "zsh", "dash", "fish", "env"}


@dataclass(frozen=True)
class Violation:
    """One forbidden match: where it is and why it is not allowed."""

    path: str
    line: int
    text: str
    reason: str

    def __str__(self) -> str:
        return f"{self.path}:{self.line}: {self.reason}\n    {self.text.strip()}"


def load_allow_list(root: Path) -> set[tuple[str, str]]:
    """Read `ci/allowed-commands-readonly.txt` as (file, program) pairs.

    Each non-comment line is `<repo-relative .rs file> <program>`. A line
    with any other shape is an error, so a typo cannot widen the list.
    """
    allowed: set[tuple[str, str]] = set()
    path = root / ALLOW_FILE
    if not path.is_file():
        raise SystemExit(f"safety grep: {ALLOW_FILE} is missing")
    for number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split()
        if len(parts) != 2 or not parts[0].startswith(ANALYSERS_DIR) or not parts[0].endswith(".rs"):
            raise SystemExit(
                f"safety grep: {ALLOW_FILE}:{number}: expected "
                f"'{ANALYSERS_DIR}<file>.rs <program>', got: {raw!r}"
            )
        allowed.add((parts[0], parts[1]))
    return allowed


def load_swift_test_allow_list(root: Path) -> set[tuple[str, str]]:
    """Read `ci/allowed-swift-test-calls.txt` as (file, call) pairs.

    Each non-comment line is `<file under app/RoomyTests/> <call>`, where
    `<call>` is exactly the text the check matched (for example `Process(`).
    Only test files can be listed, so app code can never be exempted.
    """
    allowed: set[tuple[str, str]] = set()
    path = root / SWIFT_TEST_ALLOW_FILE
    if not path.is_file():
        return allowed
    for number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        parts = line.split()
        if len(parts) != 2 or not parts[0].startswith(SWIFT_TEST_DIR) or not parts[0].endswith(".swift"):
            raise SystemExit(
                f"safety grep: {SWIFT_TEST_ALLOW_FILE}:{number}: expected "
                f"'{SWIFT_TEST_DIR}<file>.swift <call>', got: {raw!r}"
            )
        allowed.add((parts[0], parts[1]))
    return allowed


def iter_files(root: Path):
    """Yield (repo-relative path, absolute path) for every file git tracks.

    Using git's own list means build output is never scanned and no source
    folder can be skipped by its name. Untracked files cannot reach a PR.
    """
    result = subprocess.run(
        ["git", "-C", str(root), "ls-files", "-z", "--cached"],
        check=True,
        capture_output=True,
    )
    for rel in sorted(result.stdout.decode("utf-8").split("\0")):
        if not rel or rel.startswith(SKIP_PREFIXES):
            continue
        path = root / rel
        if path.is_file() and not path.is_symlink():
            yield rel, path


def line_of(text: str, offset: int) -> tuple[int, str]:
    """Return the 1-based line number and the line text at `offset`."""
    number = text.count("\n", 0, offset) + 1
    start = text.rfind("\n", 0, offset) + 1
    end = text.find("\n", offset)
    return number, text[start : end if end != -1 else len(text)]


def scan_patterns(rel: str, text: str, patterns, allowed_calls: frozenset[str] = frozenset()) -> list[Violation]:
    """Report every match of every (regex, reason) pair in `text`.

    A match whose exact text is in `allowed_calls` is not reported.
    """
    found = []
    for regex, reason in patterns:
        for match in regex.finditer(text):
            if match.group(0) in allowed_calls:
                continue
            number, line = line_of(text, match.start())
            found.append(Violation(rel, number, line, f"`{match.group(0)}` {reason}"))
    return found


def scan_commands(rel: str, text: str, allowed: set[tuple[str, str]]) -> list[Violation]:
    """Check every `Command::new` in a Rust file.

    Shells are refused everywhere. Outside the executor, a call is allowed
    only in `analysers` with a literal program on the allow-list.
    """
    found = []
    in_executor = rel.startswith(EXECUTOR_DIR)
    for match in COMMAND_NEW.finditer(text):
        number, line = line_of(text, match.start())
        literal = COMMAND_NEW_LITERAL.match(text, match.start())
        program = literal.group(1) if literal else None
        if program is not None and Path(program).name in SHELL_PROGRAMS:
            found.append(Violation(rel, number, line, f"`Command::new(\"{program}\")` starts a shell"))
            continue
        if in_executor:
            continue
        if not rel.startswith(ANALYSERS_DIR):
            found.append(Violation(rel, number, line, "`Command::new` is allowed only in the executor"))
        elif program is None:
            found.append(Violation(rel, number, line, "`Command::new` in analysers must name its program as a string literal"))
        elif (rel, program) not in allowed:
            found.append(
                Violation(rel, number, line, f"`Command::new(\"{program}\")` is not listed for this file in {ALLOW_FILE}")
            )
    return found


def scan(root: Path) -> list[Violation]:
    """Run every check over the repo at `root`."""
    allowed = load_allow_list(root)
    swift_test_allowed = load_swift_test_allow_list(root)
    found: list[Violation] = []
    for rel, path in iter_files(root):
        suffix = path.suffix
        if suffix not in SHELL_SCAN_SUFFIXES:
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        found += scan_patterns(rel, text, [(SHELL_STRING, "builds a shell command line")])
        if suffix == ".rs":
            found += scan_commands(rel, text, allowed)
            if not rel.startswith(EXECUTOR_DIR):
                found += scan_patterns(rel, text, RUST_FORBIDDEN)
        elif suffix == ".swift" and rel.startswith("app/"):
            calls = frozenset(call for (file, call) in swift_test_allowed if file == rel)
            found += scan_patterns(rel, text, SWIFT_FORBIDDEN, calls)
    return found


def main(argv: list[str]) -> int:
    root = Path(argv[1] if len(argv) > 1 else Path(__file__).resolve().parent.parent).resolve()
    violations = scan(root)
    if violations:
        print(f"Safety grep found {len(violations)} problem(s):\n")
        for violation in violations:
            print(violation)
        print(
            "\nOnly the executor crate may delete, move or trash files or run commands "
            "(CLAUDE.md safety rules 1 and 6). See ci/safety_grep.py."
        )
        return 1
    print("Safety grep passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
