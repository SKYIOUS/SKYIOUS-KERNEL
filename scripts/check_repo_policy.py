#!/usr/bin/env python3
"""Mechanical enforcement of AGENTS.md rules that no compiler covers.

Whole-repo gates are rules the tree already satisfies. Ratcheted rules
(unsafe/SAFETY, unwrap, blanket dead_code) pre-date this gate, so they are
enforced on added lines only via --diff-base. The full debt inventory is
printed with --inventory but does not fail the run.

Exit codes: 0 clean, 1 policy violation, 2 infrastructure error.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

TEXT_SUFFIXES = {
    ".rs", ".toml", ".md", ".py", ".sh", ".ps1", ".yml", ".yaml",
    ".json", ".ld", ".txt", ".cfg", ".conf", ".service", ".props",
}
TEXT_NAMES = {".gitignore", ".gitattributes", "CODEOWNERS", "Makefile"}

# Third-party source vendored into the tree; AGENTS.md rules govern our code,
# not upstream's.
SKIP_PREFIXES = ("builder/vendor/",)
NO_STD_PREFIXES = ("kernel/", "crates/")

# Pre-existing debt this gate must not legalise for new code. Each entry is a
# path whose count is already over the AGENTS.md limit; new files get no pass.
LONG_FILE_ALLOW = {
    "kernel/src/syscalls/dispatch.rs",
    "kernel/src/task/process.rs",
    "kernel/src/syscalls/process_lifecycle.rs",
    "kernel/src/syscalls/fs_stat.rs",
    "kernel/src/iommu.rs",
}
LARGE_FILE_ALLOW = {"kernel/initrd.tar"}  # boot-test fixture, not build output

# Explicit "this architecture is not implemented" bodies. New placeholders
# anywhere else are rejected.
UNIMPLEMENTED_ALLOW = {
    "kernel/src/arch/arch_riscv64.rs",
    "kernel/src/hal/riscv64.rs",
}

TEST_PATH_RE = re.compile(r"(^|/)(tests?)(/|\.rs$)")
UNSAFE_RE = re.compile(r"\bunsafe\s*(?:\{|fn\b|impl\b|trait\b|extern\b)")
SAFETY_RE = re.compile(r"SAFETY:|#\s*Safety")
STD_RE = re.compile(r"\bstd::")
UNWRAP_RE = re.compile(r"\.unwrap\(\)")
TODO_RE = re.compile(r"\b(TODO|FIXME|XXX|HACK)\b")
PLACEHOLDER_RE = re.compile(r"\b(?:todo|unimplemented)!\s*\(")
IGNORE_RE = re.compile(r"#\[ignore\b")
DEAD_CODE_RE = re.compile(r"allow\(dead_code\)")
CONFLICT_RE = re.compile(r"^(<<<<<<< |>>>>>>> |=======$)")

violations: list[str] = []
inventory: dict[str, list[str]] = {}


def fail(kind: str, path: str, line: int | None, message: str) -> None:
    where = f"{path}:{line}" if line else path
    violations.append(f"[{kind}] {where}: {message}")


def tracked_files() -> list[str]:
    try:
        out = subprocess.check_output(
            ["git", "ls-files", "-z"], cwd=ROOT, text=True,
            encoding="utf-8", errors="strict",
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        print(f"INFRASTRUCTURE_ERROR: git ls-files failed: {exc}", file=sys.stderr)
        sys.exit(2)
    return sorted(p for p in out.split("\0") if p)


def is_text(name: str) -> bool:
    return Path(name).suffix in TEXT_SUFFIXES or Path(name).name in TEXT_NAMES


def read_text(rel: str) -> str:
    return (ROOT / rel).read_text(encoding="utf-8")


def line_count(text: str) -> int:
    return text.count("\n") + (1 if text and not text.endswith("\n") else 0)


def cfg_test_ranges(text: str) -> list[tuple[int, int]]:
    """Character ranges of #[cfg(test)] modules, found by brace matching."""
    ranges: list[tuple[int, int]] = []
    for match in re.finditer(r"#\[cfg\(test\)\]", text):
        brace = text.find("{", match.end())
        if brace < 0:
            continue
        depth = 0
        for index in range(brace, len(text)):
            if text[index] == "{":
                depth += 1
            elif text[index] == "}":
                depth -= 1
                if depth == 0:
                    ranges.append((match.start(), index))
                    break
    return ranges


def line_is_test(path: str, text: str, line: int, ranges: list[tuple[int, int]]) -> bool:
    if TEST_PATH_RE.search(path):
        return True
    offset = sum(len(part) for part in text.splitlines(keepends=True)[: line - 1])
    return any(start <= offset <= end for start, end in ranges)


def code_part(line: str) -> str:
    stripped = line.lstrip()
    if stripped.startswith(("//", "/*", "*", "*/")):
        return ""
    return line.split("//", 1)[0]


def in_no_std(rel: str) -> bool:
    return rel.startswith(NO_STD_PREFIXES) and Path(rel).name != "build.rs"


def scan_rust(rel: str, text: str) -> None:
    ranges = cfg_test_ranges(text)
    lines = text.splitlines()
    for number, line in enumerate(lines, 1):
        code = code_part(line)
        if STD_RE.search(code) and in_no_std(rel) and not line_is_test(rel, text, number, ranges):
            fail("no-std", rel, number, "std:: is not available in no_std code")
        if TODO_RE.search(line):
            fail("no-placeholders", rel, number, "TODO/FIXME/XXX/HACK marker")
        if PLACEHOLDER_RE.search(line) and rel not in UNIMPLEMENTED_ALLOW:
            fail("no-placeholders", rel, number, "todo!()/unimplemented!()")
        if IGNORE_RE.search(code):
            fail("no-disabled-tests", rel, number, "#[ignore] disables a test")
        if CONFLICT_RE.match(line):
            fail("no-conflict-markers", rel, number, "merge-conflict marker")
        if UNSAFE_RE.search(code):
            window = lines[max(0, number - 11): number + 3]
            if not any(SAFETY_RE.search(item) for item in window):
                inventory.setdefault("unsafe-without-safety", []).append(f"{rel}:{number}")
        if UNWRAP_RE.search(code) and not line_is_test(rel, text, number, ranges):
            inventory.setdefault("unwrap", []).append(f"{rel}:{number}")
        if DEAD_CODE_RE.search(code):
            inventory.setdefault("dead-code-allow", []).append(f"{rel}:{number}")


def check_markdown_links(rel: str, text: str) -> None:
    in_fence = False
    links: list[tuple[int, str]] = []
    for number, raw in enumerate(text.splitlines(), 1):
        if raw.lstrip().startswith(("```", "~~~")):
            in_fence = not in_fence
            continue
        if in_fence:
            continue
        line = re.sub(r"`[^`\n]*`", "", raw)
        for match in re.finditer(r"!?\[[^\]]*\]\(([^)\s]+)(?:\s+[\"'][^\"']*[\"'])?\)", line):
            links.append((number, match.group(1)))
        definition = re.match(r"^\s*\[[^\]]+\]:\s*(\S+)", line)
        if definition:
            links.append((number, definition.group(1)))
    for number, target in links:
        value = target.strip("<>")
        if not value or value.startswith(("#", "//")) or re.match(r"^[a-zA-Z][a-zA-Z0-9+.-]*:", value):
            continue
        if any(char in value for char in "*{}<>"):
            continue
        path = value.split("#", 1)[0].split("?", 1)[0]
        if not path:
            continue
        from urllib.parse import unquote
        path = unquote(path)
        resolved = (ROOT / rel).parent / path if not path.startswith("/") else ROOT / path.lstrip("/")
        resolved = resolved.resolve()
        if not resolved.exists():
            fail("docs-links", rel, number, f"broken relative link: {value}")


def check_agents_paths() -> None:
    agents = ROOT / "AGENTS.md"
    if not agents.is_file():
        fail("docs-map", "AGENTS.md", None, "missing operating contract")
        return
    text = agents.read_text(encoding="utf-8")
    for section, end in (("## Docs map", "## Non-negotiables"), ("## Architecture summary", "## Operating protocol")):
        start = text.find(section)
        stop = text.find(end, start + 1) if start >= 0 else -1
        if start < 0 or stop < 0:
            fail("docs-map", "AGENTS.md", None, f"missing section: {section}")
            continue
        body = text[start:stop]
        for token in re.findall(r"`([^`]+)`", body):
            if not (token.endswith(".md") or "/" in token):
                continue
            if any(char in token for char in "*{}<>") or token.startswith(("http", "-")):
                continue
            if not (ROOT / token).exists():
                fail("docs-map", "AGENTS.md", None, f"documented path does not exist: {token}")


def check_adr_numbers() -> None:
    seen: dict[str, str] = {}
    for path in sorted((ROOT / "docs" / "decisions").glob("ADR-*.md")):
        match = re.match(r"ADR-(\d+)", path.name)
        if not match:
            continue
        number = match.group(1)
        if number in seen:
            fail("adr", str(path.relative_to(ROOT)), None, f"duplicate ADR-{number} (also {seen[number]})")
        seen[number] = path.name


def check_toml_json(rel: str, text: str) -> None:
    try:
        if rel.endswith(".toml"):
            tomllib.loads(text)
        elif rel.endswith(".json"):
            json.loads(text)
    except (tomllib.TOMLDecodeError, json.JSONDecodeError) as exc:
        fail("parse", rel, None, str(exc).splitlines()[0])


def check_python(rel: str, text: str) -> None:
    try:
        compile(text, rel, "exec")
    except SyntaxError as exc:
        fail("parse", rel, exc.lineno, exc.msg)


def check_workflows() -> None:
    for path in sorted((ROOT / ".github" / "workflows").glob("*.y*ml")):
        rel = str(path.relative_to(ROOT))
        text = path.read_text(encoding="utf-8")
        if not re.search(r"(?m)^permissions:", text):
            fail("workflow-policy", rel, None, "missing top-level permissions: block")
        for number, raw in enumerate(text.splitlines(), 1):
            line = raw.split("#", 1)[0]
            if "pull_request_target" in line:
                fail("workflow-policy", rel, number, "pull_request_target is forbidden")
            if re.search(r"continue-on-error:\s*true", line):
                fail("workflow-policy", rel, number, "continue-on-error: true swallows a failing gate")
            if re.search(r"\|\|\s*true\b", line):
                fail("workflow-policy", rel, number, "|| true swallows a failing gate")
            if re.search(r"uses:\s*\S+@(main|master|latest)\b", line):
                fail("workflow-policy", rel, number, "unpinned action ref")
        jobs: list[tuple[str, int]] = []
        in_jobs = False
        for number, line in enumerate(text.splitlines(), 1):
            if re.match(r"^jobs:\s*$", line):
                in_jobs = True
                continue
            if in_jobs:
                if re.match(r"^\S", line):
                    break
                job = re.match(r"^  ([A-Za-z0-9_-]+):\s*$", line)
                if job:
                    jobs.append((job.group(1), number))
        for index, (job, number) in enumerate(jobs):
            stop = jobs[index + 1][1] if index + 1 < len(jobs) else len(text.splitlines()) + 1
            block = "\n".join(text.splitlines()[number:stop - 1])
            if not re.search(r"(?m)^    timeout-minutes:", block):
                fail("workflow-policy", rel, number, f"job '{job}' has no timeout-minutes")


def check_diff(base: str) -> None:
    # ponytail: added lines only; deleting a SAFETY comment beside an
    # unchanged unsafe block is not caught. Full AST scan if that bypass
    # shows up in practice.
    try:
        subprocess.run(
            ["git", "rev-parse", "--verify", base], cwd=ROOT, check=True,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
    except (OSError, subprocess.CalledProcessError):
        print(f"INFRASTRUCTURE_ERROR: diff base not found: {base}", file=sys.stderr)
        sys.exit(2)
    try:
        raw = subprocess.check_output(
            ["git", "diff", "--unified=0", "--no-color", "--no-ext-diff", f"{base}...HEAD", "--", "*.rs"],
            cwd=ROOT, text=True, encoding="utf-8", errors="replace",
        )
    except (OSError, subprocess.CalledProcessError) as exc:
        print(f"INFRASTRUCTURE_ERROR: git diff failed: {exc}", file=sys.stderr)
        sys.exit(2)

    current = ""
    new_line = 0
    for line in raw.splitlines():
        if line.startswith("+++ b/"):
            current = line[6:]
            continue
        hunk = re.match(r"^@@ -\d+(?:,\d+)? \+(\d+)(?:,(\d+))? @@", line)
        if hunk:
            new_line = int(hunk.group(1))
            continue
        if not current or not line:
            continue
        if line.startswith("+"):
            content = line[1:]
            check_added_line(current, new_line, content)
            new_line += 1
        elif line.startswith(" "):
            new_line += 1

    added = subprocess.check_output(
        ["git", "diff", "--name-only", "--diff-filter=A", f"{base}...HEAD", "--", "*.rs"],
        cwd=ROOT, text=True, encoding="utf-8", errors="replace",
    )
    for rel in [item.strip() for item in added.splitlines() if item.strip()]:
        full = ROOT / rel
        if not full.is_file():
            continue
        count = line_count(full.read_text(encoding="utf-8"))
        if count > 1000 and rel not in LONG_FILE_ALLOW:
            fail("file-length", rel, None, f"new file has {count} lines (limit 1000)")


def check_added_line(rel: str, number: int, line: str) -> None:
    code = code_part(line)
    if CONFLICT_RE.match(line):
        fail("no-conflict-markers", rel, number, "merge-conflict marker")
        return
    if TODO_RE.search(line):
        fail("no-placeholders", rel, number, "TODO/FIXME/XXX/HACK marker")
    if PLACEHOLDER_RE.search(line) and rel not in UNIMPLEMENTED_ALLOW:
        fail("no-placeholders", rel, number, "todo!()/unimplemented!()")
    if IGNORE_RE.search(code):
        fail("no-disabled-tests", rel, number, "#[ignore] disables a test")
    if DEAD_CODE_RE.search(code):
        fail("no-blanket-allow", rel, number, "new dead_code suppression")
    full = ROOT / rel
    if not full.is_file():
        return
    text = full.read_text(encoding="utf-8")
    ranges = cfg_test_ranges(text)
    if STD_RE.search(code) and in_no_std(rel) and not line_is_test(rel, text, number, ranges):
        fail("no-std", rel, number, "std:: is not available in no_std code")
    if UNWRAP_RE.search(code) and not line_is_test(rel, text, number, ranges):
        fail("no-unwrap", rel, number, "unwrap() outside test code")
    if UNSAFE_RE.search(code):
        window = text.splitlines()[max(0, number - 11): number + 3]
        if not any(SAFETY_RE.search(item) for item in window):
            fail("safety-comment", rel, number, "unsafe without // SAFETY: or # Safety")


def run_inventory() -> None:
    if not inventory:
        return
    print("\nDebt inventory (ratcheted, not failing this run):")
    for kind in sorted(inventory):
        entries = inventory[kind]
        print(f"  {kind}: {len(entries)}")
        for entry in entries:
            print(f"    {entry}")


def self_test() -> int:
    sample = (
        "fn f() {\n"
        "    unsafe { g() }\n"
        "}\n"
        "#[cfg(test)]\n"
        "mod tests {\n"
        "    unsafe { h() }\n"
        "}\n"
    )
    ranges = cfg_test_ranges(sample)
    assert ranges, "cfg(test) range not found"
    assert not line_is_test("kernel/src/x.rs", sample, 2, ranges)
    assert line_is_test("kernel/src/x.rs", sample, 6, ranges)
    assert line_is_test("kernel/src/tests/mod.rs", sample, 2, ranges)
    assert UNSAFE_RE.search("let x = unsafe { f() };")
    assert not UNSAFE_RE.search("// unsafe is a keyword")
    assert SAFETY_RE.search("// SAFETY: caller holds the lock")
    assert CONFLICT_RE.match("<<<<<<< HEAD")
    assert PLACEHOLDER_RE.search("todo!()")
    assert STD_RE.search("std::vec::Vec")
    assert UNWRAP_RE.search("value.unwrap()")
    assert code_part("    // unsafe { f() }") == ""
    assert code_part("    /// unsafe fn f()") == ""
    assert code_part("let x = 1; // unsafe { f() }") == "let x = 1; "
    assert in_no_std("kernel/src/lib.rs")
    assert not in_no_std("kernel/build.rs")
    crlf = "fn f() {}\r\n#[cfg(test)]\r\nmod tests {\r\n    fn t() {}\r\n}\r\n"
    crlf_ranges = cfg_test_ranges(crlf)
    assert not line_is_test("kernel/src/x.rs", crlf, 1, crlf_ranges)
    assert line_is_test("kernel/src/x.rs", crlf, 4, crlf_ranges)
    print("policy self-test passed")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--diff-base", default=None, help="git ref to diff added lines against")
    parser.add_argument("--inventory", action="store_true", help="print all ratcheted debt locations")
    parser.add_argument("--self-test", action="store_true", help="test the policy patterns")
    args = parser.parse_args()
    if args.self_test:
        return self_test()

    files = tracked_files()
    for rel in files:
        if rel.startswith(SKIP_PREFIXES):
            continue
        full = ROOT / rel
        if not full.is_file():
            continue
        if full.stat().st_size > 1024 * 1024 and rel not in LARGE_FILE_ALLOW:
            fail("large-file", rel, None, f"{full.stat().st_size} bytes tracked (limit 1 MiB)")
        if not is_text(rel):
            continue
        try:
            text = read_text(rel)
        except UnicodeDecodeError:
            fail("utf8", rel, None, "not valid UTF-8")
            continue
        count = line_count(text)
        if rel.endswith(".rs") and count > 1000 and rel not in LONG_FILE_ALLOW:
            fail("file-length", rel, None, f"{count} lines (limit 1000)")
        if rel.endswith(".rs"):
            scan_rust(rel, text)
        elif rel.endswith(".md"):
            check_markdown_links(rel, text)
        elif rel.endswith((".toml", ".json")):
            check_toml_json(rel, text)
        elif rel.endswith(".py"):
            check_python(rel, text)

    check_agents_paths()
    check_adr_numbers()
    check_workflows()
    if args.diff_base:
        check_diff(args.diff_base)
    if args.inventory:
        run_inventory()

    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if violations:
        for item in violations:
            print(item)
        print(f"\nPOLICY FAILED: {len(violations)} violation(s)")
        if summary:
            with open(summary, "a", encoding="utf-8") as handle:
                handle.write(f"## Repo policy: {len(violations)} violation(s)\n\n")
                for item in violations[:200]:
                    handle.write(f"- `{item}`\n")
        return 1
    print(f"Repo policy clean ({len(files)} tracked files checked)")
    if summary:
        with open(summary, "a", encoding="utf-8") as handle:
            handle.write("## Repo policy: clean\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
