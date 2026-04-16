# Target documentation hierarchy

This plan turns the audit into the concrete documentation tree that the rewrite
should produce.

The goal is a small doc set for real users of the implemented tool, with
separate maintainer material only where the code actually needs it.

## Target file tree

```text
README.md
docs/
  quickstart.md
  architecture.md
  backend-extension.md
  archive/
    implementation-phases.md
    terraform-isolated-rehearsal-design.md
  static/
    craddle-will-rock-rehearsal-1937.jpg
```

## File mapping

| Current file | Target outcome | Target path | Notes |
| --- | --- | --- | --- |
| `README.md` | rewrite in place | `README.md` | Keep existing HTML image block and CI badge markup unchanged while rewriting prose around it. |
| `docs/architecture.md` | rewrite in place | `docs/architecture.md` | Convert from planning-heavy architecture notes into a current maintainer-facing architecture doc. |
| `docs/phases.md` | archive and rename | `docs/archive/implementation-phases.md` | Keep as history, not as part of the live user manual. |
| `docs/terraform-isolated-rehearsal.md` | archive and rename | `docs/archive/terraform-isolated-rehearsal-design.md` | Preserve as design history; move current operational truth into README, `docs/quickstart.md`, `docs/architecture.md`, and `docs/backend-extension.md`. |
| none | new file | `docs/quickstart.md` | Concise user/operator quickstart and reference. |
| none | new file | `docs/backend-extension.md` | Maintainer doc for the real internal backend extension seam. |
| `docs/static/craddle-will-rock-rehearsal-1937.jpg` | keep in place | `docs/static/craddle-will-rock-rehearsal-1937.jpg` | Asset only. No rewrite. |

## Audience split

### User-facing entry points

- `README.md`
- `docs/quickstart.md`

These should answer:

- what `dress` does
- when to use it
- how to install and run it
- how isolation and destructive mode work
- where artifacts go
- how to inspect failures

### Maintainer-facing reference

- `docs/architecture.md`
- `docs/backend-extension.md`

These should answer:

- what the implemented boundaries are
- where backend behavior lives in the codebase
- what invariants backends must preserve
- how to add a backend without inventing a wider framework

### Historical material

- `docs/archive/implementation-phases.md`
- `docs/archive/terraform-isolated-rehearsal-design.md`

These should remain available for project history, but they should not be
treated as the main manual and should not be linked as primary guidance from
the rewritten README.

## Writing-bead execution notes

### `dress-rehearsal-xxy.3`

Rewrite `README.md` in place.

Include:

- tool purpose
- safety model
- install
- first run
- artifact locations
- links to `docs/quickstart.md` and `docs/architecture.md`
- explicit mention that
  `https://github.com/pasunboneleve/gcp-service-delivery-template` is an
  infrastructure project written to use `dress-rehearsal` for validation

Do not change:

- the existing HTML image block
- the existing CI badge markup

### `dress-rehearsal-xxy.4`

Create `docs/quickstart.md`.

This bead should absorb the operator-facing live behavior that is currently
split between `README.md` and `docs/terraform-isolated-rehearsal.md`.

### `dress-rehearsal-xxy.5`

Rewrite `docs/architecture.md` in place.

This bead should absorb the current implemented boundary material that is worth
keeping from `docs/terraform-isolated-rehearsal.md`, while dropping planning
history and speculative architecture prose.

### `dress-rehearsal-xxy.6`

Create `docs/backend-extension.md`.

This bead should document the actual internal backend seam now exposed in:

- `src/backends/mod.rs`
- `src/backends/terraform.rs`
- `src/cli/mod.rs`
- `src/core/mod.rs`

## Archive rules

- Archived docs must move under `docs/archive/`.
- Archived docs are kept for project history, not as normative user guidance.
- The rewritten README should not send new users to archive docs as the main
  way to learn the tool.
