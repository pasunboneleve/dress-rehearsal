# AGENTS.md

## Purpose

This repository builds `dress-rehearsal`, a Rust-based infrastructure
integration harness.

CLI name: `dress`

The goal is to make infrastructure changes rehearsable through full end-to-end
execution:
- isolated run context
- deploy/apply
- verification
- teardown/cleanup
- artifact preservation on failure

This project exists to make infrastructure change safe to verify, not merely
easy to declare.

## Core Engineering Principles

### 1. Abstract first

Do not grow this into a pile of scripts with hidden coupling.

When repeated orchestration patterns appear, extract explicit abstractions.
Prefer named boundaries over procedural sprawl.

### 2. TDD where practical

Use tests to lock down:
- abstraction boundaries
- failure semantics
- cleanup guarantees
- verification behavior

The architecture must support three levels of testing:
- unit tests for isolated logic and invariants
- mock tests that abstract the external environment seen by the executable
- integration tests that exercise a real minimal external environment and
  workflow

Not every tiny step requires tests first, but core execution model changes
should be driven by tests whenever practical.

### 3. Directory structure first

Keep clear separation of concerns in the codebase.

Avoid mixing:
- CLI parsing
- run context/state
- orchestration
- backend-specific logic
- scenario-specific logic
- verification logic
- cleanup logic

### 4. Explicit interfaces over convenience helpers

Prefer small, explicit traits and structs over loose helper functions.

This tool must remain evolvable under change.

Interfaces should be shaped so they can be exercised at all three testing
levels without hidden coupling.

### 5. No hidden state

Do not rely on:
- global mutable state
- ambient environment assumptions
- ad hoc temp paths
- implicit cross-step coupling

All meaningful execution state should flow through explicit context objects.

## Architecture Guardrails

### Stable core concepts

The architecture should preserve these concepts:

- `RunContext`
- `StepRunner`
- `CleanupManager`
- `DeploymentBackend`
- `Scenario`
- `VerificationSpec`

### Plugin architecture, but narrow

This project supports pluggable internal abstractions for:

#### Deployment backends

Examples:
- Terraform/OpenTofu
- CloudFormation

#### Scenarios

Examples:
- AWS ECS Express HTTP service
- AWS Lambda Function URL
- GCP Cloud Run HTTP service

Use internal Rust traits/modules.
Do not build dynamic plugin loading.
Do not build a framework.

### Scenario meaning

A `Scenario` is not just:
- a directory
- a list of scripts
- a shell pipeline

A `Scenario` is the target-specific rehearsal contract:
- preparation
- deployment-specific discovery
- verification setup
- prerequisite checks
- target-specific behavior

## Scope Discipline

Do not build:
- a YAML DSL
- a generic workflow engine
- a multi-cloud platform
- a plugin marketplace
- support for hypothetical future use cases before real demand

Do build:
- a small, explicit infrastructure rehearsal engine
- one happy path at a time
- reusable boundaries proven by at least two real use cases before broadening
  them

## Implementation Strategy

### 1. Extraction over rewrite

Prefer incremental extraction from working behavior.
Do not rewrite everything at once.

### 2. Move one happy path first

Stabilize one real path using the new abstractions before migrating additional
paths.

### 3. Cleanup is structural

Cleanup must be guaranteed by the execution model, not by convention or memory.

### 4. Preserve failure artifacts

When runs fail, preserve enough artifacts and summaries to support diagnosis.

### 5. Keep the core independent

Do not couple the architecture to devloop or other external orchestration
tools. Future integrations are fine, but the core must stand alone.

## Tooling Requirements

### beads

Use beads for planning and execution tracking.

When substantial work is needed:
- create or update epics/issues first
- keep issue scope explicit
- prefer small executable units

### roborev

Use roborev for adversarial review.

Especially use it to critique:
- abstraction quality
- coupling risks
- naming quality
- missing invariants
- cleanup/failure semantics

Do not treat roborev as ceremonial.
Use it to find real architectural weaknesses.

## Review Heuristics

Before merging a change, ask:

- Does this make boundaries clearer or blurrier?
- Does this reduce coupling or hide it?
- Does cleanup become more guaranteed or less?
- Does verification become more explicit or more procedural?
- Would a second backend or scenario fit without reshaping the core?
- Is this abstraction demanded by real behavior, or invented too early?
- Can this behavior be tested at unit, mock, and real integration levels
  without redesigning the boundary?

## Naming

- Repository / concept: `dress-rehearsal`
- CLI: `dress`

Keep the metaphor coherent:
this tool performs a full rehearsal before live execution.

## Current Bias

Bias toward:
- explicit state
- small interfaces
- strong cleanup semantics
- reproducibility
- readability
- evolvability

Bias against:
- cleverness
- hidden control flow
- ambient assumptions
- giant abstractions
- premature generalization

## Session Completion

When ending a work session in this repo:

1. File or update beads for remaining work.
2. Run the relevant quality gates for changed code.
3. Update issue status so the next session starts from the real state.
4. Sync and push completed work:
   ```bash
   git pull --rebase
   bd sync
   git push
   git status
   ```
5. Confirm the branch is up to date and the work is not stranded locally.

Do not stop at a locally complete change if the repo workflow still expects the
work to be synced and pushed.
