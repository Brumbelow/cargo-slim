use std::process::Command;

const NOT_OBJECT_FIXTURE: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/not-object.txt");

fn cargoslim() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cargoslim"))
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
    assert!(value["object"].is_null());
}

#[test]
fn inspect_rejects_missing_limit_value() {
    let output = cargoslim()
        .args(["inspect", "--limit"])
        .output()
        .expect("cargoslim should run");

    assert_eq!(output.status.code(), Some(2));

    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf-8");
    assert!(stderr.contains("--limit requires a value"));
}
