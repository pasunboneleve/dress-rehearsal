# Changelog

All notable changes to `dress-rehearsal` will be recorded in this file.

## [Unreleased]

## [0.4.0] - 2026-04-16

### Changed
- Release publishing now uses `softprops/action-gh-release@v3`, which is
  compatible with the GitHub Actions Node 24 runtime.
- Isolated Terraform/OpenTofu runs now inject
  `TF_VAR_is_dress_rehearsal=true` in addition to `TF_VAR_dress_run_id`,
  making rehearsal-only conditionals explicit in HCL.
- The Terraform/OpenTofu contract now distinguishes between:
  - `is_dress_rehearsal` for skipping non-idempotent or tombstone-prone
    resources
  - `dress_run_id` for deriving rehearsal-safe resource names when resources
    should still be created during rehearsal
- Rewrote the repository docs as a manual for the implemented tool, with a
  user-facing README, operator quickstart, backend-extension guide, and archive
  split between live documentation and historical design notes.

### Fixed
- Isolated Terraform/OpenTofu runs now scrub ambient backend-shaping
  environment such as `TF_CLI_ARGS_init` before spawning child processes, so a
  previous non-isolated run or direnv-loaded shell cannot leak remote backend
  config into the isolated path.

## [0.3.0] - 2026-04-15

### Added
- Isolated Terraform/OpenTofu rehearsal is now the default execution mode.
  Rehearsals run in a copied workspace with local-only state, preventing
  accidental modification of shared remote state or live infrastructure.
- Added `--disable-isolation` flag as a destructive escape hatch for operators
  who need to run against shared state. Requires interactive confirmation
  (typing `disable-isolation`) or `--yes` for non-interactive automation.
- Added `TF_VAR_dress_run_id` injection in isolated mode, providing modules
  with a per-run identifier for resource name isolation.
- Documented the module naming contract for safe rehearsal coexistence in
  `docs/terraform-isolated-rehearsal.md`, including safe/unsafe patterns and
  guidance for module authors.

### Changed
- Backend configuration shaping is now fully owned by the Terraform/OpenTofu
  backend. In isolated mode, a `dress_backend_override.tf` file forces local
  backend; in non-isolated mode, user-provided config files are passed via
  `-backend-config`.
- Workspace copying in isolated mode now excludes `.terraform`, `.git`,
  `.dress-runs`, `backend.auto.hcl`, and `*.auto.tfbackend` files to avoid
  polluting the run-scoped workspace and prevent backend config conflicts.
- Isolated workspace materialization now widens the copied source root when
  Terraform/OpenTofu configuration uses parent-relative paths such as
  `${path.module}/../scripts/...`, so sibling helper assets remain available
  inside the run-scoped workspace.
- Transient state files are now stored exclusively under the run-scoped
  `.dress-runs/<run-id>/` directory, never in the user's deployment directory.

### Fixed
- Fixed terminal detection for `--disable-isolation` confirmation to check both
  stdin and stderr, preventing blocking when stdin is redirected but stderr is
  a terminal.
- Fixed isolated mode to work with modules that have remote backend
  configurations (GCS, S3, etc.) by generating a `dress_backend_override.tf`
  file that forces local backend. Previously, `terraform init -backend=false`
  was insufficient because apply/destroy would still detect the unconfigured
  remote backend.
- Final CLI failure summaries now use warning styling for existing-resource
  conflict failures when the underlying Terraform/OpenTofu error indicates a
  collision such as `already exists`.

## [0.2.0] - 2026-04-15

### Added
- Added a real first-run CLI surface: `dress` now runs the current backend
  rehearsal flow by default, and `dress version` plus `dress --version` report
  the Cargo package version.
- Added comprehensive CLI help text that describes the current scope, default
  behavior, minimal requirements, and environment-driven configuration without
  advertising unsupported capabilities.
- Added a documented local-only developer workflow for sibling template repos
  using explicit sourced env files such as `.dress.local.env`, which remain
  git-ignored and are never auto-loaded.
- Added explicit tests for command selection, version reporting, deployment-root
  fallback behavior, and the generic backend rehearsal path.

### Changed
- Corrected the architectural boundary so `dress-rehearsal` is documented and
  implemented as a provider-agnostic backend rehearsal tool rather than a
  provider-service-aware orchestration layer.
- Replaced the live AWS-specific CLI/runtime path with a generic backend
  rehearsal flow and removed the legacy provider-aware scenario module.
- Defaulted the deployment root to the current working directory when
  `DRESS_DEPLOYMENT_ROOT` is unset, while keeping the environment variable as
  an explicit override.
- Updated the smoke helper entrypoint and repository docs to match the new
  default-command behavior and explicit local workflow.
- Validated and documented core isolation, cleanup, and boundary guarantees so
  rehearsal behavior remains deterministic as the CLI surface becomes more
  usable.

## [0.1.0] - 2026-04-13

### Added
- Released `dress-rehearsal` as an initial MIT-licensed crate with a
  changelog, release-note extraction script, and GitHub Actions workflows for
  Linux and Apple Silicon CI and release packaging.
- Added GitHub Actions CI that runs `cargo fmt --check`, `cargo test`, and
  `cargo clippy --all-targets --all-features -- -D warnings` on Ubuntu and
  Apple Silicon macOS runners.
- Added tag-driven release workflows that verify the Cargo package version,
  build the `dress` binary for Linux x86_64 and macOS Apple Silicon, package
  archives, and publish GitHub release assets.

### Changed
- Updated the crate version to `0.1.0` and documented installation, license,
  and CI status in the repository README.
- Tightened backend and cleanup code so the existing Rust codebase passes the
  stricter formatting and clippy gates enforced by CI.
