# Backend Extension

This repository does not implement dynamic plugins.

The extension seam is internal Rust code. To add a new backend, add a new
`DeploymentBackend` implementation and wire it into the existing executable
path.

## Where The Seam Lives

Core backend contract:

- `src/backends/mod.rs`

Current concrete backend:

- `src/backends/terraform.rs`

Current orchestration path that consumes the backend:

- `src/core/mod.rs`
- `src/cli/mod.rs`
- `src/scenarios/mod.rs`
- `src/scenarios/backend_rehearsal.rs`

## What A Backend Must Implement

Implement `DeploymentBackend`:

- `name()`
- `initialize()`
- `deploy()`
- `outputs()`
- `destroy_action()`

Meaning:

- `initialize()` validates inputs, materializes backend-local state, and returns
  a `BackendSession`
- `deploy()` performs the backend tool's apply step
- `outputs()` returns normalized string outputs as `BackendOutputs`
- `destroy_action()` returns the cleanup action that the core will register and
  run later

The backend owns the tool-specific details. The core owns orchestration order.

## Backend Inputs And Session State

The scenario hands the backend a `BackendRequest`.

It contains:

- deployment root
- optional working directory
- explicit backend environment

The backend turns that into a `BackendSession`, which carries:

- backend name
- deployment root
- working directory
- backend-scoped work directory
- backend-scoped artifact directory
- explicit environment

Use `BackendSession` for all run-local backend paths. Do not reconstruct backend
paths ad hoc in the backend implementation.

## What Must Stay Inside The Backend

Keep backend-tool semantics inside the backend implementation.

For Terraform/OpenTofu, that currently includes:

- child-process environment shaping
- isolated workspace copying
- local backend override files
- backend config file handling
- `-state=...` handling
- JSON output parsing
- `TF_VAR_*` overlays

Do not push those concerns into:

- `RunContext`
- `StepRunner`
- `CleanupManager`
- `Scenario`
- CLI help text beyond user-visible behavior

If a second backend needs similar behavior, extract only the minimum shared
helper that reduces duplication without making the contract harder to read.

## Wiring A New Backend

Minimum work:

1. add a new module under `src/backends/`
2. export it from `src/backends/mod.rs`
3. implement `DeploymentBackend`
4. add backend-specific tests
5. instantiate it from `src/cli/mod.rs` when the CLI grows to support backend
   selection

Today the CLI hardwires `TerraformBackend`. That is acceptable because there is
only one backend family in the repository.

## Invariants A Backend Must Preserve

### Cleanup

- `destroy_action()` must be safe to register once deployment state exists
- cleanup must remain attributable to the run that created it
- cleanup logic must not depend on hidden global state

### Observability

- step names should stay human-readable
- failures must preserve enough context to diagnose backend behavior
- backend artifacts should be written under the backend artifact directory

### Explicit state

- backend inputs should flow through `BackendRequest` and `BackendSession`
- avoid hidden ambient coupling where practical
- if the backend must read parent environment, turn that into explicit child
  process state at the backend boundary

### Boundaries

- the backend may drive the backend tool
- the backend must not issue direct provider-service lifecycle commands outside
  that tool
- the backend should return normalized outputs rather than exposing raw
  tool-specific output structures to the core

## Testing A Backend

At minimum, cover:

- request validation
- command construction
- run-local state and artifact paths
- output parsing
- cleanup action behavior
- isolation behavior, if the backend supports it

The current Terraform/OpenTofu tests are the model to follow.
