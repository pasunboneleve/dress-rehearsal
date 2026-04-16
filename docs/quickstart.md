# Quickstart

`dress` rehearses an infrastructure change from a real deployment directory.

It runs the backend tool, records what happened, tears the run down, and keeps
failure evidence when something breaks.

## What You Need

- a Terraform or OpenTofu deployment directory
- `terraform` or `tofu` installed, unless `DRESS_TERRAFORM_BINARY` points to a
  custom binary
- credentials and backend inputs required by that deployment

Supported runtime targets today:

- Linux x86_64
- macOS Apple Silicon

Windows is not supported.

## First Run

From the root of your HCL code:

```bash
dress
```

If `DRESS_DEPLOYMENT_ROOT` is unset, `dress` uses the current working
directory.

The current CLI surface is:

```bash
dress
dress --help
dress --version
dress version
```

## Key Environment Variables

Optional deployment-root override:

```bash
export DRESS_DEPLOYMENT_ROOT=/path/to/deployment/root
```

Useful optional settings:

```bash
export DRESS_RUNS_ROOT=/tmp/dress-runs
export DRESS_WORKING_DIRECTORY=/path/to/deployment/root/env/dev
export DRESS_TERRAFORM_BINARY=tofu
export DRESS_TF_VAR_FILES=/path/to/common.tfvars:/path/to/dev.tfvars
export DRESS_TF_BACKEND_CONFIG_FILES=/path/to/backend.hcl
```

Meaning:

- `DRESS_RUNS_ROOT`
  overrides the default runs directory
- `DRESS_WORKING_DIRECTORY`
  narrows execution to a subdirectory under the deployment root
- `DRESS_TERRAFORM_BINARY`
  selects `terraform`, `tofu`, or a custom binary path
- `DRESS_TF_VAR_FILES`
  passes backend var files through to apply and destroy
- `DRESS_TF_BACKEND_CONFIG_FILES`
  is used only for non-isolated runs; isolated runs force local backend state

## Isolation

Isolated rehearsal is the default.

In isolated mode, the Terraform/OpenTofu backend:

- copies the deployment into a run-scoped workspace
- forces local backend state
- stores transient state under the run directory
- preserves incoming `TF_VAR_*` values
- overlays `TF_VAR_is_dress_rehearsal=true`
- overlays `TF_VAR_dress_run_id=<run-id>`
- scrubs ambient backend-shaping Terraform environment that would break the
  isolated local backend

Use those variables in HCL like this:

- `is_dress_rehearsal` for skip-or-run decisions
- `dress_run_id` for rehearsal-safe naming

Isolation protects backend state. It does not make fixed cloud resource names
safe by itself. Modules still need explicit seams for non-idempotent or
collision-prone resources.

## Destructive Mode

Use `--disable-isolation` only when you mean to run against the deployment
directory's configured backend state:

```bash
dress --disable-isolation
dress --disable-isolation --yes
```

Behavior:

- `dress` prints a warning first
- interactive terminals require typing `disable-isolation`, unless `--yes` is
  given
- this mode can modify or destroy real infrastructure

There is no silent fallback from isolated mode to shared-state mode.

## Artifacts

Default runs root:

```text
<deployment-root>/.dress-runs/
```

Each run lives under:

```text
<deployment-root>/.dress-runs/<run-id>/
```

Useful files:

- `run-metadata.txt`
- `artifacts/run/rehearsal-summary.txt`
- `artifacts/run/step-events.log`
- `artifacts/steps/`
- `artifacts/backends/`
- `preserved/`

## Inspecting Failures

When a run fails, the CLI prints:

- the run id
- the run directory
- the failure stage
- the summary path
- the step log path
- the preserved-artifacts path

Start with:

1. `artifacts/run/rehearsal-summary.txt`
2. `artifacts/run/step-events.log`
3. the failed step's stdout/stderr files under `artifacts/steps/`
4. backend artifacts under `artifacts/backends/`

## Local Template Testing

For local sibling-template work, keep machine-specific paths out of git and use
an explicit sourced file such as `.dress.local.env`:

```bash
export DRESS_DEPLOYMENT_ROOT=../minimal-gcp-github-ci-template/infra
export DRESS_WORKING_DIRECTORY=../minimal-gcp-github-ci-template/infra
export DRESS_TERRAFORM_BINARY=tofu
```

Then:

```bash
source .dress.local.env
dress
```

`dress` does not load that file automatically.
