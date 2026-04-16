# Documentation audit

This audit exists to decide what the current documentation set is, what each
file is doing now, and what should happen to it before rewrite work starts.
It does not rewrite the docs. It classifies them.

## Scope

Main documentation surface in the repository today:

- `README.md`
- `docs/architecture.md`
- `docs/phases.md`
- `docs/terraform-isolated-rehearsal.md`
- `docs/static/craddle-will-rock-rehearsal-1937.jpg`

Reference material consulted for classification, but not itself part of the
user documentation rewrite target:

- `AGENTS.md`
- CLI help from `dress --help`
- code structure under `src/`

## Classification

| File | Audience today | Current state | Fate | Why |
| --- | --- | --- | --- | --- |
| `README.md` | users and maintainers | mixed user docs and planning-era framing | rewrite in place | It is the repo front door, but it still says things like "Current scope", "First concrete target", and "Not implemented yet" even though the tool is implemented. |
| `docs/architecture.md` | maintainers and serious users | partly current, partly planning history | rewrite in place | It names real abstractions, but it still contains sections such as "Early Shape" and "Current Narrow Assumptions" that read like active planning notes rather than stable maintainer docs. |
| `docs/phases.md` | historical/planning | pure implementation history | archive out of main path | It is not user documentation. It describes staged implementation work rather than current behavior. It should not remain in the main doc path once the rewrite lands. |
| `docs/terraform-isolated-rehearsal.md` | mixed operator/maintainer/design audience | design-heavy, behavior-heavy, and overly long for its current role | archive as design history; redistribute current behavior into rewritten user and maintainer docs | It contains real current behavior, but the tone and structure are design-doc oriented. The live parts belong in the rewritten README, quickstart/reference, and architecture/backend docs. |
| `docs/static/craddle-will-rock-rehearsal-1937.jpg` | asset only | current | keep in place | It is a README asset, not prose documentation. |

## Files outside the rewrite set

| File | Role | Decision |
| --- | --- | --- |
| `AGENTS.md` | maintainer/agent operating instructions | keep as-is for now; not part of the user-doc rewrite set |

## Current problems to correct in the rewrite

### `README.md`

- still frames the repository partly as a concept repo
- mixes current behavior with future-looking architecture notes
- sends users to planning/history docs as if they were normal reference docs

### `docs/architecture.md`

- explains real boundaries, but with planning-language residue
- spends space on future extraction points instead of present code reality

### `docs/phases.md`

- implementation history, not user manual material
- should not sit in the live documentation path

### `docs/terraform-isolated-rehearsal.md`

- mixes design rationale, behavior, and implementation detail in one long page
- overlaps with what should become user quickstart/reference and maintainer
  architecture/extension docs

## Outcome required from the next bead

The next planning step must define:

- the target file tree
- which files are rewritten in place
- which files are newly added
- which files move to `docs/archive/`
- which files are removed from the main documentation path entirely
