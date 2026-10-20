use std::process::{Command, Output};

fn flowgen(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_flowgen"))
        .args(args)
        .output()
        .unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

#[test]
fn version_flag() {
    let o = flowgen(&["--version"]);
    assert!(o.status.success());
    assert!(stdout(&o).contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn presets_list_and_show() {
    let o = flowgen(&["presets"]);
    assert!(o.status.success());
    let out = stdout(&o);
    for name in ["three-tier", "datacenter", "scan-demo"] {
        assert!(out.contains(name), "{out}");
    }
    let o = flowgen(&["presets", "--show", "scan-demo"]);
    assert!(stdout(&o).contains("[[scan]]"));
    let o = flowgen(&["presets", "--show", "nope"]);
    assert_eq!(o.status.code(), Some(2));
}

#[test]
fn ips_for_preset() {
    let o = flowgen(&["ips", "--preset", "three-tier"]);
    assert!(o.status.success(), "{}", stderr(&o));
    let lines: Vec<String> = stdout(&o).lines().map(String::from).collect();
    assert_eq!(lines.len(), 46);
    assert_eq!(lines[0], "10.1.0.10");
}

#[test]
fn dry_run_short() {
    let o = flowgen(&[
        "run",
        "--preset",
        "three-tier",
        "--dry-run",
        "--duration",
        "1s",
        "--no-progress",
        "--seed",
        "1",
    ]);
    assert!(o.status.success(), "{}", stderr(&o));
    assert!(stdout(&o).lines().count() > 10);
    assert!(stderr(&o).contains("summary:"));
}

#[test]
fn missing_collector_is_error_2() {
    let o = flowgen(&["run", "--preset", "three-tier", "--duration", "1s"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("flowgen: error:"), "{}", stderr(&o));
    assert!(stderr(&o).contains("collector"), "{}", stderr(&o));
}

#[test]
fn unknown_preset_and_bad_file_are_error_2() {
    let o = flowgen(&["run", "--preset", "nope", "--dry-run"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("unknown preset"));

    let dir = std::env::temp_dir().join(format!("flowgen-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("bad.toml");
    std::fs::write(
        &file,
        "[[rule]]\nname='m'\nsrc=['10.0.0.1']\ndst=['224.0.0.1']\nproto='udp'\ndst_ports=[5000]\n",
    )
    .unwrap();
    let o = flowgen(&["run", "-c", file.to_str().unwrap(), "--dry-run"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(stderr(&o).contains("multicast"), "{}", stderr(&o));
}

#[test]
fn config_and_preset_are_mutually_exclusive() {
    let o = flowgen(&["run", "-c", "x.toml", "--preset", "three-tier"]);
    assert!(!o.status.success());
    let o = flowgen(&["run", "--dry-run"]);
    assert!(!o.status.success());
}
