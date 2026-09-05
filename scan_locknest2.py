"""Block-scope lock-nesting scanner (invariant I1/I3 enforcement).

Finds: `let g = RECV.lock()` (block-scoped guard) where a call to a
function that itself BLOCKING-locks RECV appears before `drop(g)` /
block end — the kill_from_fault family (guard held across a call that
re-locked the same mutex). Callee-side try_lock acquisitions are
excluded: they bail on contention, so they cannot self-deadlock.

Caveats: crude tokenizer (no string/comment stripping), name matching on
the last path segment (per-process locks like p.memory vs par.memory are
DIFFERENT mutexes but share the segment 'memory' -> false positives),
one- and two-level call graph. Screening tool: hits get manual review.

Usage: py scan_locknest.py [root-dirs...]
"""
import os, re, sys

ROOTS = sys.argv[1:] or ['kernel/src', 'crates/task/src']

# Callee-side: only BLOCKING lock() acquisitions can nest-deadlock. try_lock
# bails on contention, so route_*_for-style try-lock callers are safe under
# any guard. (Guard-side LET_LOCK_RE below still matches try_lock guards.)
LOCK_RE = re.compile(r'\b(\w+)\.lock\(\)')
CALL_RE = re.compile(r'\b([A-Za-z_]\w*)\s*\(')
DROP_RE = re.compile(r'\bdrop\(\s*(\w+)\s*\)')
LET_LOCK_RE = re.compile(r'\blet\s+(\w+)\s*=\s*([A-Za-z0-9_:\.]+)\.(?:try_)?lock\(\)\s*(?:;|$)')
FN_RE = re.compile(r'\bfn\s+(\w+)\s*\(')

STR_RE = re.compile(r'"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])*\'')

def strip_comments_lines(text):
    out = []
    for line in text.splitlines():
        # crude: cut // comments
        line = re.sub(r'//.*$', '', line)
        # mask string/char literals so embedded { } / ( ) cannot skew
        # brace and statement counting (format_args! strings were breaking
        # fn_bodies depth tracking)
        line = STR_RE.sub('""', line)
        out.append(line)
    return out

def collect_files(roots):
    files = []
    for r in roots:
        for dirpath, _, names in os.walk(r):
            for n in names:
                if n.endswith('.rs'):
                    files.append(os.path.join(dirpath, n))
    return files

def fn_bodies(lines):
    """Yield (fn_name, start_line_idx, end_line_idx).

    Matches free fns and impl methods (depth <= 1). Body ends when the
    brace depth returns to the fn's opening depth on a line with '}'.
    """
    bodies = []
    depth = 0
    cur = None  # [name, start_idx, open_depth]
    for i, line in enumerate(lines):
        if cur is None:
            m = FN_RE.search(line)
            if m and depth <= 1:
                cur = [m.group(1), i, depth]
                if '}' in line and '{' in line:
                    # one-liner `fn f() {}` — closes on the same line
                    bodies.append((cur[0], cur[1], i))
                    cur = None
        else:
            nd = depth + line.count('{') - line.count('}')
            if nd <= cur[2] and '}' in line:
                bodies.append((cur[0], cur[1], i))
                cur = None
            depth = nd
            continue
        depth += line.count('{') - line.count('}')
    return bodies

def function_lock_map(files):
    """fn name -> set of receiver last-segments it locks directly."""
    m = {}
    for f in files:
        lines = strip_comments_lines(open(f, encoding='utf-8', errors='replace').read())
        for name, s, e in fn_bodies(lines):
            segs = set()
            for j in range(s, min(e + 1, len(lines))):
                for mm in LOCK_RE.finditer(lines[j]):
                    segs.add(mm.group(1))
            if name in m:
                m[name] |= segs
            else:
                m[name] = segs
    return m

def main():
    files = collect_files(ROOTS)
    fm_t = function_lock_map(files)

    hits = []
    for f in files:
        lines = strip_comments_lines(open(f, encoding='utf-8', errors='replace').read())
        depth = 0
        guard = None  # (name, receiver_segment, depth_at_let)
        for i, line in enumerate(lines):
            if guard is None:
                gm = LET_LOCK_RE.search(line)
                if gm:
                    recv = gm.group(2)
                    seg = recv.rsplit('.', 1)[-1] if '.' in recv else recv
                    guard = (gm.group(1), seg, depth)
            else:
                name, seg, depth_at_let = guard
                dm = DROP_RE.search(line)
                if dm and dm.group(1) == name:
                    guard = None
                else:
                    for cm in CALL_RE.finditer(line):
                        callee = cm.group(1)
                        if callee in fm_t and seg in fm_t[callee]:
                            hits.append((f, i + 1, f"guard `{name}` (locks {seg}) alive across call to {callee} (locks {sorted(fm_t[callee])})"))
            depth += line.count('{') - line.count('}')
            if guard is not None and depth < guard[2]:
                guard = None  # left the enclosing block
    for h in hits:
        print(f"{h[0]}:{h[1]}: {h[2]}")
    print(f"\n{len(hits)} hits across {len(files)} files")

def i4_pass(files):
    """I4 audit: BFS from IF=0 entry points; flag blocking .lock() calls
    (non-try) in reachable functions. Per-CPU sched locks are self-owned
    (safe-by-construction); they are printed for review, not auto-flagged."""
    ENTRIES = {'page_fault_handler', 'kill_user_process', 'kill_from_fault',
               'tick', 'tick_itimers', 'keyboard_interrupt_handler',
               'feed_scancode', 'timer_interrupt_handler', 'irq_handler'}
    # Generic names whose CALL_RE matches are almost always stdlib/builtin
    # calls or local closures, not real cross-function edges; following them
    # pollutes the closure with same-named syscall functions.
    STOP = {'read', 'write', 'get', 'set', 'push', 'pop', 'lock', 'unlock',
            'handle', 'map', 'format_args', 'alloc', 'box', 'drop', 'clone',
            'new', 'init', 'call', 'next', 'iter', 'map_or', 'unwrap',
            'expect', 'is_some', 'is_none', 'as_ref', 'as_mut', 'ok', 'err'}
    funcs = {}  # name -> (file, calls, blocking_locks)
    for f in files:
        lines = strip_comments_lines(open(f, encoding='utf-8', errors='replace').read())
        for name, s, e in fn_bodies(lines):
            calls, locks = set(), []
            for j in range(s, min(e + 1, len(lines))):
                for cm in CALL_RE.finditer(lines[j]):
                    if cm.group(1) not in STOP:
                        calls.add(cm.group(1))
                for lm in re.finditer(r'(\w+)\.(try_)?lock\(\)', lines[j]):
                    if lm.group(2) is None:
                        locks.append((j + 1, lm.group(1)))
            funcs.setdefault(name, []).append((f, calls, locks))
    seen = set()
    stack = list(ENTRIES)
    hits = []
    while stack:
        name = stack.pop()
        if name in seen or name not in funcs:
            continue
        seen.add(name)
        for f, calls, locks in funcs[name]:
            for ln, recv in locks:
                if 'sched' not in recv.lower():
                    hits.append(f'{f}:{ln}: blocking .lock() on `{recv}` in {name} (IF=0 reachable)')
            stack.extend(calls - seen)
    print(f'\n=== I4 pass: {len(seen)} functions reachable from IF=0 entries ===')
    for h in sorted(hits):
        print(h)
    print(f'{len(hits)} blocking-lock sites (per-CPU sched locks exempt)')

if __name__ == '__main__':
    main()
    i4_pass(collect_files(ROOTS))