use std::process::Command;

const NOT_OBJECT_FIXTURE: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/not-object.txt");
const CARGO_PROJECT_MANIFEST: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/cargo-project/Cargo.toml"
);
const WORKSPACE_MEMBER_MANIFEST: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/workspace/member/Cargo.toml"
);
const SUGGESTIONS_MANIFEST: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/suggestions/Cargo.toml"
);

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

fn suggestion_with_title<'a>(
    suggestions: &'a [serde_json::Value],
    title: &str,
) -> &'a serde_json::Value {
    suggestions
        .iter()
        .find(|suggestion| suggestion["title"] == title)
        .unwrap_or_else(|| panic!("missing suggestion {title:?} in {suggestions:#?}"))
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
    assert!(stdout.contains("inspect [--json] [--limit <n>] [--manifest-path <path>] <path>"));
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
fn inspect_reports_cargo_context_text() {
    let output = cargoslim()
        .args([
            "inspect",
            "--manifest-path",
            CARGO_PROJECT_MANIFEST,
            NOT_OBJECT_FIXTURE,
        ])
        .output()
        .expect("cargoslim should run");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf-8");
    assert!(stdout.contains("object: not recognized"));
    assert!(stdout.contains("cargo:"));
    assert!(stdout.contains("package: fixture-app 0.1.0 (edition 2021)"));
    assert!(stdout.contains("lockfile:"));
    assert!(stdout.contains("(2 packages)"));
    assert!(stdout.contains("opt-level: z"));
    assert!(stdout.contains("debug: false"));
    assert!(stdout.contains("lto: thin"));
    assert!(stdout.contains("codegen-units: 1"));
    assert!(stdout.contains("panic: abort"));
    assert!(stdout.contains("strip: symbols"));
}

#[test]
fn inspect_reports_cargo_context_json() {
    let output = cargoslim()
        .args([
            "inspect",
            "--json",
            "--manifest-path",
            CARGO_PROJECT_MANIFEST,
            NOT_OBJECT_FIXTURE,
        ])
        .output()
        .expect("cargoslim should run");

    assert!(output.status.success());

    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid json");
    let cargo = value["cargo"].as_object().expect("cargo should be present");

    assert_eq!(cargo["package"]["name"], "fixture-app");
    assert_eq!(cargo["package"]["version"], "0.1.0");
    assert_eq!(cargo["package"]["edition"], "2021");
    assert_eq!(cargo["release_profile"]["opt_level"], "z");
    assert_eq!(cargo["release_profile"]["debug"], false);
    assert_eq!(cargo["release_profile"]["lto"], "thin");
    assert_eq!(cargo["release_profile"]["codegen_units"], 1);
    assert_eq!(cargo["release_profile"]["panic"], "abort");
    assert_eq!(cargo["release_profile"]["strip"], "symbols");
    assert_eq!(cargo["lockfile"]["package_count"], 2);
    assert_eq!(cargo["lockfile"]["packages"][0]["name"], "fixture-app");
    assert_eq!(cargo["lockfile"]["packages"][1]["name"], "serde");
    assert!(value["suggestions"].as_array().unwrap().is_empty());
}

#[test]
fn inspect_reports_conservative_suggestions_text() {
    let output = cargoslim()
        .args([
            "inspect",
            "--manifest-path",
            SUGGESTIONS_MANIFEST,
            NOT_OBJECT_FIXTURE,
        ])
        .output()
        .expect("cargoslim should run");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf-8");
    assert!(stdout.contains("suggestions:"));
    assert!(stdout.contains("Remove release debug information"));
    assert!(stdout.contains("Strip symbols from release binaries"));
    assert!(stdout.contains("Review duplicate dependency versions"));
    assert!(stdout.contains("Audit direct dependency default features"));
    assert!(stdout.contains("confidence: high"));
    assert!(stdout.contains("Cargo.lock contains multiple versions for: duplicate (1.0.0, 2.0.0)."));
}

#[test]
fn inspect_reports_conservative_suggestions_json() {
    let output = cargoslim()
        .args([
            "inspect",
            "--json",
            "--manifest-path",
            SUGGESTIONS_MANIFEST,
            NOT_OBJECT_FIXTURE,
        ])
        .output()
        .expect("cargoslim should run");

    assert!(output.status.success());

    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid json");
    let suggestions = value["suggestions"]
        .as_array()
        .expect("suggestions should be an array");

    let debug = suggestion_with_title(suggestions, "Remove release debug information");
    assert_eq!(debug["confidence"], "high");
    assert_eq!(debug["evidence"], "[profile.release].debug is true.");

    let duplicates = suggestion_with_title(suggestions, "Review duplicate dependency versions");
    assert_eq!(duplicates["confidence"], "high");
    assert!(duplicates["evidence"]
        .as_str()
        .unwrap()
        .contains("duplicate (1.0.0, 2.0.0)"));

    let default_features =
        suggestion_with_title(suggestions, "Audit direct dependency default features");
    assert_eq!(default_features["confidence"], "low");
    assert_eq!(
        default_features["evidence"],
        "Direct dependencies with default features enabled or unspecified: regex, serde."
    );
}

#[test]
fn inspect_resolves_workspace_profile_context() {
    let output = cargoslim()
        .args([
            "inspect",
            "--json",
            "--manifest-path",
            WORKSPACE_MEMBER_MANIFEST,
            NOT_OBJECT_FIXTURE,
        ])
        .output()
        .expect("cargoslim should run");

    assert!(output.status.success());

    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout should be valid json");
    let cargo = value["cargo"].as_object().expect("cargo should be present");
    let package_root = cargo["package_root"].as_str().unwrap();
    let workspace_root = cargo["workspace_root"].as_str().unwrap();
    let profile_manifest_path = cargo["release_profile"]["profile_manifest_path"]
        .as_str()
        .unwrap();

    assert!(package_root.ends_with("/tests/fixtures/workspace/member"));
    assert!(workspace_root.ends_with("/tests/fixtures/workspace"));
    assert!(profile_manifest_path.ends_with("/tests/fixtures/workspace/Cargo.toml"));
    assert_eq!(cargo["package"]["name"], "workspace-member");
    assert_eq!(cargo["release_profile"]["opt_level"], 3);
    assert_eq!(cargo["release_profile"]["debug"], 1);
    assert_eq!(cargo["release_profile"]["lto"], false);
    assert_eq!(cargo["release_profile"]["codegen_units"], 8);
    assert_eq!(cargo["release_profile"]["strip"], true);
    assert_eq!(cargo["lockfile"]["package_count"], 1);
}

#[test]
fn inspect_rejects_missing_limit_value() {
    assert_inspect_error(&["inspect", "--limit"], "--limit requires a value");
}

#[test]
fn inspect_rejects_missing_manifest_path_value() {
    assert_inspect_error(
        &["inspect", "--manifest-path"],
        "--manifest-path requires a value",
    );
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
