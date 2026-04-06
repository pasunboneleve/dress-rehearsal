# dress-rehearsal

`dress` is a Rust-based infrastructure integration harness.

Most infrastructure changes are never rehearsed end-to-end before they are
needed live. `dress-rehearsal` exists to make that rehearsal explicit:
materialize isolated context, deploy, verify, tear down, and preserve evidence
when things fail.

The metaphor is literal. A dress rehearsal is the full run before the real
performance. This tool is for rehearsing infrastructure changes the same way:
with the real sequence, clear boundaries, and visible failure handling.

Current scope:
- establish architecture and execution boundaries
- plan phased implementation work
- keep the initial code skeleton narrow and structural

First concrete target:
- Terraform/OpenTofu backend
- AWS ECS Express HTTP scenario

Not implemented yet:
- real deployment backends
- real scenarios
- full end-to-end execution

See [docs/architecture.md](/home/dmvianna/src/projects/dress-rehearsal/docs/architecture.md) for the initial shape.
See [docs/phases.md](/home/dmvianna/src/projects/dress-rehearsal/docs/phases.md) for the ordered implementation plan.
