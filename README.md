# dress-rehearsal

[![Linux CI](https://github.com/pasunboneleve/dress-rehearsal/actions/workflows/linux-ci.yml/badge.svg)](https://github.com/pasunboneleve/dress-rehearsal/actions/workflows/linux-ci.yml)
[![macOS CI](https://github.com/pasunboneleve/dress-rehearsal/actions/workflows/macos-ci.yml/badge.svg)](https://github.com/pasunboneleve/dress-rehearsal/actions/workflows/macos-ci.yml)

`dress` is a Rust-based infrastructure integration harness.

Most infrastructure changes are never rehearsed end-to-end before they are
needed live. `dress-rehearsal` exists to make that rehearsal explicit:
materialize isolated context, apply infrastructure, tear it down, and preserve
evidence when things fail.

<br>

<p align="center" style="margin: 0.35rem 0 0.35rem 0;">
  <a href="https://commons.wikimedia.org/wiki/File:Cradle_Will_Rock_Rehearsal_370-9.jpg"
  target="_blank"
  rel="noopener noreferrer">
    <img
        src="docs/static/craddle-will-rock-rehearsal-1937.jpg"
        alt="The Cradle Will Rock rehearsal, 1937"
        style="width:100%;"
        />
  </a>
</p>

<p align="center" style="margin: 0 0 1.25rem 0;">
    <sub>Practice the failure.</sub>
</p>
<br>

A dress rehearsal is the full run before the real performance.

This tool does the same for infrastructure:\
runs it end-to-end, observes what breaks, and tears it down safely.

Current scope:
- establish architecture and execution boundaries
- plan phased implementation work
- keep the initial code skeleton narrow and structural

First concrete target:
- backend-tool rehearsal
- current implementation: Terraform/OpenTofu
- isolated rehearsal is now the default execution path
- lifecycle observability for apply/destroy, not application correctness
- no provider-service model in the intended architecture

Not implemented yet:
- broad backend coverage
- application-level verification

Strict boundary:
- `dress-rehearsal` should operate the selected infrastructure backend tool and generic rehearsal
  mechanics only
- cloud providers must be driven by the backend tool itself, not by
  provider-service-aware logic in `dress-rehearsal`
- any current AWS-specific naming in the codebase is legacy implementation debt,
  not intended product scope

## Install

Install the latest published `main` branch directly from GitHub:

```bash
cargo install --git https://github.com/pasunboneleve/dress-rehearsal.git
```

Tagged releases publish prebuilt archives for:

- `x86_64-unknown-linux-gnu`
- `aarch64-apple-darwin`

Supported platforms are currently:

- Linux x86_64
- macOS Apple Silicon

Windows is not currently supported or tested.

## Usage

Run the current backend rehearsal flow:

```bash
dress
```

Show help or version:

```bash
dress --help
dress --version
dress version
```

Default behavior:

`dress` uses the current working directory as the deployment root when
`DRESS_DEPLOYMENT_ROOT` is not set. So if you run it from the root of your HCL
code, you do not need to export that variable.

By default, the Terraform/OpenTofu backend now rehearses from a run-scoped
copied workspace with a run-scoped local state file under `.dress-runs/`. That
isolates backend state from the source working tree, but it does not make
hardcoded cloud resource names safe by itself. Modules still need explicit
naming seams for collision-safe rehearsal coexistence.

Explicit deployment root override:

```bash
export DRESS_DEPLOYMENT_ROOT=/path/to/deployment/root
```

Useful optional environment:

```bash
export DRESS_RUNS_ROOT=/tmp/dress-runs
export DRESS_WORKING_DIRECTORY=/path/to/deployment/root/env/dev
export DRESS_TERRAFORM_BINARY=tofu
```

The backend also injects `TF_VAR_dress_run_id` into Terraform/OpenTofu child
processes during isolated rehearsal so modules can derive rehearsal-specific
resource names without mutating the parent shell.

## Local Dev Workflow

For local sibling-template testing, keep machine-specific paths out of git and
use an explicit sourced env file such as `.dress.local.env`, which is ignored by
the repo.

Example local-only file contents:

```bash
export DRESS_DEPLOYMENT_ROOT=../minimal-aws-github-ci-template/infra
export DRESS_WORKING_DIRECTORY=../minimal-aws-github-ci-template/infra
export DRESS_TERRAFORM_BINARY=tofu
```

or:

```bash
export DRESS_DEPLOYMENT_ROOT=../minimal-gcp-github-ci-template/infra
export DRESS_WORKING_DIRECTORY=../minimal-gcp-github-ci-template/infra
export DRESS_TERRAFORM_BINARY=tofu
```

Use it explicitly:

```bash
source .dress.local.env
dress
```

`dress` does not load that file automatically. The state remains explicit in
the shell session that sourced it.

## License

Released under the [MIT License](LICENSE).

See [docs/architecture.md](/home/dmvianna/src/projects/dress-rehearsal/docs/architecture.md) for the initial shape.
See [docs/phases.md](/home/dmvianna/src/projects/dress-rehearsal/docs/phases.md) for the ordered implementation plan.
See [docs/terraform-isolated-rehearsal.md](/home/dmvianna/src/projects/dress-rehearsal/docs/terraform-isolated-rehearsal.md) for the isolated rehearsal design.
