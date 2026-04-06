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

Owns the contract for what is being rehearsed:
- prerequisite checks
- configuration materialization
- bootstrap preparation
- deployed-surface discovery
- verification wiring
- cleanup expectations

Examples:
- AWS ECS Express HTTP service
- AWS Lambda Function URL
- GCP Cloud Run HTTP service

### `VerificationSpec`

Owns success criteria:
- readiness target
- request shape
- assertions
- retry/timeout policy
- failure artifact capture

## Non-Goals

- no dynamic plugin system
- no generic workflow engine
- no YAML DSL
- no automatic inference of arbitrary infrastructure layouts
- no coupling to `devloop` in the core architecture

## Early Shape

The first implementation path should move one real happy path through these
boundaries:

1. CLI parses a command into a request.
2. `RunContext` materializes an isolated run.
3. `Scenario` prepares backend inputs and verification behavior.
4. `DeploymentBackend` applies infrastructure.
5. `VerificationSpec` verifies the deployed surface.
6. `CleanupManager` tears the run down or preserves artifacts on failure.
