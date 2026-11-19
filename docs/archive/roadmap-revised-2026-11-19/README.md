# Archived: docs/roadmap-revised/

**Archived:** 2026-11-19  
**Superseded by:** `docs/ROADMAP.md` (single source of truth)

---

## Reason for Archive

The `docs/roadmap-revised/` directory contained 11 files that were the previous attempt at a roadmap. They contained numerous false "✅ COMPLETE" claims that did not match code reality (verified against `docs/architecture/issues.md` and `docs/tested-working.md`).

The 2026-11-07 audit session identified 30+ false checkmarks across these files. The 2026-11-19 documentation reconciliation consolidated all verified status into a single `docs/ROADMAP.md` file.

## Files Archived

| File | Description |
|------|-------------|
| `00-master-index.md` | Index/TOC |
| `00-executive-architecture.md` | Architecture overview |
| `01-current-state.md` | Status claims (many false ✅) |
| `08-performance-smp.md` | SMP performance (claimed fixed, actually S-1/S-2 OPEN) |
| `11-vahi-native.md` | Native features |
| `12-testing-strategy.md` | Testing approach |
| `13-ci-cd.md` | CI/CD pipeline |
| `14-milestone-matrix.md` | Milestones (many false ✅) |
| `15-technical-debt.md` | Technical debt |
| `16-adr-index.md` | ADR index |
| `17-definition-of-done.md` | Definition of done |
| `architecture-review/` | Subdirectory with reviews |

## Verified Replacements

All verified status now lives in:
- `docs/ROADMAP.md` — single source of truth for roadmap
- `docs/PLAN.md` — master plan with honest gates
- `docs/tested-working.md` — verified working status
- `docs/architecture/issues.md` — verified open-defect ledger
- `docs/syscall-classification.md` — honest syscall classification
- `docs/architecture/UNDONE-audit-profile.md` — audit profile gaps

## Recovery

If any content from these files is needed, it can be recovered from git history or this archive directory.