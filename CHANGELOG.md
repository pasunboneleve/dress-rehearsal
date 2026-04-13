# Changelog

All notable changes to `dress-rehearsal` will be recorded in this file.

## [Unreleased]

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
