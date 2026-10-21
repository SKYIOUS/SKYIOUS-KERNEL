#!/usr/bin/env python3
"""Generate the CI report site that ci-pages.yml publishes to GitHub Pages.

Reads recent runs, jobs, artifacts and logs of the watched workflows through
the `gh` CLI and renders a static site: site/index.html redirects to the
report, site/ci/index.html holds run history, per-job results, artifact
indexes, downloaded logs and TAP summaries. Only data from runs on the
default branch is published (ci-pages.yml triggers on master only).
"""

import argparse
import html
import json
import os
import re
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

WORKFLOWS = (
    ("ci", "ci.yml"),
    ("quality", "quality.yml"),
    ("nightly", "nightly.yml"),
    ("t00-demo", "t00-demo.yml"),
)
RUN_LIMIT = 10
MAX_EMBED_BYTES = 20 * 1024 * 1024
BAD_CONCLUSIONS = {"failure", "timed_out", "cancelled", "startup_failure", "action_required"}
TAP_RE = re.compile(r"#\s*(\d+)/(\d+) passed(?:,\s*(\d+) failed)?")


def gh(*args):
    # ponytail: whole-output capture; logs over MAX_EMBED_BYTES are dropped
    # after the read instead of streamed to disk. Fine for kernel-CI log
    # sizes (KBs); switch to streaming if multi-100MB logs ever appear.
    proc = subprocess.run(
        ["gh", *args], capture_output=True, text=True, encoding="utf-8", errors="replace"
    )
    if proc.returncode != 0:
        raise RuntimeError(f"gh {' '.join(args)}: {proc.stderr.strip()}")
    return proc.stdout


def gh_json(*args):
    return json.loads(gh(*args))


def esc(value):
    return html.escape(str(value), quote=True)


def repo_slug():
    if os.environ.get("GITHUB_REPOSITORY"):
        return os.environ["GITHUB_REPOSITORY"]
    return gh("repo", "view", "--json", "nameWithOwner", "--jq", ".nameWithOwner").strip()


def fetch_runs(repo, workflow_file):
    return gh_json(
        "run", "list", "--repo", repo, "--workflow", workflow_file,
        "--limit", str(RUN_LIMIT),
        "--json",
        "databaseId,displayTitle,event,headBranch,status,conclusion,createdAt,url,number",
    )


def fetch_jobs(repo, run_id):
    try:
        return gh_json("run", "view", str(run_id), "--repo", repo, "--json", "jobs").get("jobs", [])
    except RuntimeError:
        return []


def fetch_artifacts(repo, run_id):
    try:
        payload = gh_json("api", f"repos/{repo}/actions/runs/{run_id}/artifacts")
        return payload.get("artifacts", [])
    except RuntimeError:
        return []


def download_artifact(repo, run_id, name, dest):
    try:
        gh("run", "download", str(run_id), "--repo", repo, "--name", name, "--dir", str(dest))
        return True
    except RuntimeError:
        return False


def save_run_log(repo, run_id, dest):
    try:
        text = gh("run", "view", str(run_id), "--repo", repo, "--log")
    except RuntimeError:
        return None
    if not text.strip() or len(text.encode("utf-8", "replace")) > MAX_EMBED_BYTES:
        return None
    dest.write_text(text, encoding="utf-8", errors="replace")
    return dest


def stamp(value):
    return esc(value[:16].replace("T", " ") if value else "-")


def conclusion_cell(conclusion, status):
    label = conclusion or status or "unknown"
    cls = "good" if conclusion == "success" else ("bad" if conclusion in BAD_CONCLUSIONS else "mute")
    return f'<span class="{cls}">{esc(label)}</span>'


def runs_table(runs):
    rows = [
        "<table><tr><th>Run</th><th>Title</th><th>Event</th><th>Branch</th>"
        "<th>Result</th><th>Created (UTC)</th></tr>"
    ]
    for run in runs:
        rows.append(
            "<tr>"
            f'<td><a href="{esc(run["url"])}">#{esc(run["number"])}</a></td>'
            f"<td>{esc(run.get('displayTitle', ''))}</td>"
            f"<td>{esc(run.get('event', ''))}</td>"
            f"<td>{esc(run.get('headBranch', ''))}</td>"
            f"<td>{conclusion_cell(run.get('conclusion'), run.get('status'))}</td>"
            f"<td>{stamp(run.get('createdAt', ''))}</td>"
            "</tr>"
        )
    rows.append("</table>")
    return "\n".join(rows)


def jobs_html(jobs):
    if not jobs:
        return "<p class='mute'>No job data for this run.</p>"
    table = [
        "<table><tr><th>Job</th><th>Result</th><th>Started (UTC)</th><th>Finished (UTC)</th></tr>"
    ]
    details = []
    for job in jobs:
        table.append(
            "<tr>"
            f"<td>{esc(job.get('name', ''))}</td>"
            f"<td>{conclusion_cell(job.get('conclusion'), job.get('status'))}</td>"
            f"<td>{stamp(job.get('startedAt', ''))}</td>"
            f"<td>{stamp(job.get('completedAt', ''))}</td>"
            "</tr>"
        )
        steps = job.get("steps") or []
        if steps:
            step_rows = "".join(
                f"<tr><td>{esc(step.get('number'))}</td><td>{esc(step.get('name', ''))}</td>"
                f"<td>{conclusion_cell(step.get('conclusion'), step.get('status'))}</td></tr>"
                for step in steps
            )
            details.append(
                f"<details><summary>{esc(job.get('name', ''))} - steps</summary>"
                "<table><tr><th>#</th><th>Step</th><th>Result</th></tr>"
                f"{step_rows}</table></details>"
            )
    table.append("</table>")
    return "\n".join(table + details)


def tap_lines(run_dir):
    lines = []
    for log in sorted(run_dir.rglob("*.log")):
        match = TAP_RE.search(log.read_text(encoding="utf-8", errors="replace"))
        if match:
            failed = match.group(3) or "0"
            lines.append(
                f"<li>{esc(log.relative_to(run_dir.parent.parent).as_posix())}: "
                f"<b>{esc(match.group(1))}/{esc(match.group(2))} passed</b>, {esc(failed)} failed</li>"
            )
    return lines


def inventory_blocks(run_dir):
    blocks = []
    for path in sorted(run_dir.rglob("*inventory*.txt")):
        text = path.read_text(encoding="utf-8", errors="replace")
        if len(text) > 200_000:
            text = text[:200_000] + "\n... truncated ...\n"
        blocks.append(
            f"<h4>{esc(path.name)}</h4><pre>{esc(text)}</pre>"
        )
    return blocks


def data_html(run_dir, site_ci):
    if not run_dir.exists():
        return []
    files = [path for path in sorted(run_dir.rglob("*")) if path.is_file()]
    if not files:
        return []
    return [
        "<h4>Published data</h4>",
        "<ul>"
        + "".join(
            f'<li><a href="{esc(path.relative_to(site_ci).as_posix())}">'
            f"{esc(path.relative_to(site_ci).as_posix())}</a> "
            f"<span class='mute'>({path.stat().st_size // 1024} KiB)</span></li>"
            for path in files
        )
        + "</ul>",
    ]


def workflow_html(repo, slug, workflow_file, site_ci):
    parts = [f'<h3 id="{slug}">{esc(workflow_file)}</h3>']
    try:
        runs = fetch_runs(repo, workflow_file)
    except RuntimeError as exc:
        parts.append(f"<p class='bad'>run list unavailable: {esc(exc)}</p>")
        return "\n".join(parts)
    if not runs:
        parts.append("<p class='mute'>No runs yet.</p>")
        return "\n".join(parts)

    parts.append(runs_table(runs))
    latest = next((run for run in runs if run.get("conclusion")), None)
    if latest is None:
        parts.append("<p class='mute'>No completed run to detail yet.</p>")
        return "\n".join(parts)

    run_id = latest["databaseId"]
    parts.append(
        f'<h4>Latest completed run <a href="{esc(latest["url"])}">#{esc(latest["number"])}</a> '
        f"- {conclusion_cell(latest.get('conclusion'), latest.get('status'))}</h4>"
    )
    parts.append(jobs_html(fetch_jobs(repo, run_id)))

    run_dir = site_ci / "data" / slug
    run_dir.mkdir(parents=True, exist_ok=True)
    artifacts = fetch_artifacts(repo, run_id)
    if artifacts:
        rows = ["<table><tr><th>Artifact</th><th>Size</th><th>State</th></tr>"]
        for artifact in artifacts:
            name = artifact.get("name", "")
            size = artifact.get("size_in_bytes") or 0
            state = "expired" if artifact.get("expired") else "available"
            if state == "available" and size <= MAX_EMBED_BYTES:
                if download_artifact(repo, run_id, name, run_dir / name):
                    state = "published below"
            rows.append(
                f"<tr><td>{esc(name)}</td><td>{size // 1024} KiB</td><td>{esc(state)}</td></tr>"
            )
        rows.append("</table>")
        parts.append("<h4>Artifacts</h4>")
        parts.extend(rows)

    if not save_run_log(repo, run_id, run_dir / f"run-{run_id}.log"):
        parts.append(
            "<p class='mute'>run log not published (unavailable or over 20 MiB)</p>"
        )
    taps = tap_lines(run_dir)
    if taps:
        parts.append("<h4>TAP summaries</h4><ul>" + "".join(taps) + "</ul>")
    parts.extend(inventory_blocks(run_dir))
    parts.extend(data_html(run_dir, site_ci))
    return "\n".join(parts)


def page(title, body):
    return f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{esc(title)}</title>
<style>
:root {{ color-scheme: light dark; }}
body {{ font: 14px/1.45 ui-monospace, SFMono-Regular, Consolas, monospace;
        margin: 0 auto; max-width: 1100px; padding: 1rem; }}
table {{ border-collapse: collapse; width: 100%; margin: .5rem 0 1.5rem; }}
th, td {{ border: 1px solid #8884; padding: .3rem .5rem; text-align: left; vertical-align: top; }}
.bad {{ color: #c00; font-weight: 700; }}
.good {{ color: #080; }}
.mute {{ opacity: .7; }}
pre {{ overflow: auto; background: #8881; padding: .5rem; }}
details {{ margin: .3rem 0; }}
</style>
</head>
<body>
{body}
</body>
</html>
"""


def self_test():
    assert "&lt;script&gt;" in esc("<script>alert(1)</script>"), "esc must encode tags"
    match = TAP_RE.search("# 197/197 passed, 0 failed")
    assert match and match.group(1) == "197" and match.group(3) == "0", "TAP summary parse"
    table = runs_table([{
        "url": "https://example.invalid/run/1", "number": 1,
        "displayTitle": "<img src=x onerror=alert(1)>", "event": "push",
        "headBranch": "master", "conclusion": "success", "status": "completed",
        "createdAt": "2026-01-01T00:00:00Z",
    }])
    assert "<img" not in table, "run titles must be escaped"
    assert 'class="good"' in table, "success must render as a good badge"
    jobs = jobs_html([{
        "name": "build", "conclusion": "failure", "status": "completed",
        "startedAt": "2026-01-01T00:00:00Z", "completedAt": "2026-01-01T00:01:00Z",
        "steps": [{"number": 1, "name": "compile", "conclusion": "success", "status": "completed"}],
    }])
    assert 'class="bad"' in jobs and "compile" in jobs, "job and step rendering"
    print("generate_ci_report self-test OK")
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", default="site", help="site directory to write")
    parser.add_argument("--repo", help="owner/repo (default: GITHUB_REPOSITORY or gh repo view)")
    parser.add_argument("--self-test", action="store_true", help="run the built-in checks")
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    repo = args.repo or repo_slug()
    site = Path(args.output)
    site_ci = site / "ci"
    site_ci.mkdir(parents=True, exist_ok=True)

    generated = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M UTC")
    nav = " | ".join(f'<a href="#{slug}">{esc(slug)}</a>' for slug, _ in WORKFLOWS)
    body = [
        "<h1>CI Reports</h1>",
        f"<p class='mute'>repo <a href='https://github.com/{esc(repo)}'>{esc(repo)}</a> "
        f"| generated {esc(generated)} | per-run data below and linked from each run page.</p>",
        f"<p>{nav}</p>",
    ]
    for slug, workflow_file in WORKFLOWS:
        body.append(workflow_html(repo, slug, workflow_file, site_ci))
    (site_ci / "index.html").write_text(page("CI Reports", "\n".join(body)), encoding="utf-8")
    (site / "index.html").write_text(
        page(
            "CI Reports",
            '<meta http-equiv="refresh" content="0; url=ci/">'
            '<p>CI reports: <a href="ci/">open</a></p>',
        ),
        encoding="utf-8",
    )
    print(f"wrote {site_ci / 'index.html'}")


if __name__ == "__main__":
    sys.exit(main())
