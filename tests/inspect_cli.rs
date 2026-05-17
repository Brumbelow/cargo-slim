use std::process::Command;

const NOT_OBJECT_FIXTURE: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/not-object.txt");

fn cargoslim() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cargoslim"))
}

fn assert_inspect_error(args: &[&str], expected: &str) {
    let output = cargoslim()
        .args(args)
        .output()
        .expect("cargoslim should run");

    assert_eq!(output.status.code(), Some(2));

    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf-8");
    assert!(
        stderr.contains(expected),
        "expected {expected:?} in stderr: {stderr}"
    );
}

#[test]
fn help_lists_inspect_command() {
    let output = cargoslim()
        .arg("--help")
        .output()
        .expect("cargoslim should run");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf-8");
    assert!(stdout.contains("cargoslim"));
    assert!(stdout.contains("inspect [--json] [--limit <n>] <path>"));
}

#[test]
fn inspect_reports_text_for_binary() {
    let output = cargoslim()
        .args(["inspect", "--limit", "2", env!("CARGO_BIN_EXE_cargoslim")])
        .output()
        .expect("cargoslim should run");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf-8");
    assert!(stdout.contains("path:"));
    assert!(stdout.contains("size:"));
    assert!(stdout.contains("object:"));
    assert!(stdout.contains("sections:"));

    let section_lines = stdout
        .lines()
        .filter(|line| line.starts_with("  .") && line.contains(": "))
        .count();
    assert!(
        section_lines <= 2,
        "expected at most 2 sections in {stdout}"
    );
}

#[test]
fn inspect_reports_json_for_binary() {
    let output = cargoslim()
        .args([
            "inspect",
            "--json",
            "--limit=3",
            env!("CARGO_BIN_EXE_cargoslim"),
        ])
        .output()
        .expect("cargoslim should run");

    assert!(output.status.success());

    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid json");

    assert_eq!(value["path"], env!("CARGO_BIN_EXE_cargoslim"));
    assert!(value["file_size_bytes"].as_u64().unwrap() > 0);
    assert!(value.get("file_size_mib").is_none());

    let object = value["object"]
        .as_object()
        .expect("object should be present");
    assert!(
        object["total_sections"].as_u64().unwrap()
            >= object["sections"].as_array().unwrap().len() as u64
    );
    assert!(object["sections"].as_array().unwrap().len() <= 3);
}

#[test]
fn inspect_reports_unrecognized_text_file() {
    let output = cargoslim()
        .args(["inspect", NOT_OBJECT_FIXTURE])
        .output()
        .expect("cargoslim should run");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf-8");
    assert!(stdout.contains("path:"));
    assert!(stdout.contains("size:"));
    assert!(stdout.contains("object: not recognized"));
    assert!(!stdout.contains("sections:"));
}

#[test]
fn inspect_reports_unrecognized_json_file() {
    let output = cargoslim()
        .args(["inspect", "--json", NOT_OBJECT_FIXTURE])
        .output()
        .expect("cargoslim should run");

    assert!(output.status.success());

    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid json");

    assert_eq!(value["path"], NOT_OBJECT_FIXTURE);
    assert!(value["file_size_bytes"].as_u64().unwrap() > 0);
    assert!(value.get("file_size_mib").is_none());
    assert!(value["object"].is_null());
}

#[test]
fn inspect_rejects_missing_limit_value() {
    assert_inspect_error(&["inspect", "--limit"], "--limit requires a value");
}

#[test]
fn inspect_rejects_missing_path() {
    assert_inspect_error(&["inspect"], "inspect requires a path");
}

#[test]
fn inspect_rejects_duplicate_paths() {
    assert_inspect_error(
        &[
            "inspect",
            NOT_OBJECT_FIXTURE,
            env!("CARGO_BIN_EXE_cargoslim"),
        ],
        "inspect accepts exactly one path",
    );
}

#[test]
fn inspect_rejects_unknown_option() {
    assert_inspect_error(
        &["inspect", "--wat", NOT_OBJECT_FIXTURE],
        "unknown inspect option",
    );
}

#[test]
fn inspect_rejects_bad_limit_value() {
    assert_inspect_error(
        &["inspect", "--limit", "many", NOT_OBJECT_FIXTURE],
        "invalid --limit value 'many'",
    );
}

#[test]
fn inspect_rejects_zero_limit_value() {
    assert_inspect_error(
        &["inspect", "--limit=0", NOT_OBJECT_FIXTURE],
        "--limit must be greater than zero",
    );
}

#[test]
fn inspect_reports_nonexistent_file_error() {
    let missing_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/does-not-exist.bin"
    );
    let output = cargoslim()
        .args(["inspect", missing_path])
        .output()
        .expect("cargoslim should run");

    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf-8");
    assert!(stderr.contains("could not inspect"));
}

#[test]
fn inspect_rejects_directory_input() {
    let output = cargoslim()
        .args(["inspect", env!("CARGO_MANIFEST_DIR")])
        .output()
        .expect("cargoslim should run");

    assert_eq!(output.status.code(), Some(1));

    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf-8");
    assert!(stderr.contains("is not a file"));
}
