# dress-rehearsal

[![Linux CI](https://github.com/pasunboneleve/dress-rehearsal/actions/workflows/linux-ci.yml/badge.svg)](https://github.com/pasunboneleve/dress-rehearsal/actions/workflows/linux-ci.yml)
[![macOS CI](https://github.com/pasunboneleve/dress-rehearsal/actions/workflows/macos-ci.yml/badge.svg)](https://github.com/pasunboneleve/dress-rehearsal/actions/workflows/macos-ci.yml)

`dress` rehearses infrastructure changes end to end.

It runs Terraform in a run-scoped workspace, applies the
infrastructure, records what happened, tears it down, and preserves
evidence when a run fails.

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
runs it end to end, observes what breaks, and tears it down safely.

## What it is for

Use `dress` to rehearse an infrastructure change before trusting
it against shared state.

Current scope:

- backend implementation: Terraform/OpenTofu (extensible to other
  backends)
- default execution mode: isolated rehearsal
- lifecycle: init, apply, output collection, destroy
- evidence preserved on failure: step logs, summaries, backend artifacts
- no provider-service control surface in `dress` itself
- no application-health or readiness checks

`dress` operates the backend tool and the rehearsal mechanics around it.
Cloud-provider APIs are reached through that backend tool, not through
provider-aware logic in `dress`.

## Install

Install from GitHub:

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

## Quick start

From the root of your HCL code:

```bash
dress
```

What `dress` does by default:

- uses the current working directory as the deployment root unless
  `DRESS_DEPLOYMENT_ROOT` is set
- creates a run directory under `DRESS_RUNS_ROOT` or
  `<deployment-root>/.dress-runs`
- runs Terraform/OpenTofu in isolated mode by default
- collects outputs and then runs destroy
- preserves run artifacts when a step fails

Show help or version:

```bash
dress --help
dress --version
dress version
```

Disable isolation (destructive, use with caution):

```bash
dress --disable-isolation        # requires interactive confirmation
dress --disable-isolation --yes  # skips confirmation (for automation)
```

## Safety model

Isolated rehearsal is the default.

In isolated mode, the Terraform/OpenTofu backend:

- runs from a copied run-scoped workspace
- uses run-scoped local state under `.dress-runs/`
- preserves parent `TF_VAR_*` inputs and overlays rehearsal-only values in the
  child backend process
- avoids mutating the source deployment directory

That isolates run artifacts and backend state from the source working tree. It
does not make fixed cloud resource names safe by itself. Modules still need
explicit seams for:

- skipping non-rehearsal-safe resources with `is_dress_rehearsal`
- deriving rehearsal-safe names with `dress_run_id`

`--disable-isolation` turns those protections off and runs against the working
directory's configured backend state. Treat it as destructive.

## Configuration

Useful optional environment variables:

```bash
export DRESS_DEPLOYMENT_ROOT=/path/to/terraform/project/root
export DRESS_RUNS_ROOT=/tmp/dress-runs
export DRESS_TERRAFORM_BINARY=tofu
```

During isolated runs, the backend injects these child-process variables for the
module under test:

- `TF_VAR_is_dress_rehearsal=true` for explicit rehearsal-only conditionals
- `TF_VAR_dress_run_id=<run-id>` when a module needs rehearsal-specific names

This leaves the parent shell unchanged while giving HCL a small explicit
contract for isolated runs.

## Artifacts and failure evidence

Runs write their artifacts under:

```text
<deployment-root>/.dress-runs/<run-id>/
```

Useful paths after a failure:

- `artifacts/run/rehearsal-summary.txt`
- `artifacts/run/step-events.log`
- `preserved/`

The CLI prints the run directory, summary path, step log path, and preserved
artifacts path at the end of each run.

## Example project

[`gcp-service-delivery-template`](https://github.com/pasunboneleve/gcp-service-delivery-template)
is an infrastructure project written to use `dress-rehearsal` for validation.
It is the right kind of repository to read when you want to see how a template
can expose rehearsal-safe seams to `dress`.

## Local development workflow

After cloning, wire up the tracked git hooks. If you use
[direnv](https://direnv.net/), allow the `.envrc`:

```bash
direnv allow
```

Otherwise run it once manually:

```bash
git config core.hooksPath hooks
```

This enables the pre-commit hook, which runs `cargo fmt` on staged changes
to `src/` and delegates to beads for issue tracking.

For local sibling-template testing, keep machine-specific paths out of git and
use an explicit sourced env file such as `.dress.local.env`, which is ignored by
the repo.

Example local-only file contents:

```bash
export DRESS_DEPLOYMENT_ROOT=../minimal-aws-github-ci-template/infra
export DRESS_TERRAFORM_BINARY=tofu
```

## More detail

- [Quickstart and operator reference](docs/quickstart.md)
- [Architecture](docs/architecture.md)
- [Backend extension guide](docs/backend-extension.md)

## License

Released under the [MIT License](LICENSE).
