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

Owns the minimal rehearsal contract for a target:
- prerequisite checks
- backend input shaping when needed
- discovery of backend-managed outputs when needed

A scenario does not own:
- direct cloud-service lifecycle control
- service-specific teardown commands
- application-level correctness checks

Examples:
- AWS ECS Express infrastructure rehearsal
- AWS Lambda Function URL
- GCP Cloud Run HTTP service

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

## Early Shape

The first implementation path should move one real happy path through these
boundaries:

Initial concrete target:
- backend: Terraform/OpenTofu
- scenario: AWS ECS Express infrastructure rehearsal
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

## Boundary Notes

- Scenario bootstrap remains inside `ScenarioPreparation`: it may add
  prerequisite steps and scenario-owned cleanup actions before backend
  initialization, but it must not implicitly register backend cleanup or
  reshape teardown order across that boundary.
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

### Observability

- `StepRunner` should support human-readable CLI logs first.
- Structured logs may be added later, but should not complicate the first happy
  path.
- CI usability matters: step names, live process output, and failure summaries
  should remain clear in non-interactive environments.
