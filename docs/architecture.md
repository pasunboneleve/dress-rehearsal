# Architecture

`dress-rehearsal` is designed as a small execution engine for infrastructure
rehearsal.

## Core Concepts

### `RunContext`

Owns the identity and filesystem shape of a rehearsal run:
- run id
- workdir
- derived artifact paths
- preserved failure artifacts
- materialized metadata

### `StepRunner`

Owns consistent step execution:
- step naming
- stdout/stderr capture
- live logging
- uniform success/failure semantics

### `CleanupManager`

Owns teardown behavior:
- cleanup registration
- reverse-order cleanup
- failure-triggered cleanup
- explicit teardown
- recovery hints and preserved artifacts

### `DeploymentBackend`

Owns deployment engine behavior behind a narrow interface:
- initialize backend state
- deploy/apply changes
- fetch outputs
- destroy/teardown

Initial target: Terraform/OpenTofu.
Future room: CloudFormation.

### `Scenario`

If retained at all, owns only a minimal provider-agnostic contract around a
backend-tool rehearsal:
- prerequisite checks
- backend input shaping when needed
- discovery of backend-managed outputs when needed

A scenario does not own:
- direct cloud-service lifecycle control
- service-specific teardown commands
- provider-service concepts or service families
- application-level correctness checks

### `VerificationSpec`

For the first implementation, verification is lifecycle verification:
- did apply complete successfully
- did destroy/cleanup complete successfully
- were logs, summaries, and failure artifacts preserved

Application-level verification such as HTTP health checks, readiness polling, and
response assertions is explicitly out of scope for the first version.

## Testing Model

The architecture should support three distinct testing levels:

### Unit tests

Used for:
- pure state transitions
- path derivation
- failure classification
- cleanup ordering
- verification assertions

These tests should not require external processes or real infrastructure.

### Mock-environment tests

Used for:
- executable-level behavior with abstracted external dependencies
- backend and scenario interactions behind narrow interfaces
- process and output handling without real cloud infrastructure

These tests should exercise the harness as a system while replacing the
external environment with controllable fakes or test doubles.

### Real integration tests

Used for:
- one minimal but real deployment workflow
- real verification against a deployed surface
- real teardown and artifact preservation behavior

These tests should target a minimal external environment and remain narrow
enough to support iterative development rather than broad platform coverage.

## Non-Goals

- no dynamic plugin system
- no generic workflow engine
- no YAML DSL
- no automatic inference of arbitrary infrastructure layouts
- no coupling to `devloop` in the core architecture
- no provider-service model inside `dress-rehearsal`

## Early Shape

The first implementation path should move one real backend-tool happy path
through these boundaries:

Initial concrete target:
- backend: Terraform/OpenTofu
- verification: lifecycle observability only

Execution path:

1. CLI parses a command into a request.
2. `RunContext` materializes an isolated run.
3. `Scenario` prepares minimal prerequisites and backend inputs.
4. `DeploymentBackend` applies infrastructure.
5. Observability artifacts are recorded for the apply result.
6. `CleanupManager` tears the run down or preserves artifacts on failure.

## Failure Semantics

- Step failure propagates uniformly through `StepRunner`.
- By default, failure should trigger registered cleanup in reverse order.
- Artifact preservation should happen before the process exits on failure.
- Explicit teardown remains available for operator-driven recovery.
- The early implementation should prefer deterministic teardown over partial
  rollback heuristics.
- The first version treats backend apply/destroy as the rehearsal boundary.
- Failures must be diagnosable from preserved step logs, summaries, and backend artifacts.
- The harness must not issue direct cloud-service lifecycle commands outside the backend contract.
- The harness must not model provider services or require provider-service
  concepts in order to run the chosen backend tool.

## Boundary Notes

- The selected backend tool is the sole cloud-facing control surface.
  Cloud-provider APIs should be reached only through that backend tool, not
  through provider-aware orchestration in `dress-rehearsal`.
- Scenario bootstrap remains inside `ScenarioPreparation`: it may add
  prerequisite steps and scenario-owned cleanup actions before backend
  initialization, but it must not implicitly register backend cleanup or
  reshape teardown order across that boundary.
- Any temporary scenario-like abstraction must remain generic to backend
  invocation. Provider-service targets such as ECS services or Lambda functions
  are outside the intended architecture.
- Verification wiring begins only after `Scenario::discover` receives backend
  outputs. Changing verification labels, metadata, requests, or assertions must
  not change deployment inputs or cleanup ordering.
- Any cleanup needed after verification failure must already be registered
  through scenario preparation, scenario discovery, or the backend destroy
  action. Verification itself is not a lifecycle control surface.

## Operational Invariants

### Credentials and secrets

- Secret values must not be persisted in run metadata by default.
- Backends and scenarios must receive credentials through explicit inputs,
  not hidden ambient coupling.
- Secret injection rules should be testable at the boundary where they enter a
  backend or scenario.

### Concurrency and isolation

- `RunContext` must make local filesystem collisions impossible for concurrent
  runs on the same machine.
- Backend state isolation must be explicit per run.
- Preserved artifacts must remain attributable to a single run id.
- Terraform/OpenTofu-specific isolation mechanics, including child-process
  `TF_VAR_*` overlays and backend config shaping, belong inside the backend
  implementation rather than the core orchestration types.

See [terraform-isolated-rehearsal.md](/home/dmvianna/src/projects/dress-rehearsal/docs/terraform-isolated-rehearsal.md)
for the current isolated rehearsal design.

### Observability

- `StepRunner` should support human-readable CLI logs first.
- Structured logs may be added later, but should not complicate the first happy
  path.
- CI usability matters: step names, live process output, and failure summaries
  should remain clear in non-interactive environments.

## Current Narrow Assumptions

### POSIX process model only

- Current limitation: step execution and test fixtures assume POSIX tools such
  as `/bin/sh`, `printf`, and standard filesystem semantics. Windows is not a
  supported runtime target today.
- Justification: Linux and macOS are the only supported release targets, and a
  POSIX-first execution model keeps early failure artifacts and shell commands
  easy to inspect.
- Future extraction point: introduce a platform-aware command construction
  boundary only when a real non-POSIX target is required.

### One backend family

- Current limitation: `DeploymentBackend` currently has one concrete family,
  Terraform/OpenTofu.
- Justification: the first backend exists to prove the apply/destroy lifecycle
  boundary before broadening the configuration surface to additional tools.
- Future extraction point: add a second real backend before generalizing shared
  backend helpers or CLI/backend selection rules.

### One scenario family

- Current limitation: there is still only one generic runtime shape around the
  backend tool, so the abstraction has not yet been proven across materially
  different backend-input or output-discovery needs.
- Justification: one narrow, provider-agnostic path is enough to stabilize the
  orchestration boundary before deciding whether the abstraction should broaden
  or collapse further.
- Future extraction point: only broaden the abstraction if a second real backend
  tool or generic rehearsal mode needs different preparation or discovery
  behavior.

### Verification stays observational

- Current limitation: verification wiring may translate discovered outputs into
  named-value or HTTP checks, but it is not allowed to own service lifecycle
  commands or cleanup registration.
- Justification: keeping verification observational preserves the boundary where
  deployment and teardown stay owned by the backend and cleanup manager.
- Future extraction point: expand `VerificationSpec` only when a second real
  verification mode requires new inputs without crossing into lifecycle
  control.

### Run artifacts stay local and filesystem-backed

- Current limitation: rehearsal evidence is written under `RunContext` on the
  local filesystem rather than through a pluggable artifact sink.
- Justification: local paths are the simplest way to keep summaries, step logs,
  and preserved artifacts attributable to a single run during early
  architecture work.
- Future extraction point: add an artifact publishing boundary only when a real
  remote sink or CI retention workflow needs the same evidence model.
