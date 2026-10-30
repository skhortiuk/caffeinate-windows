//! End-to-end checks against the built binary. These actually call
//! `SetThreadExecutionState`, so they only run on Windows.

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_caffeinate"))
}

#[test]
fn help_exits_zero() {
    let out = bin().arg("--help").output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("usage: caffeinate"));
}

#[test]
fn version_matches_cargo() {
    let out = bin().arg("--version").output().unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!("caffeinate {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn timeout_zero_holds_and_exits_zero() {
    let out = bin().args(["-di", "-t", "0"]).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn utility_exit_code_passes_through() {
    let out = bin().args(["-i", "cmd", "/c", "exit 7"]).output().unwrap();
    assert_eq!(out.status.code(), Some(7));
}

#[test]
fn bad_flag_exits_two() {
    let out = bin().arg("-x").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown option '-x'"));
}

#[test]
fn wait_and_utility_conflict() {
    let out = bin().args(["-w", "1", "cmd"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}
