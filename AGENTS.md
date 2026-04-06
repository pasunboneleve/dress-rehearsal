# AGENTS.md

## Purpose

`dress-rehearsal` is a small infrastructure rehearsal engine, not a script pile
and not a generic workflow platform.

The first job of an agent in this repo is to preserve boundary quality.

## Guardrails

- Abstract first, not script accretion.
- Create directory structure and explicit interfaces before convenience helpers.
- Use TDD where practical, especially at abstraction boundaries.
- No global mutable state.
- No hidden environment coupling.
- Keep run state explicit through `RunContext`.
- Keep sequencing and failure semantics explicit through `StepRunner`.
- Keep teardown explicit through `CleanupManager`.
- Treat deployment engines as `DeploymentBackend` implementations.
- Treat target-specific behavior as `Scenario` implementations.
- Keep verification explicit through `VerificationSpec`.
- Move one happy path first before widening scope.
- Prefer extraction over rewrite.
- Do not build broad generic machinery before two real use cases demand it.
- Do not introduce a YAML DSL.
- Do not build dynamic plugin loading.
- Use beads for planning and execution tracking.
- Use roborev for adversarial review before broadening the design.

## Execution Rules

- Start with structure, interfaces, and tests.
- Keep the CLI thin. Business logic belongs in library modules.
- Add the smallest useful code needed to move the next bead.
- Stop after one coherent unit of work unless explicitly asked to continue.
- Preserve clear module ownership:
  - `context/` owns run context and derived paths
  - `steps/` owns step execution and process behavior
  - `cleanup/` owns teardown registration and recovery hints
  - `backends/` owns deployment backend abstractions and implementations
  - `scenarios/` owns target-specific rehearsal behavior
  - `verification/` owns readiness and assertion behavior

## Current Intent

The current round is planning and scaffolding only. Do not grow full execution
behavior opportunistically.

## Landing the Plane (Session Completion)

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd sync
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds
