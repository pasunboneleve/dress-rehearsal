# Terraform/OpenTofu Isolated Rehearsal Design

## Purpose

This document defines how `dress` should make Terraform/OpenTofu rehearsals
safe by default without teaching the core execution model about
Terraform-specific state, variable, or backend semantics.

## Current Risk

Today `dress` isolates run-local artifacts under `.dress-runs/<run-id>/`, but
the Terraform/OpenTofu backend still executes `init`, `apply`, `output`, and
`destroy` in the selected deployment working directory using that directory's
configured backend.

That means:

- run logs and preserved artifacts are isolated
- Terraform/OpenTofu state is not isolated
- backend configuration is not isolated
- resource identity is not isolated unless the module already does it

The practical consequence is that a rehearsal can touch or destroy real
deployed infrastructure if it is pointed at a directory that uses shared remote
state.

## Why Local Transient State Alone Is Insufficient

Using a local transient state file is necessary, but not sufficient.

It isolates Terraform/OpenTofu's view of state, but it does not isolate the
actual cloud resource names and addresses that the module creates. If the HCL
still uses fixed names, fixed GCS/S3 object prefixes, singleton DNS names,
repository names, or other global identifiers, then a rehearsal apply can still
collide with live infrastructure even when state is local and transient.

Safe rehearsal therefore requires both:

1. backend/state isolation
2. resource identity isolation

`dress` can own the first requirement in the backend plugin. The second
requires an explicit contract with the Terraform/OpenTofu module under test.

## Design Goals

- isolated rehearsal must be the default
- shared-state execution must require an explicit destructive escape hatch
- Terraform/OpenTofu-specific logic must remain inside the backend boundary
- the core must not learn `TF_VAR_*`, backend flags, backend file generation,
  or shell mutation rules
- no writes should occur in the user's deployment directory
- backend configuration shaping must be owned by `DeploymentBackend`, not by
  external helper scripts
- failures must remain diagnosable through run-local artifacts
- if safe isolated rehearsal cannot be achieved well enough, the backend should
  fail closed rather than silently using shared state

## Boundary Decision

The seam belongs in the Terraform/OpenTofu backend, not in the core.

The core already knows how to:

- materialize a run
- call `DeploymentBackend::initialize`
- call `deploy`, `outputs`, and `destroy`
- preserve artifacts and report failures

The core does not need to know:

- how Terraform/OpenTofu isolates state
- how child process environment overlays are constructed
- how backend config files are materialized
- how `init` arguments differ between isolated and non-isolated runs

The only new cross-boundary concept allowed is a narrow execution mode selected
by the CLI and carried to the backend as configuration:

- `Isolated` (default)
- `NonIsolated` (only via `--disable-isolation`)

That mode is generic enough to avoid Terraform-specific coupling in the core.

## Proposed Design

### 1. Default isolated rehearsal mode

In isolated mode, the Terraform/OpenTofu backend will:

1. materialize a run-scoped backend workspace under the existing backend work
   directory
2. copy or materialize the deployment files needed for the backend tool into
   that run-scoped workspace
3. run `terraform`/`tofu` from the run-scoped workspace, not from the user's
   deployment directory
4. avoid the configured remote backend by default
5. use a run-scoped local state path inside the run workspace
6. preserve backend config materialization and derived state inputs as run
   artifacts for debugging

This keeps source-tree mutation and backend-state mutation out of the user's
deployment directory.

### 2. Backend state isolation

The backend will own Terraform/OpenTofu-specific state shaping.

In isolated mode it will prefer:

- run-scoped working directory materialization
- local backend execution
- run-scoped local state file

Operationally, this means the backend should construct child-process execution
that behaves like a local-state rehearsal even when the source module normally
uses a remote backend.

The initial implementation should be conservative:

- fail closed if the backend cannot establish isolated local-state execution
  deterministically
- do not silently reuse the configured shared backend

### 3. Resource identity isolation

`dress` cannot guarantee safe coexistence if the Terraform/OpenTofu module
hardcodes collision-prone names.

The backend will therefore support a rehearsal naming contract by injecting
rehearsal-specific variable overrides into the child backend process
environment. The first contract should stay simple:

- preserve parent `TF_VAR_*` values
- overlay backend-owned rehearsal values only in the child process
- include at least a stable per-run identifier that modules can use for naming

Example contract shape:

- parent shell may already provide `TF_VAR_environment=dev`
- backend adds `TF_VAR_dress_run_id=<run-id>` in the child process only
- backend may also overlay a small set of clearly documented rehearsal
  variables, such as a suffix or prefix derived from the run id

This keeps Terraform/OpenTofu naming semantics inside the backend boundary.
Neither `Scenario` nor the core should know anything about `TF_VAR_*`.

### 4. Child process environment construction

The Terraform/OpenTofu backend may read the parent environment and construct a
child environment for backend commands.

Rules:

- never mutate the parent shell
- preserve incoming `TF_VAR_*` values unless explicitly shadowed by
  rehearsal-specific values
- preserve explicit backend request environment values
- add backend-owned rehearsal overlays only to the child process environment
- record derived non-secret environment decisions in run artifacts or metadata
  when useful for diagnosis
- never persist secret values in plain text artifacts by default

This environment construction belongs to the backend implementation because it
is specific to Terraform/OpenTofu execution semantics.

### 5. Backend configuration shaping moves inside the backend

Some templates currently require external scripts that generate
`backend.auto.hcl` or equivalent backend config files because Terraform does not
allow normal variable interpolation inside backend blocks.

This logic should move into the Terraform/OpenTofu backend implementation.

The backend may:

- read environment variables and explicit backend inputs
- derive backend config values for the child process
- materialize backend config files inside the run-scoped workspace
- pass `-backend-config` arguments

The backend may not:

- require a pre-run bash script to mutate the source tree
- require the CLI, scenario layer, or core to understand backend config
  generation rules

Invariant after this change:

- backend configuration shaping is owned entirely by the Terraform/OpenTofu
  backend
- no external backend-shaping script is required for a `dress` run
- generated backend config is attributable to a run and preserved for debugging

### 6. Destructive escape hatch: `--disable-isolation`

The CLI will gain:

- `--disable-isolation`

Semantics:

- default: isolated rehearsal mode
- with `--disable-isolation`: run against the working directory's configured
  backend and shared state with no isolation guarantees

Behavior in non-isolated mode:

- print an explicit warning before execution
- in interactive terminals, require the operator to confirm by typing
  `disable-isolation`
- allow a second explicit bypass mechanism for non-interactive automation,
  such as `--yes`
- record the selected execution mode in run metadata and logs

The CLI selects the mode; the Terraform/OpenTofu backend interprets it. The
core remains unaware of Terraform-specific consequences.

### 7. Failure semantics and artifacts

Existing failure behavior remains structurally the same:

- backend initialize/deploy/output/destroy failures surface through the backend
  and `StepRunner`
- cleanup remains owned by `CleanupManager`
- preserved artifacts remain attributable to a single run

Additional artifact expectations for isolated mode:

- run-scoped backend workspace path
- derived backend config files
- run-scoped state path location
- execution mode
- a summary of whether isolation was enforced or explicitly disabled

If isolated rehearsal cannot be established safely enough, initialization should
fail before apply.

## Out of Scope

- parsing arbitrary HCL to prove naming safety
- automatically rewriting modules to add naming seams
- guaranteeing coexistence for modules with hardcoded global names
- inventing a generic backend configuration DSL
- teaching `Scenario` about Terraform naming, backend config, or state
- teaching `VerificationSpec` about Terraform/OpenTofu execution semantics
- cloud-service lifecycle commands outside the backend contract

## Refusal and Fail-Closed Behavior

Isolated mode must not silently degrade into shared-state execution.

The backend should fail closed when:

- it cannot materialize a run-scoped workspace safely
- it cannot establish local or otherwise isolated state execution
- required backend config shaping inputs are missing
- the template declares a safety contract that `dress` can tell is not met

The first implementation should prefer conservative refusal over broad
heuristics. It is better to reject a run than to silently target shared state.

## Tradeoffs

### Accepted tradeoffs

- isolated mode adds backend-specific complexity inside the Terraform/OpenTofu
  plugin
- some modules will need explicit naming seams before they can rehearse safely
- backend config shaping inside `dress` increases responsibility in the backend
  implementation, but keeps that responsibility in the correct boundary

### Rejected alternative: teach the core about Terraform isolation

Rejected approach:

- add Terraform-specific state paths, naming variables, backend config files,
  or `TF_VAR_*` overlays to `RunContext`, `Scenario`, or `StepRunner`

Why rejected:

- it would couple the core to one backend family's execution semantics
- it would make future backends fit Terraform's model instead of their own
- it would blur the architecture by pushing backend-specific rules into generic
  orchestration types

### Rejected alternative: keep helper scripts outside the backend

Rejected approach:

- rely on sibling repo scripts to generate `backend.auto.hcl` before running
  `dress`

Why rejected:

- backend behavior would stay split across bash and Rust
- the execution contract would be hidden and repo-specific
- it would violate the invariant that `DeploymentBackend` owns backend behavior

### Rejected alternative: local state only, no naming contract

Rejected approach:

- isolate only the state file and assume that is enough

Why rejected:

- state isolation does not prevent resource-name collisions
- the resulting safety story would be misleading

## Incremental Implementation Plan

1. Write and commit this design.
2. Add backend-level isolated vs non-isolated mode selection.
3. Materialize a run-scoped Terraform/OpenTofu workspace and local state path.
4. Execute backend commands from that workspace in isolated mode.
5. Overlay backend-owned `TF_VAR_*` rehearsal inputs in child processes only.
6. Move backend config generation into the backend implementation.
7. Add CLI confirmation behavior for `--disable-isolation`.
8. Document the module naming contract and refusal behavior.

The first implementation slice should prove the seam by introducing
run-scoped isolated mode in the Terraform/OpenTofu backend without modifying
the core architecture beyond a narrow execution-mode selection.

## Module Naming Contract

Isolated mode protects Terraform/OpenTofu state, but it cannot prevent
resource-name collisions in the cloud. If a module creates resources with
hardcoded names, a rehearsal can still collide with live infrastructure.

### The `TF_VAR_dress_run_id` variable

In isolated mode, `dress` injects a per-run identifier into the child process
environment:

```
TF_VAR_dress_run_id=run-<timestamp>-<sequence>
```

Modules should use this variable to create unique resource names during
rehearsal.

### Safe module pattern

A module that supports isolated rehearsal declares the variable and uses it
for resource naming:

```hcl
variable "dress_run_id" {
  type        = string
  default     = ""
  description = "Rehearsal run identifier for resource name isolation"
}

variable "environment" {
  type = string
}

locals {
  name_suffix = var.dress_run_id != "" ? "-${var.dress_run_id}" : ""
}

resource "google_storage_bucket" "data" {
  name = "my-app-${var.environment}${local.name_suffix}"
  # ...
}
```

When `dress` runs in isolated mode:
- `TF_VAR_dress_run_id` is set to the run id
- Resources get unique names like `my-app-dev-run-0192abc-0001`
- No collision with live `my-app-dev` resources

When running outside `dress` or in production:
- `dress_run_id` defaults to empty string
- Resources use their standard names like `my-app-dev`

### Unsafe module pattern

A module that hardcodes global names cannot rehearse safely:

```hcl
# UNSAFE: no way to isolate resource names
resource "google_storage_bucket" "data" {
  name = "my-app-production"  # collision-prone
}
```

If you attempt to rehearse this module:
- Isolated mode will prevent state collisions
- But `terraform apply` will still try to create or modify `my-app-production`
- This can destroy or corrupt live infrastructure

### Refusal behavior

`dress` cannot detect unsafe naming patterns automatically. It relies on module
authors to adopt the naming contract.

When isolated rehearsal cannot be safe:
- The backend fails closed rather than silently targeting shared resources
- Failures are preserved as run artifacts for diagnosis
- Recovery hints guide operators toward manual cleanup

### Guidelines for module authors

1. Declare `variable "dress_run_id"` with an empty default
2. Use it as a suffix or prefix for all globally unique resource names
3. Test both paths: with and without the variable set
4. Document which resources require the naming contract

### What `dress` guarantees in isolated mode

- State is local and transient (never touches remote state)
- `TF_VAR_dress_run_id` is injected for resource name isolation
- Workspace is copied, so source files are not modified
- Failures preserve artifacts under the run directory

### What `dress` does not guarantee

- Automatic safe naming for modules that don't use `dress_run_id`
- Protection against hardcoded global identifiers
- Cleanup of orphaned cloud resources if naming isolation fails
