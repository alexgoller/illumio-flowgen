# flowgen — todo

## v0.2: workloader CSV export for unmanaged workloads (current)

Design: labels attach to networks via new `[[host]]` tables; `flowgen ips --format workloader`
emits a CSV for `workloader wkld-import --umwl`.

- [ ] Task A: `[[host]]` config (nets, labels, hostname_prefix) + validation + longest-prefix label merge (TDD)
- [ ] Task B: `ips --format plain|workloader` CSV writer (hostname,name,interfaces,description,<label keys>) with CSV escaping (TDD)
- [ ] Task C: add `[[host]]` labels to the 3 presets and 7 examples (subagent); examples test asserts every emitted IP gets a role
- [ ] Task D: docs (scenarios.md, quickstart/demo-guide workloader step, README), bump to 0.2.0
- [ ] Verify: fmt, clippy -D warnings, tests, sample CSV inspected; PR, CI green

## v0.1.0 (done)

Spec: `docs/superpowers/specs/2026-10-20-flowgen-design.md`
Plan: `docs/superpowers/plans/2026-10-20-flowgen.md`
Released v0.1.0 with packages built in CI; docs live on GitHub Pages.
