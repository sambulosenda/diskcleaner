#!/usr/bin/env python3
"""Tests for the T12 CI checks. Run: python3 -m unittest discover -s ci -p 'test_*.py'"""

from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import protected_files  # noqa: E402
import safety_grep  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parent.parent


class FakeRepo:
    """A throwaway git repo with an empty allow-list."""

    def __init__(self, allow: str = "") -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.root = Path(self._tmp.name)
        subprocess.run(["git", "init", "-q", str(self.root)], check=True)
        self.write("ci/allowed-commands-readonly.txt", allow)

    def write(self, rel: str, text: str) -> None:
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
        subprocess.run(["git", "-C", str(self.root), "add", rel], check=True)

    def reasons(self) -> list[str]:
        return [f"{v.path}: {v.reason}" for v in safety_grep.scan(self.root)]

    def close(self) -> None:
        self._tmp.cleanup()


class SafetyGrepTests(unittest.TestCase):
    def setUp(self) -> None:
        self.repo = FakeRepo("core/crates/analysers/src/xcode.rs xcrun\n")

    def tearDown(self) -> None:
        self.repo.close()

    def assertFlags(self, rel: str, code: str) -> None:
        self.repo.write(rel, code)
        self.assertTrue(any(r.startswith(rel) for r in self.repo.reasons()), f"{rel} not flagged:\n{code}")

    def assertClean(self, rel: str, code: str) -> None:
        self.repo.write(rel, code)
        self.assertEqual(self.repo.reasons(), [])

    def test_remove_file_in_scanner_is_flagged(self) -> None:
        self.assertFlags("core/crates/scanner/src/lib.rs", "fn f() { std::fs::remove_file(p).ok(); }")

    def test_every_forbidden_rust_call_is_flagged_outside_executor(self) -> None:
        for code in [
            "std::fs::remove_dir(p)",
            "std::fs::remove_dir_all(p)",
            "std::fs::rename(a, b)",
            "use std::fs::rename; rename(a, b)",
            "libc::unlink(p)",
            "libc::rmdir(p)",
            "manager.trashItem(url)",
            "std::process::Command::new(\"ls\")",
            "use std::process::Command as Run;",
        ]:
            with self.subTest(code=code):
                self.assertFlags("core/crates/planner/src/lib.rs", code)

    def test_test_code_is_not_exempt(self) -> None:
        self.assertFlags("core/crates/scanner/tests/walk.rs", "fs::remove_dir_all(home)")

    def test_source_folder_named_build_is_still_scanned(self) -> None:
        self.assertFlags("core/crates/scanner/src/build/mod.rs", "fs::remove_file(p)")

    def test_executor_may_delete_and_run_commands(self) -> None:
        self.assertClean(
            "core/crates/executor/src/lib.rs",
            'fn f() { std::fs::remove_file(p); std::process::Command::new("xcrun"); }',
        )

    def test_shells_are_refused_even_in_executor(self) -> None:
        for code in ['Command::new("sh")', 'Command::new("/bin/bash")', 'Command::new("/usr/bin/env")']:
            with self.subTest(code=code):
                self.assertFlags("core/crates/executor/src/commands.rs", code)

    def test_shell_command_strings_are_refused_anywhere(self) -> None:
        self.assertFlags("scripts/tool.sh", 'sh -c "rm thing"')
        self.assertFlags(".github/workflows/x.yml", "run: bash -c 'echo hi'")

    def test_allow_listed_analyser_command_passes(self) -> None:
        self.assertClean("core/crates/analysers/src/xcode.rs", 'let c = Command::new("xcrun");')

    def test_analyser_command_not_on_the_list_is_flagged(self) -> None:
        self.assertFlags("core/crates/analysers/src/xcode.rs", 'Command::new("docker")')
        self.assertFlags("core/crates/analysers/src/docker.rs", 'Command::new("xcrun")')

    def test_analyser_command_must_be_a_literal(self) -> None:
        self.assertFlags("core/crates/analysers/src/xcode.rs", "Command::new(program)")

    def test_swift_app_cannot_touch_files_or_start_programs(self) -> None:
        for code in ["fm.trashItem(at: u)", "fm.removeItem(at: u)", "fm.moveItem(at: a, to: b)", "let p = Process()"]:
            with self.subTest(code=code):
                self.assertFlags("app/Roomy/ContentView.swift", code)

    def test_malformed_allow_list_fails(self) -> None:
        self.repo.write("ci/allowed-commands-readonly.txt", "core/crates/scanner/src/lib.rs ls\n")
        with self.assertRaises(SystemExit):
            safety_grep.scan(self.repo.root)


CLAUDE_MD = """# CLAUDE.md
## Files you must NEVER edit
Owned by Sam.
- `core/crates/safety/src/protected.rs`
- `core/crates/safety/tests/**`
- `CLAUDE.md`

## How to work
- `not/protected.rs`
"""


class ProtectedFilesTests(unittest.TestCase):
    def setUp(self) -> None:
        self.patterns = protected_files.patterns_from_claude_md(CLAUDE_MD) + [".github/**", "ci/**"]

    def hits(self, *paths: str) -> list[str]:
        return [p for p, _ in protected_files.protected_changes(list(paths), self.patterns)]

    def test_reads_only_the_never_edit_section(self) -> None:
        self.assertEqual(
            protected_files.patterns_from_claude_md(CLAUDE_MD),
            ["core/crates/safety/src/protected.rs", "core/crates/safety/tests/**", "CLAUDE.md"],
        )

    def test_real_claude_md_lists_the_safety_files(self) -> None:
        patterns = protected_files.load_patterns(REPO_ROOT)
        for expected in ["core/crates/safety/src/protected.rs", "docs/safety/**", ".claude/settings.json", "ci/**"]:
            self.assertIn(expected, patterns)

    def test_protected_paths_are_found(self) -> None:
        self.assertEqual(
            self.hits(
                "core/crates/safety/src/protected.rs",
                "core/crates/safety/tests/common/mod.rs",
                "CLAUDE.md",
                ".github/workflows/ci.yml",
                "ci/safety_grep.py",
            ),
            [
                "core/crates/safety/src/protected.rs",
                "core/crates/safety/tests/common/mod.rs",
                "CLAUDE.md",
                ".github/workflows/ci.yml",
                "ci/safety_grep.py",
            ],
        )

    def test_matching_ignores_case(self) -> None:
        self.assertEqual(self.hits("claude.md", "Core/Crates/Safety/Tests/x.rs"), ["claude.md", "Core/Crates/Safety/Tests/x.rs"])

    def test_ordinary_paths_pass(self) -> None:
        self.assertEqual(
            self.hits("core/crates/safety/src/lib.rs", "core/crates/safety/tests_extra.rs", "not/protected.rs"), []
        )

    def test_missing_section_fails_closed(self) -> None:
        with self.assertRaises(SystemExit):
            protected_files.patterns_from_claude_md("# CLAUDE.md\nnothing here\n")

    def run_main(self, changed: str, labels: str) -> int:
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            (base / "CLAUDE.md").write_text(CLAUDE_MD, encoding="utf-8")
            (base / "changed.txt").write_text(changed, encoding="utf-8")
            (base / "labels.json").write_text(labels, encoding="utf-8")
            return protected_files.main(["x", str(base), str(base / "changed.txt"), str(base / "labels.json")])

    def test_protected_change_without_label_fails(self) -> None:
        self.assertEqual(self.run_main("CLAUDE.md\n", '[{"name": "bug"}]'), 1)

    def test_protected_change_with_label_passes(self) -> None:
        self.assertEqual(self.run_main("CLAUDE.md\n", '[{"name": "sam-approved"}]'), 0)

    def test_unprotected_change_passes(self) -> None:
        self.assertEqual(self.run_main("rules/js.toml\n", "[]"), 0)


if __name__ == "__main__":
    unittest.main()
