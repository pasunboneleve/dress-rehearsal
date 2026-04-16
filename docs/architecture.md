# Architecture

`dress-rehearsal` is a small orchestration layer around a deployment backend.

The current executable path is narrow:

- one CLI command surface: `dress`
- one backend family: Terraform/OpenTofu
- one generic scenario shape: backend rehearsal
- one verification posture: observe lifecycle success or failure and preserve
  evidence

The architecture matters because the tool must stay easy to change without
turning into a pile of shell glue.

## Execution Flow

Current run flow:

1. the CLI loads configuration from explicit environment variables and the
   current working directory
2. the CLI selects an execution mode: isolated by default, non-isolated only
   through `--disable-isolation`
3. `rehearse()` creates a `RunContext`
4. the `Scenario` prepares backend inputs and any scenario-local setup
5. the `DeploymentBackend` initializes, deploys, collects outputs, and exposes
   a destroy action
6. the `Scenario` discovers surfaced values and verification inputs from the
   backend result
7. verification runs
8. `CleanupManager` tears the run down in reverse registration order
9. summaries, step logs, metadata, and preserved artifacts are written under the
   run directory

## Core Types

### `RunContext`

`RunContext` owns the filesystem identity of one run.

It creates and names:

- the run root
- the work directory
- the artifacts directory
- the preserved-artifacts directory
- `run-metadata.txt`

The rest of the system receives explicit paths from `RunContext` instead of
reconstructing them ad hoc.

### `StepRunner`

`StepRunner` owns named process execution.

It provides:

- stable step names
- stdout and stderr capture
- live terminal logging
- recorded step events
- consistent success and failure reporting

The backend and scenario layers build `StepCommand` values. `StepRunner`
executes them.

### `CleanupManager`

`CleanupManager` owns teardown.

It registers cleanup actions as the run progresses, executes them in reverse
order, and distinguishes normal teardown from failure-triggered cleanup.

This is what keeps cleanup structural instead of optional.

### `DeploymentBackend`

`DeploymentBackend` is the deployment-tool seam.

It must provide:

- `initialize`
- `deploy`
- `outputs`
- `destroy_action`

The core does not know how the backend tool shapes state, environment, backend
config, or child-process arguments. That detail belongs in the backend
implementation.

### `Scenario`

`Scenario` is still present, but it is intentionally narrow.

It owns:

- preparation of backend inputs
- optional preparation steps
- optional scenario-local cleanup actions
- discovery of surfaced values and verification inputs from backend outputs

It does not own:

- direct cloud-service lifecycle commands
- provider-service concepts
- backend-specific state shaping
- deployment or destroy execution

### Verification

Verification is observational.

The current tool does not perform application-health or readiness testing. It
uses scenario discovery to build verification inputs, then records whether the
run produced the expected surfaced values and whether cleanup completed.

## Terraform/OpenTofu Boundary

The current backend implementation lives in `src/backends/terraform.rs`.

Terraform/OpenTofu-specific behavior stays there:

- binary selection
- isolated versus non-isolated execution behavior
- run-scoped workspace materialization
- local-state forcing in isolated mode
- child-process environment shaping
- backend config file handling
- backend JSON output parsing

The core orchestration code does not know about:

- `TF_VAR_*`
- `TF_CLI_ARGS*`
- backend override files
- `-state=...`
- backend config file formats

That is the intended boundary.

## Safety Boundary

The tool's safety model has two layers:

### Filesystem and backend-state isolation

Isolated mode creates a run-scoped workspace and local transient backend state
under `.dress-runs/`.

That keeps:

- run artifacts attributable to one run
- backend state separate from the source working tree
- source deployment files free from run-time mutation

### HCL-owned resource isolation

The backend also overlays:

- `TF_VAR_is_dress_rehearsal=true`
- `TF_VAR_dress_run_id=<run-id>`

Those variables give the module under test explicit seams for:

- skipping non-rehearsal-safe resources
- deriving rehearsal-safe names

`dress` cannot make hardcoded resource identities safe by itself. That remains
the module's responsibility.

## Failure Semantics

The tool fails loudly and preserves evidence.

Failure behavior:

- step failures propagate through `StepRunner`
- failed runs still attempt cleanup
- cleanup runs in reverse registration order
- failure summaries and step logs are written into the run directory
- failed-step stdout and stderr can be preserved for diagnosis

The CLI then reports:

- run id
- run directory
- failure stage
- summary path
- step log path
- preserved-artifacts path

## Current Scope

Implemented now:

- default isolated rehearsal
- explicit destructive escape hatch with `--disable-isolation`
- Terraform/OpenTofu backend
- backend-driven apply/output/destroy cycle
- scenario preparation and discovery
- recorded cleanup and artifact preservation

Not implemented now:

- multiple backends
- dynamic plugin loading
- provider-service orchestration
- application-level health checks
- Windows support
