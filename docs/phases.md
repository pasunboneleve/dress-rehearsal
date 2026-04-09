# Implementation Phases

This project is intentionally staged. The goal is to move one real happy path
through explicit abstractions before broadening scope.

## Phase 1: Scaffold and guardrails

- repository layout
- top-level docs
- initial beads plan
- minimal CLI and module skeleton

## Phase 2: Core execution model

- `RunContext`
- `StepRunner`
- `CleanupManager`

Goal:
- make state, step semantics, and cleanup explicit before backend work starts

## Phase 3: Deployment backend abstraction

- define `DeploymentBackend`
- add Terraform/OpenTofu backend skeleton

Goal:
- isolate deployment engine behavior behind a narrow contract

## Phase 4: Scenario abstraction

- define `Scenario`
- sketch AWS ECS Express scenario boundary

Goal:
- keep target-specific setup, discovery, and verification wiring out of
  orchestration

## Phase 5: Verification model

- define lifecycle-oriented verification reporting
- capture apply/destroy outcomes and failure evidence
- defer application-level assertions

Goal:
- make failure evidence explicit without expanding into service correctness checks

## Phase 6: First AWS happy path

- Terraform/OpenTofu backend
- AWS ECS Express scenario
- apply, artifact capture, destroy through the new abstractions

Goal:
- prove the execution model on one real infrastructure rehearsal with clear observability

## Phase 7: Refactor and stabilize

- remove procedural leftovers
- eliminate global mutable state
- validate isolation guarantees

## Phase 8: Design validation

- validate change isolation
- validate cleanup guarantees
- document remaining hard-coded areas

## Not Yet

- CloudFormation backend implementation
- dynamic plugins
- multiple scenario families at once
- broad generic machinery before the AWS path works cleanly
- direct cloud-service lifecycle commands outside backend apply/destroy
- application-level verification such as HTTP health checks or readiness polling
- service-specific lifecycle control contracts
