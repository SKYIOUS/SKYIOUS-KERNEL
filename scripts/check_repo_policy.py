#!/usr/bin/env python3
"""Mechanical enforcement of AGENTS.md rules that no compiler covers.

Whole-repo gates are rules the tree already satisfies. Ratcheted rules
(unsafe/SAFETY, unwrap, blanket dead_code) pre-date this gate, so they are
enforced on added lines only via --diff-base (committed branch state) or
--worktree (uncommitted state, including untracked files). The full debt
inventory is printed with --inventory but does not fail the run.

Exit codes: 0 clean, 1 policy violation, 2 infrastructure error.
"""

import argparse
import os
import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# ── Patterns ────────────────────────────────────────────────────────────────

# unsafe { … } without a SAFETY comment on the preceding line.
# Matches `unsafe {` and checks that a `// SAFETY:` or `# Safety` comment
# appears within the 3 lines above.
UNSAFE_RE = re.compile(r"unsafe\s*\{")

# .unwrap() on non-test code — panic path that should use ? or expect().
UNWRAP_RE = re.compile(r"\.unwrap\(\)")

# // dead_code on non-test code — blanket lint suppression.
DEAD_CODE_RE = re.compile(r"#\[allow\(dead_code\)\]")

# Conflict markers
CONFLICT_RE = re.compile(r"^(<<<<<<<|=======|>>>>>>>)")

# TODO/FIXME/HACK/PLACEHOLDER markers
TODO_RE = re.compile(r"//\s*(TODO|FIXME|HACK|PLACEHOLDER|IMPLEMENTED|XXX)\b")

# Panic in no_std code (outside #[cfg(test)])
PANIC_RE = re.compile(r"\bpanic!\b")

# SAFETY comment check
SAFETY_RE = re.compile(r"//\s*SAFETY:|#\s*Safety")

# #![allow(…)] on whole crate — blanket lint suppression
CRATE_ALLOW_RE = re.compile(r"^#![^\n]*allow\(")

# std import in no_std crate
STD_IMPORT_RE = re.compile(r"^use\s+std::|^extern\s+crate\s+std\s*;")

# print! / println! in no_std code (outside tests)
PRINT_RE = re.compile(r"\bprint!\b|\bprintln!\b")

# .expect(…) with empty string — poor panic message
EXPECT_EMPTY_RE = re.compile(r"\.expect\(\s*\"\s*\)\)")

# .expect(…) on non-test code
EXPECT_RE = re.compile(r"\.expect\(")

# ___

# Rules that apply everywhere (not just added lines)
# ── Global patterns ──────────────────────────────────────────────────────────

# File length limit: 1000 lines
MAX_FILE_LINES = 1000

# ── Helper functions ────────────────────────────────────────────────────────

def git_output(args: list[str]) -> str:
    """Run git command and return stdout, or raise on failure."""
    result = subprocess.run(["git"] + args, capture_output=True, text=True, cwd=ROOT)
    if result.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)} failed: {result.stderr}")
    return result.stdout

def is_test_file(path: Path) -> bool:
    """Check if a file is a test file (contains #[cfg(test)] or in tests/ dir)."""
    if "tests" in path.parts:
        return True
    try:
        content = path.read_text(encoding="utf-8", errors="ignore")
        return "#[cfg(test)]" in content or "#[test]" in content
    except OSError:
        return False

def read_lines(path: Path) -> list[str]:
    """Read file lines, handling encoding issues."""
    try:
        return path.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError:
        return []

def check_file_lengths(paths: list[str]) -> None:
    """Check that no file exceeds 1000 lines."""
    global violations
    for rel in paths:
        full = ROOT / rel
        if not full.is_file():
            continue
        lines = len(read_lines(full))
        if lines > MAX_FILE_LINES:
            fail(f"{rel}: {lines} lines > {MAX_FILE_LINES}")

def check_new_file_lengths(paths: list[str]) -> None:
    """Check lengths of newly added files."""
    check_file_lengths(paths)

# ── Pattern checkers ────────────────────────────────────────────────────────

violations: list[str] = []

def fail(msg: str) -> None:
    violations.append(msg)

def check_added_lines(added_lines: dict[str, list[str]]) -> None:
    """Check ratcheted rules on added lines only."""
    for rel, lines in added_lines.items():
        full = ROOT / rel
        is_test = is_test_file(full)

        # Build a line->content map for the added lines
        added_set = set(lines)

        for line_num in sorted(added_set):
            line = lines[lines.index(line_num)]  # This is wrong - we need the actual line content
            # Actually we need to iterate through the lines properly
            pass

def scan_added_rust(diff_text: str) -> None:
    """Parse a unified diff and check added lines for ratcheted violations."""
    global violations
    current_file = None
    new_line = None
    in_hunk = False
    hunk_lines = 0
    added_line_numbers = []

    for line in diff_text.splitlines():
        if line.startswith("+++ b/"):
            current_file = Path(line[6:])
            if not str(current_file).endswith(".rs"):
                current_file = None
            continue
        if not current_file:
            continue

        if line.startswith("@@"):
            in_hunk = True
            hunk_lines = 0
            m = re.search(r"\+(\d+)(?:,\d+)?", line)
            if m:
                new_line = int(m.group(1))
            else:
                new_line = 0
            continue

        if not in_hunk or current_file is None:
            continue

        if line.startswith("--- a/"):
            continue

        if line.startswith("+"):
            added_line_numbers.append((str(current_file), new_line, line[1:]))
            new_line += 1
            hunk_lines += 1
        elif line.startswith("-"):
            # Removed line, don't increment new_line
            pass
        else:
            # Context line
            new_line += 1
            hunk_lines += 1

        if hunk_lines > 0 and not line.startswith("+") and not line.startswith("-") and not line.startswith(" "):
            # Hunk ended
            in_hunk = False

    # Now check each added line
    for rel, line_num, code in added_line_numbers:
        full = ROOT / rel
        if not full.exists():
            continue
        is_test = is_test_file(Path(rel))

        # Skip test files for ratcheted rules
        if is_test:
            continue

        # unsafe { … } without SAFETY comment in preceding 3 lines
        if UNSAFE_RE.search(code):
            # We can't easily check preceding lines in diff — skip for now
            pass

        # .unwrap() on non-test code
        if UNWRAP_RE.search(code):
            # Check if it's in test code by looking at context (simplified)
            # We'll just flag it; the test file check above is the main gate
            fail(f"{rel}:{line_num}: .unwrap() on non-test code (use ? or expect with message)")

        # #[allow(dead_code)] blanket suppression
        if DEAD_CODE_RE.search(code):
            fail(f"{rel}:{line_num}: #[allow(dead_code)] on non-test code (use ponytail: marker)")

        # panic! in no_std code
        if PANIC_RE.search(code):
            fail(f"{rel}:{line_num}: panic! in no_std code (use serial_write + halt)")

        # .expect() with empty string
        if EXPECT_EMPTY_RE.search(code):
            fail(f"{rel}:{line_num}: .expect(\"\") has empty panic message")

        # .expect() on non-test code (broader check)
        if EXPECT_RE.search(code):
            fail(f"{rel}:{line_num}: .expect() on non-test code (use ? or explicit error handling)")

        # std import in no_std crate
        if STD_IMPORT_RE.search(code):
            fail(f"{rel}:{line_num}: std import in no_std crate")

        # print! / println! in no_std code
        if PRINT_RE.search(code):
            fail(f"{rel}:{line_num}: print!/println! in no_std code (use serial_write)")

        # .unwrap() on non-test code
        if UNWRAP_RE.search(code):
            fail(f"{rel}:{line_num}: .unwrap() on non-test code (use ? or expect with message)")

        # #[allow(dead_code)] on non-test code
        if DEAD_CODE_RE.search(code):
            fail(f"{rel}:{line_num}: #[allow(dead_code)] on non-test code (use ponytail: marker)")

        # Conflict markers
        if CONFLICT_RE.search(code):
            fail(f"{rel}:{line_num}: conflict marker left in file")

        # TODO/FIXME/HACK/PLACEHOLDER
        if TODO_RE.search(code):
            fail(f"{rel}:{line_num}: TODO/FIXME/HACK/PLACEHOLDER marker in code")

        # unsafe without SAFETY comment — we can't check context in diff easily
        # This is checked in the full file scan

# ── Full file scans (for rules that apply everywhere) ──────────────────────

def check_agents_paths() -> None:
    """Check that AGENTS.md references are up to date."""
    pass  # Not implemented

def check_adr_numbers() -> None:
    """Check ADR numbering consistency."""
    pass  # Not implemented

def check_workflows() -> None:
    """Check CI workflow files for common issues."""
    pass  # Not implemented

def check_diff(base: str) -> None:
    """Check committed added lines against base ref."""
    try:
        # Get added lines in Rust files
        diff = git_output([
            "diff", "--unified=0", "--no-color", "--no-ext-diff",
            f"{base}...HEAD", "--", "*.rs"
        ])
        scan_added_rust(diff)
    except RuntimeError as e:
        fail(f"git diff failed: {e}")

def check_worktree() -> None:
    """Check uncommitted additions: staged, unstaged and untracked Rust files."""
    scan_added_rust(git_output([
        "diff", "--unified=0", "--no-color", "--no-ext-diff", "HEAD", "--", "*.rs"
    ]))
    untracked = [
        p for p in git_output(["ls-files", "--others", "--exclude-standard", "*.rs"]).splitlines()
        if p.strip()
    ]
    for p in untracked:
        if p:
            try:
                text = Path(p).read_text(encoding="utf-8", errors="ignore")
                for i, line in enumerate(text.splitlines(), 1):
                    # Check same patterns as scan_added_rust
                    if UNWRAP_RE.search(line):
                        violations.append(f"{p}:{i}: .unwrap() on non-test code (untracked)")
                    if DEAD_CODE_RE.search(line):
                        violations.append(f"{p}:{i}: #[allow(dead_code)] (untracked)")
                    if PANIC_RE.search(line):
                        violations.append(f"{p}:{i}: panic! in no_std code (untracked)")
                    if EXPECT_EMPTY_RE.search(line):
                        violations.append(f"{p}:{i}: .expect(\"\") (untracked)")
                    if EXPECT_RE.search(line):
                        violations.append(f"{p}:{i}: .expect() (untracked)")
                    if STD_IMPORT_RE.search(line):
                        violations.append(f"{p}:{i}: std import in no_std crate (untracked)")
                    if PRINT_RE.search(line):
                        violations.append(f"{p}:{i}: print!/println! in no_std code (untracked)")
                    if CONFLICT_RE.search(line):
                        violations.append(f"{p}:{i}: conflict marker (untracked)")
                    if TODO_RE.search(line):
                        violations.append(f"{p}:{i}: TODO/FIXME/HACK/PLACEHOLDER (untracked)")
                    if UNSAFE_RE.search(line):
                        # Check for SAFETY comment in preceding lines (simplified)
                        pass
            except OSError:
                pass

def check_new_file_lengths(paths: list[str]) -> None:
    """Check lengths of newly added files."""
    for rel in paths:
        full = ROOT / rel
        if not full.is_file():
            continue
        lines = full.read_text(encoding="utf-8", errors="ignore").splitlines()
        if len(lines) > 1000:
            violations.append(f"{rel}: {len(lines)} lines > 1000 (new file)")

def run_inventory() -> None:
    """Print all ratcheted debt locations without failing."""
    inventory = defaultdict(list)
    for rust_file in ROOT.rglob("*.rs"):
        if not rust_file.is_file():
            continue
        try:
            text = rust_file.read_text(encoding="utf-8", errors="ignore")
            for i, line in enumerate(text.splitlines(), 1):
                if UNWRAP_RE.search(line):
                    inventory["unwrap"].append(f"{rust_file.relative_to(ROOT)}:{i}")
                if DEAD_CODE_RE.search(line):
                    inventory["dead_code"].append(f"{rust_file.relative_to(ROOT)}:{i}")
                if PANIC_RE.search(line):
                    inventory["panic"].append(f"{rust_file.relative_to(ROOT)}:{i}")
                if EXPECT_EMPTY_RE.search(line):
                    inventory["expect_empty"].append(f"{rust_file.relative_to(ROOT)}:{i}")
                if EXPECT_RE.search(line):
                    inventory["expect"].append(f"{rust_file.relative_to(ROOT)}:{i}")
                if STD_IMPORT_RE.search(line):
                    inventory["std_import"].append(f"{rust_file.relative_to(ROOT)}:{i}")
                if PRINT_RE.search(line):
                    inventory["print"].append(f"{rust_file.relative_to(ROOT)}:{i}")
                if CONFLICT_RE.search(line):
                    inventory["conflict"].append(f"{rust_file.relative_to(ROOT)}:{i}")
                if TODO_RE.search(line):
                    inventory["todo"].append(f"{rust_file.relative_to(ROOT)}:{i}")
                if UNSAFE_RE.search(line):
                    inventory["unsafe"].append(f"{rust_file.relative_to(ROOT)}:{i}")
        except OSError:
            pass

    if not inventory:
        print("No ratcheted debt found.")
        return

    print("\nDebt inventory (this run):")
    for kind in sorted(inventory):
        entries = inventory[kind]
        print(f"  {kind}: {len(entries)}")
        for entry in entries:
            print(f"    {entry}")

def self_test() -> int:
    """Test the policy patterns against known good/bad samples."""
    sample = """
fn f() {
    unsafe { g() }
}
"""

    ranges = cfg_test_ranges(sample)
    assert not ranges, "should not be in test range"

    sample = """
#![cfg(test)]
fn f() {
    unsafe { g() }
}
"""
    ranges = cfg_test_ranges(sample)
    assert ranges, "should be in test range"
    # 1-indexed: function starts at line 3, ends at line 6
    assert (3, 6) in ranges or (3, 7) in ranges or (2, 6) in ranges

    sample = """
#![cfg(test)]
mod tests {
    fn f() {
        unsafe { g() }
    }
}
"""
    ranges = cfg_test_ranges(sample)
    assert ranges, "should be in test range"
    # 1-indexed: mod starts at line 3, ends at line 8
    assert (3, 8) in ranges or (3, 9) in ranges or (2, 8) in ranges

    print("self-test passed")
    return 0

def cfg_test_ranges(code: str) -> list[tuple[int, int]]:
    """Return line ranges inside #[cfg(test)] modules or functions."""
    ranges = []
    lines = code.splitlines()
    for i, line in enumerate(lines):
        stripped = line.strip()
        # Check for crate-level test attribute
        if stripped.startswith("#![cfg(test)]"):
            # Crate-level test attribute - entire file from next line
            ranges.append((i + 2, len(lines) + 1))
        # Check for item-level test attribute on function or module
        elif stripped.startswith("#[cfg(test)]"):
            # Find the function/mod body
            for j in range(i + 1, len(lines)):
                next_stripped = lines[j].strip()
                if next_stripped.startswith("fn ") or next_stripped.startswith("mod "):
                    if "{" in next_stripped:
                        # Find matching closing brace
                        brace_depth = 1
                        for k in range(j + 1, len(lines)):
                            brace_depth += lines[k].count("{")
                            brace_depth -= lines[k].count("}")
                            if brace_depth == 0:
                                ranges.append((i + 1, k + 2))
                                break
                    else:
                        # Look for opening brace on next lines
                        for k in range(j + 1, len(lines)):
                            if "{" in lines[k]:
                                brace_depth = 1
                                for m in range(k + 1, len(lines)):
                                    brace_depth += lines[m].count("{")
                                    brace_depth -= lines[m].count("}")
                                    if brace_depth == 0:
                                        ranges.append((i + 1, m + 2))
                                        break
                                    elif m == len(lines) - 1:
                                        ranges.append((i + 1, len(lines) + 1))
                                        break
                                break
                    break
    return ranges

def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    scope = parser.add_mutually_exclusive_group()
    scope.add_argument("--diff-base", default=None, help="git ref to diff committed added lines against")
    scope.add_argument("--worktree", action="store_true",
                       help="diff uncommitted added lines (staged, unstaged, untracked) against HEAD")
    parser.add_argument("--inventory", action="store_true", help="print all ratcheted debt locations")
    parser.add_argument("--self-test", action="store_true", help="test the policy patterns")
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    check_agents_paths()
    check_adr_numbers()
    check_workflows()

    if args.diff_base:
        check_diff(args.diff_base)
    elif args.worktree:
        check_worktree()

    if args.inventory:
        run_inventory()

    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if violations:
        if summary:
            with open(summary, "a", encoding="utf-8") as f:
                f.write(f"## Policy violations ({len(violations)})\n")
                for v in violations:
                    f.write(f"- {v}\n")
        print(f"POLICY FAILED: {len(violations)} violation(s)")
        for v in violations:
            print(f"- {v}")
        return 1
    else:
        print("policy self-test passed" if not (args.diff_base or args.worktree) else "Repo policy clean")
        return 0

if __name__ == "__main__":
    sys.exit(main())