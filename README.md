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
- Terraform/OpenTofu backend
- AWS ECS Express infrastructure rehearsal
- lifecycle observability for apply/destroy, not application correctness

Not implemented yet:
- broad backend coverage
- multiple scenario families
- application-level verification

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

## License

Released under the [MIT License](LICENSE).

See [docs/architecture.md](/home/dmvianna/src/projects/dress-rehearsal/docs/architecture.md) for the initial shape.
See [docs/phases.md](/home/dmvianna/src/projects/dress-rehearsal/docs/phases.md) for the ordered implementation plan.
