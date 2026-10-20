# flowgen — todo

## v0.2: workloader CSV export for unmanaged workloads (current)

Design: labels attach to networks via new `[[host]]` tables; `flowgen ips --format workloader`
emits a CSV for `workloader wkld-import --umwl`.

- [x] Task A: `[[host]]` config (nets, labels, hostname_prefix) + validation + longest-prefix label merge (TDD)
- [x] Task B: `ips --format plain|workloader` CSV writer (hostname,name,interfaces,description,<label keys>) with CSV escaping (TDD)
- [x] Task C: add `[[host]]` labels to the 3 presets and 7 examples (subagent); examples test asserts every emitted IP gets a role
- [x] Task D: docs (scenarios.md, quickstart/demo-guide workloader step, README), bump to 0.2.0
- [x] Verify locally: fmt, clippy -D warnings, 97 tests, sample CSV inspected
- [ ] PR, CI green

### Review

Subagent-driven (plan `docs/superpowers/plans/2026-10-20-workloader-export.md`): 4 tasks, each approved by task review with no Critical/Important findings. Final opus review → Ready with fixes: scan-demo gave 10.1.1.13 a different hostname than three-tier (workloader matches on hostname → duplicate workload); reserved/whitespace label keys accepted; `ips | head` exited 2. One fix wave (f5741f5) fixed those plus doc/comment polish; scoped re-review clean.

Not verified: behaviour against a live PCE/workloader (CSV format taken from workloader source, cmd/wkldimport).

## v0.1.0 (done)

Spec: `docs/superpowers/specs/2026-10-20-flowgen-design.md`
Plan: `docs/superpowers/plans/2026-10-20-flowgen.md`
Released v0.1.0 with packages built in CI; docs live on GitHub Pages.
