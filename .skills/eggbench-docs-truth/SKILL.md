---
name: eggbench-docs-truth
description: |
  Audit and repair eggbench's prose so it matches the code — docs/*.md,
  README.md, and the architecture/ deep dives. Use when a task says the docs
  are stale, wrong, out of date, or need pruning; when reviewing a doc-only
  change; when a version number, driver list, exit code, field table, or
  citation may have drifted; or when deciding whether a doc duplicates
  architecture/. Triggers on "update the docs", "docs are stale", "verify the
  README", "prune documentation", "is this claim still true", "schema version in
  the docs". Do not use for changing a schema or contract to make code match a
  document (use `eggbench-contract-versioning`), and do not use for building or
  running the workspace.
---

# Eggbench docs truth

## Inputs to collect

- Whether the goal is to fix a known claim, to sweep a whole file, or to decide
  what to delete. The three have different stopping conditions.
- For any version or count claim, the code location that defines it. A doc
  number is never the source of truth.

## The rule this repo has already paid for

The single largest defect class found in this workspace's deep-dive audit was
prose that contradicted code which was itself correct: `ResolvedPlan` v3 to v6,
`TrialExecutionResult` v1 to v2, `ComparisonReceipt` v2 to v4, the unconditional
drain contract, the timeout allowlist. Each bump was verified against source
while the document stating the version was left alone. A second class is
citations that survive a refactor: a `file.rs:LINE` reference stays
mechanically in range while the code under it has moved on. So the default
assumption when auditing is that a doc is stale until re-read against source,
not that it is fine because it was reviewed once.

## Procedure

1. Establish the code source of truth for each claim class before reading
   prose. Grep the constant or the parser rather than the doc:

   | Claim | Source of truth |
   |---|---|
   | Schema version | the `*_SCHEMA_VERSION` const in the owning crate |
   | Readable version range | the accept branch / legacy constants |
   | Exit code | `ExitCode` discriminants in the CLI's `envelope.rs`, plus the category strings and mapping in `error.rs` |
   | CLI flags and subcommands | the clap parser in the CLI crate |
   | Driver inventory and features | `catalog.rs` and the `Cargo.toml` feature tables |
   | Evidence layout | the writer in `evidence.rs` |
   | Bounds (counts, byte caps) | the enforcing check, not a comment |

2. For each citation of the form `file.rs:LINE`, confirm the target line still
   supports the sentence. Line-in-range is necessary, not sufficient — the
   verifier only checks the range.

3. Read the surrounding structure, not just the text. A real example: a
   corrective paragraph was inserted between the rows of the metric-vocabulary
   table in `docs/metrics.md`, which pushed six rows outside the table so they
   stopped rendering as rows at all. The values were correct; the structure was
   not, and no version check would have caught it.

4. Run the mechanical checks after editing:

   ```bash
   python3 scripts/verify-architecture-docs.py
   ```

   It verifies relative links resolve, `file:line` citations are in range, every
   deep dive carries the ten required sections, and `overview.md` links every
   deep dive. It cannot judge whether a citation points at the right line.

5. Sweep for staleness that no mechanical check catches:

   - a milestone or condition that `plans/registry.md` records as CLOSED while
     a doc still narrates it as open work;
   - a scope statement written in an early milestone and never retracted (a
     "remaining non-goals" list is the usual offender once the non-goal ships);
   - a findings table whose rows were fixed but still read as live defects.

   For the last case, a fixed row should be deleted or explicitly marked
   resolved. A table that lists already-corrected items as open contradictions
   is worse than no table, because it trains the reader to distrust the parts
   that are still true.

## Pruning

Prune when a document is narrative with no contract content that
`architecture/` does not already carry with citations. A milestone-by-milestone
recap whose every technical claim is restated more precisely in a deep dive is
a candidate. Before deleting, state what unique information would be lost; if
the answer is nothing, delete rather than keep a second, weaker source.

Do not delete a contract doc that carries detail no deep dive restates —
rejection categories, bound values, gate semantics, per-version rules. Overlap
is acceptable; losing the only statement of a rule is not.

When a doc is kept, keep it authoritative for something. `README.md` should
orient a reader and link; `docs/` should hold contracts; `architecture/` should
explain mechanism and cite code.

## Failure handling

- A doc claim that is right and the code that is wrong is a code finding, not a
  doc fix. Report it rather than editing the doc to match a bug.
- When a claim cannot be verified, say so instead of guessing; an unverified
  rewrite is a new claim with no evidence.
- Report which files were checked and found accurate, so the absence of a fix is
  distinguishable from an unchecked file.

## Examples

Input: "check the README and docs for anything stale"
Output: version/exit-code/flag/inventory/citation sweep against the table above,
structural read of tables, then the verifier, then a per-file statement of what
was corrected and what was confirmed accurate.

## Windows (win32) platform notes

The audit is file reads and `python3 scripts/verify-architecture-docs.py`. On
Windows the script runs as `python scripts\verify-architecture-docs.py`. Path
handling in ad-hoc greps needs the repo-relative form; the verifier already
normalizes separators itself.
