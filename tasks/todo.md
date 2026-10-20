# flowgen — todo

Spec: `docs/superpowers/specs/2026-10-20-flowgen-design.md`
Detailed plan: `docs/superpowers/plans/2026-10-20-flowgen.md`

- [x] Task 1: Crate scaffold, FlowRecord model, CI workflow
- [x] Task 2: Export module foundation + NetFlow v5 encoder
- [x] Task 3: NetFlow v9 encoder + template refresh
- [x] Task 4: IPFIX encoder + encoder factory
- [x] Task 5: Scenario config, overrides, validation, collector parsing
- [x] Task 6: Traffic patterns (host pools, rate shapes, pacer, generator, ips)
- [x] Task 7: Built-in presets + example scenario
- [x] Task 8: Stats + terminal UI
- [x] Task 9: Sender loop, CLI, main (end to end)
- [x] Task 9b: Sample scenarios shipped in packages (user request)
- [x] Task 10: install.sh, package scripts, release workflow (CI only)
- [x] Task 11: README

## Review

Executed subagent-driven on branch `feat/flowgen` (12 tasks incl. user-added 9b sample scenarios), each task reviewed (spec + quality); fix rounds on Tasks 2, 9b, 11; final whole-branch review (opus) → one fix wave (dry-run on TTY, `ips` dedup bound, datagram/rate/NaN validation, install.sh examples path, SIGTERM summary, release permissions) → re-review clean.

Verification (local): `cargo fmt --check`, `cargo clippy --all-targets --locked -D warnings`, `cargo test --locked` (78 tests), `sh scripts/test-install.sh`, shellcheck; SIGTERM summary checked manually. Packages are built only in GitHub Actions — release workflow not yet exercised (no remote).

Open before first release:
- Push to a GitHub remote, replace `<owner>/<repo>` in README, run `release` via workflow_dispatch once (validates cargo-deb/generate-rpm `--target` paths, arm64 musl, Docker verify jobs).
- Parked (accepted): summary "flows" includes failed sends; clap usage errors lack `flowgen:` prefix; MSRV 1.88 not CI-tested; `ips` bound may under-count by ≤2 per aggregated net.
