# Final fix wave report (F1-F8)

- F1: src/ui.rs `pick_mode(no_progress, quiet, dry_run)` wraps pure `select_mode(stderr_tty, stdout_tty, dry_run, no_progress, quiet)`; src/cli.rs:126 passes `args.dry_run`. Tests: tests/ui.rs `select_mode_matrix`, updated `mode_selection`.
- F2: src/pattern.rs `scenario_hosts` bound from `Ipv4Net::aggregate` of all nets + uncovered scan srcs, checked before enumeration. Test: `scenario_hosts_dedups_repeated_subnets_before_limit` (9 identical /24 -> 254); `scenario_hosts_respects_limit` still passes.
- F3: src/config.rs validate warning for max_datagram > 1472; examples/scenario.toml and README table row. Test: `large_datagram_warns_about_fragmentation`.
- F4: src/config.rs `MAX_FLOWS_PER_SEC` = 1_000_000 hard error; 5000 warning kept. Test in tests/config.rs error cases (1_000_001).
- F5: install.sh examples dir depends on basename (bin -> ../share/flowgen/examples, else $PREFIX/examples), non-fatal with warning. scripts/test-install.sh case 4 (`--prefix $work/opt`). README sample-location text updated.
- F6: Cargo.toml ctrlc `termination` feature (Cargo.lock needed no change; build/--locked pass).
- F7: is_finite checks for period_secs (sine), amplitude, burst_chance, burst_multiplier, burst_secs. Tests: NaN period_secs, NaN burst_multiplier, NaN amplitude, inf burst_secs.
- F8: release.yml workflow permissions contents: read; publish job contents: write.

## Verification
- cargo fmt --check: OK; clippy --all-targets --locked -D warnings: clean; cargo test --locked: all suites pass.
- sh -n install.sh: OK; sh scripts/test-install.sh: "install.sh tests passed"; shellcheck: clean.
- `cargo run -q -- run --preset three-tier --dry-run --duration 2s | head -3`:
  tcp 192.168.10.1:55346 -> 10.1.1.14:443 pkts=102 bytes=97308 (and 2 more flow lines)
- Under `script` (pty): header lines then plain flow lines, no bar escape sequences (cat -v shows only ^M line endings).
