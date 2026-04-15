# Changelog

All notable changes to `dress-rehearsal` will be recorded in this file.

## [Unreleased]

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
