# Workloader CSV Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `flowgen ips --format workloader` emits a CSV that `workloader wkld-import <file> --umwl` turns into labelled unmanaged workloads in the PCE.

**Architecture:** Scenario TOML gains optional `[[host]]` tables that attach Illumio labels (and a hostname prefix) to networks. A new `src/inventory.rs` resolves each scenario IP to a hostname + merged labels (longest prefix wins per key) and writes the CSV. `cli.rs` gains `--format plain|workloader` on `ips`.

**Tech Stack:** Rust 2024, MSRV 1.88, serde/toml, ipnet, clap 4. No new dependencies.

**Spec:** Approved in-chat design (this plan restates it in full; there is no separate spec file).

## Global Constraints

- No new crates. CSV escaping is hand-written.
- `[[host]]` is optional; every existing scenario file must keep parsing unchanged.
- `flowgen ips` with no `--format` prints exactly what it prints today (one IP per line).
- `cargo fmt --check`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test --locked` must pass.
- Commit trailer, exactly: `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`
- Never build packages locally (no cargo-deb / cargo-generate-rpm / pkgbuild).

## Workloader facts (verified from brian1917/workloader `cmd/wkldimport`)

- Recognised columns: `href, hostname, name, interfaces, public_ip, ...description, ...`; any other header that equals an existing PCE label *type* key is a label column; unknown headers are ignored.
- Interfaces: `"10.1.1.9"` is valid; with no `name:` prefix the interface is named `umwl`. Multiple separated by `;`.
- Matching default: href, then hostname, then name. With `--umwl`, unmatched rows create unmanaged workloads.
- Missing label values are created; label types must already exist.
- Blank CSV cells do not change existing values.

## Review Focus

1. Overlapping `[[host]]` nets: a `/16` with `env` plus a `/29` with `role` must give the IP both labels; a `/29` setting `role` overrides a `/16` setting `role`.
2. Equal prefix length conflict: the later `[[host]]` in file order wins.
3. Label values containing commas or quotes must be CSV-quoted (`"a,b"`, `"say ""hi"""`).
4. Scenario with no `[[host]]` at all: CSV still lists every IP with hostname `fg-a-b-c-d` and empty label cells.
5. `--format workloader` respects the same 4096 limit / `--all` as plain.

---

### Task 1: `[[host]]` config, validation, label resolution

**Files:**
- Modify: `src/config.rs` (new `HostGroup` struct, `Scenario.hosts`, validation)
- Create: `src/inventory.rs` (resolution), register in `src/lib.rs`
- Test: `tests/inventory.rs` (new), `tests/config.rs` (validation cases)

**Interfaces — Produces:**
```rust
// src/config.rs
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostGroup {
    #[serde(deserialize_with = "de_nets")]
    pub nets: Vec<Ipv4Net>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
    #[serde(default)]
    pub hostname_prefix: Option<String>,
}
// in Scenario:
    #[serde(default, rename = "host")]
    pub hosts: Vec<HostGroup>,

// src/inventory.rs
pub const DEFAULT_HOSTNAME_PREFIX: &str = "fg";
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostInfo {
    pub ip: Ipv4Addr,
    pub hostname: String,                 // "{prefix}-{a}-{b}-{c}-{d}"
    pub labels: BTreeMap<String, String>,
}
pub fn resolve(groups: &[HostGroup], ip: Ipv4Addr) -> HostInfo;
```

**Resolution rules (exact):**
- Collect every (group index, matching net) where a net of the group contains `ip`. A group's specificity for this ip is the largest `prefix_len()` among its nets containing `ip`.
- Apply groups in ascending order of (specificity, group index); each applied group inserts all its labels (overwriting) and, if `hostname_prefix` is `Some`, overwrites the prefix. So the most specific wins per key, and at equal specificity the later group wins.
- Prefix starts as `DEFAULT_HOSTNAME_PREFIX`. Hostname = `format!("{prefix}-{}-{}-{}-{}", o[0], o[1], o[2], o[3])`.

**Validation (in `Scenario::validate`, errors via `bail!`), messages include the 1-based index as `[[host]] #N`:**
- `nets` empty → `"[[host]] #N: nets must not be empty"`
- any label key or value empty after `trim()` → `"[[host]] #N: label keys and values must not be empty"`
- `hostname_prefix` empty, or containing chars other than ASCII alphanumerics and `-`, or starting/ending with `-` → `"[[host]] #N: hostname_prefix must be letters, digits and inner '-'"`
- `[[host]]` alone does not satisfy the "no [[rule]] and no [[scan]]" check (unchanged).

- [ ] Step 1: Write failing tests in `tests/inventory.rs`:
  - no groups → hostname `fg-10-1-1-9`, empty labels
  - `/16 {app=shop, env=prod}` + `/29 {role=web}` prefix `web` → `web-10-1-1-9`, labels app/env/role all present
  - `/16 {role=any}` + `/29 {role=web}` → role `web` regardless of file order (test both orders)
  - two `/29` groups both setting `role` → later one wins
  - group with nets `["10.0.0.0/8", "10.1.1.8/29"]` has specificity 29 for 10.1.1.9
  - ip outside all groups → defaults
- [ ] Step 2: Write failing validation tests in `tests/config.rs` (empty nets, empty label value `" "`, bad prefix `"web_1"`, `"-web"`, good prefix `"web-1"`), and a parse test showing a TOML `[[host]]` round-trips into `Scenario.hosts`.
- [ ] Step 3: Implement; run `cargo test --locked`; all pass.
- [ ] Step 4: fmt + clippy; commit `feat: [[host]] label groups and host resolution`.

### Task 2: `ips --format plain|workloader`

**Files:**
- Modify: `src/inventory.rs` (CSV writer), `src/cli.rs` (`--format` flag)
- Test: `tests/inventory.rs`, `tests/cli.rs`

**Interfaces — Consumes:** Task 1 `resolve`, `HostInfo`, `Scenario.hosts`. **Produces:**
```rust
pub fn write_workloader_csv<W: std::io::Write>(
    w: &mut W,
    description: &str,
    hosts: &[HostInfo],
) -> std::io::Result<()>;
// cli.rs
#[derive(Clone, Copy, Debug, Default, clap::ValueEnum)]
enum IpsFormat { #[default] Plain, Workloader }
// Ips { source, all, #[arg(long, value_enum, default_value_t = IpsFormat::Plain)] format: IpsFormat }
```

**CSV format (exact):**
- Line ending `\n`. Header: `hostname,name,interfaces,description,role,app,env,loc` followed by every other label key used by any host, sorted ascending, comma-separated. The four standard label columns are always present.
- One row per host, input order: `hostname`, `name` (= hostname), `interfaces` (= the IP, no name prefix), `description`, then each label column's value or empty.
- Escaping: a field containing `,`, `"`, `\r` or `\n` is wrapped in `"` with inner `"` doubled; otherwise written raw.
- Description passed by cli: `format!("flowgen {name}")` when the scenario has a name, else `"flowgen"`.
- cli: load + validate scenario, `scenario_hosts(&scenario, limit)` (same limit logic as now), map each IP through `resolve(&scenario.hosts, ip)`, write to a locked buffered stdout.

- [ ] Step 1: Failing unit tests: header with no custom keys; header with custom keys `zone`, `owner` → `...,loc,owner,zone`; escaping of `a,b` and `say "hi"`; empty cells for missing labels; description escaped too.
- [ ] Step 2: Failing CLI tests in `tests/cli.rs`: `ips --preset three-tier --format workloader` → first line equals the standard header, line count = plain line count + 1; `ips --preset three-tier` output unchanged (still bare IPs); `--format bogus` exits non-zero.
- [ ] Step 3: Implement; tests pass; fmt + clippy; commit `feat: ips --format workloader CSV export`.

### Task 3: Labels for presets and sample scenarios

**Files:**
- Modify: `presets/three-tier.toml`, `presets/datacenter.toml`, `presets/scan-demo.toml`, `examples/scenario.toml` (annotated reference: document `[[host]]` with comments), and the 7 `examples/*.toml` samples.
- Test: `tests/examples.rs`, `tests/presets.rs`

**Rules for content:**
- Every IP that `scenario_hosts` emits for a preset/sample must resolve to a non-empty `role` and `app`, and `env` should be set (use broad nets for app/env/loc, specific nets for role).
- Use realistic, short lowercase label values (`web`, `app`, `db`, `dns`, `client`, `dc`, `plc`, ...); app = scenario-appropriate name; env `prod` unless the scenario implies otherwise; loc optional.
- `hostname_prefix` = role-ish short name on role groups.
- Do not change any rule, scan, rate or target content. Do not change IPs.
- Add `[[host]]` tables after the `[rate]`/`[target]` section and before the first `[[rule]]`, with a one-line comment header `# Labels for `flowgen ips --format workloader``.

- [ ] Step 1: Failing tests: for every preset and every sample (load-test via `scenario_hosts(&s, None)`), every emitted IP's `resolve` result has non-empty `role`, `app`, and `env`. Existing tests stay.
- [ ] Step 2: Add `[[host]]` content to all files; tests pass; fmt + clippy.
- [ ] Step 3: Commit `feat: label groups for presets and sample scenarios`.

### Task 4: Docs and version 0.2.0

**Files:** `docs/scenarios.md` (`[[host]]` reference: fields, merge rules, examples), `docs/quickstart.md` and `docs/demo-guide.md` (replace manual workload creation with the workloader step), `docs/troubleshooting.md` (label types must exist; hostname match/re-import updates), `README.md` (short mention + command), `Cargo.toml` + `Cargo.lock` version `0.2.0`.

Workloader command to document (exact):
```
flowgen ips --preset three-tier --format workloader > three-tier.csv
workloader wkld-import three-tier.csv --umwl            # dry run: logs what would change
workloader wkld-import three-tier.csv --umwl --update-pce --no-prompt
```
Note in docs: the label types used (role/app/env/loc by default, plus any custom keys) must exist in the PCE; label values are created automatically; re-running updates the same workloads because hostnames are derived from IPs; the `description` column is `flowgen <scenario>`.

- [ ] Step 1: Edit docs; verify every relative link target exists and anchors match headings.
- [ ] Step 2: Bump version (`cargo update -p flowgen --offline` or `cargo check` to refresh lock); `cargo test --locked` passes.
- [ ] Step 3: Commit `docs: workloader export; bump to 0.2.0`.
