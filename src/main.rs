use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use object::{Object, ObjectSection};
use serde::{Deserialize, Serialize};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    match Command::parse(env::args().skip(1)) {
        Ok(Command::Help) => {
            print_help();
            ExitCode::SUCCESS
        }
        Ok(Command::Version) => {
            println!("cargoslim {VERSION}");
            ExitCode::SUCCESS
        }
        Ok(Command::Inspect(options)) => {
            match inspect_path(&options.path, options.manifest_path.as_deref()) {
                Ok(mut report) => {
                    report.apply_section_limit(options.section_limit);

                    if options.output == OutputFormat::Json {
                        match serde_json::to_string_pretty(&report) {
                            Ok(json) => println!("{json}"),
                            Err(error) => {
                                eprintln!("error: could not serialize report: {error}");
                                return ExitCode::from(1);
                            }
                        }
                    } else {
                        print_text_report(&report);
                    }

                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("error: {error}");
                    ExitCode::from(1)
                }
            }
        }
        Err(error) => {
            eprintln!("error: {error}");
            eprintln!();
            print_help();
            ExitCode::from(2)
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
enum Command {
    Help,
    Version,
    Inspect(InspectOptions),
}

#[derive(Debug, Eq, PartialEq)]
struct InspectOptions {
    path: PathBuf,
    manifest_path: Option<PathBuf>,
    output: OutputFormat,
    section_limit: Option<usize>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum OutputFormat {
    #[default]
    Text,
    Json,
}

impl Command {
    fn parse<I, S>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut args = args.into_iter().map(Into::into);

        match args.next().as_deref() {
            None | Some("-h") | Some("--help") => Ok(Self::Help),
            Some("-V") | Some("--version") => Ok(Self::Version),
            Some("inspect") => parse_inspect(args),
            Some(command) => Err(format!("unknown command '{command}'")),
        }
    }
}

fn parse_inspect<I>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = String>,
{
    let mut output = OutputFormat::Text;
    let mut manifest_path = None;
    let mut section_limit = None;
    let mut path = None;
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => output = OutputFormat::Json,
            "--manifest-path" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--manifest-path requires a value".to_string())?;
                manifest_path = Some(PathBuf::from(value));
            }
            "--limit" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--limit requires a value".to_string())?;
                section_limit = Some(parse_section_limit(&value)?);
            }
            "-h" | "--help" => return Ok(Command::Help),
            _ if arg.starts_with("--limit=") => {
                let value = arg
                    .strip_prefix("--limit=")
                    .expect("argument should have --limit= prefix");
                section_limit = Some(parse_section_limit(value)?);
            }
            _ if arg.starts_with("--manifest-path=") => {
                let value = arg
                    .strip_prefix("--manifest-path=")
                    .expect("argument should have --manifest-path= prefix");
                manifest_path = Some(PathBuf::from(value));
            }
            _ if arg.starts_with('-') => return Err(format!("unknown inspect option '{arg}'")),
            _ if path.is_none() => path = Some(PathBuf::from(arg)),
            _ => return Err("inspect accepts exactly one path".to_string()),
        }
    }

    let path = path.ok_or_else(|| "inspect requires a path".to_string())?;
    Ok(Command::Inspect(InspectOptions {
        path,
        manifest_path,
        output,
        section_limit,
    }))
}

fn parse_section_limit(value: &str) -> Result<usize, String> {
    let limit = value
        .parse::<usize>()
        .map_err(|_| format!("invalid --limit value '{value}'"))?;

    if limit == 0 {
        return Err("--limit must be greater than zero".to_string());
    }

    Ok(limit)
}

#[derive(Debug, Serialize)]
struct InspectReport {
    path: String,
    file_size_bytes: u64,
    object: Option<ObjectReport>,
    cargo: Option<CargoReport>,
}

#[derive(Debug, Serialize)]
struct ObjectReport {
    format: String,
    architecture: String,
    endianness: String,
    entry: u64,
    has_debug_symbols: bool,
    total_sections: usize,
    sections_omitted: usize,
    sections: Vec<SectionReport>,
}

#[derive(Debug, Serialize)]
struct SectionReport {
    name: String,
    address: u64,
    size_bytes: u64,
}

#[derive(Debug, Serialize)]
struct CargoReport {
    manifest_path: String,
    package_root: String,
    workspace_root: String,
    package: Option<PackageReport>,
    release_profile: ReleaseProfileReport,
    lockfile: Option<LockfileReport>,
}

#[derive(Debug, Serialize)]
struct PackageReport {
    name: String,
    version: String,
    edition: Option<String>,
}

#[derive(Debug, Serialize)]
struct ReleaseProfileReport {
    profile_manifest_path: String,
    opt_level: Option<ProfileValue>,
    debug: Option<ProfileValue>,
    lto: Option<ProfileValue>,
    codegen_units: Option<u64>,
    panic: Option<String>,
    strip: Option<ProfileValue>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(untagged)]
enum ProfileValue {
    Bool(bool),
    Integer(i64),
    String(String),
}

#[derive(Debug, Serialize)]
struct LockfileReport {
    path: String,
    package_count: usize,
    packages: Vec<LockfilePackageReport>,
}

#[derive(Debug, Serialize)]
struct LockfilePackageReport {
    name: String,
    version: String,
    source: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CargoManifest {
    package: Option<ManifestPackage>,
    profile: Option<ManifestProfiles>,
}

#[derive(Debug, Deserialize)]
struct ManifestPackage {
    name: String,
    version: String,
    edition: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ManifestProfiles {
    release: Option<ManifestReleaseProfile>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct ManifestReleaseProfile {
    opt_level: Option<toml::Value>,
    debug: Option<toml::Value>,
    lto: Option<toml::Value>,
    codegen_units: Option<u64>,
    panic: Option<String>,
    strip: Option<toml::Value>,
}

fn inspect_path(path: &Path, manifest_path: Option<&Path>) -> Result<InspectReport, String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("could not inspect '{}': {error}", path.display()))?;

    if !metadata.is_file() {
        return Err(format!("'{}' is not a file", path.display()));
    }

    let bytes =
        fs::read(path).map_err(|error| format!("could not read '{}': {error}", path.display()))?;
    let object = object::File::parse(bytes.as_slice())
        .ok()
        .map(read_object_report);
    let cargo = manifest_path.map(read_cargo_report).transpose()?;

    Ok(InspectReport {
        path: path.display().to_string(),
        file_size_bytes: metadata.len(),
        object,
        cargo,
    })
}

fn read_cargo_report(manifest_path: &Path) -> Result<CargoReport, String> {
    let manifest_path = normalize_manifest_path(manifest_path)?;
    let package_root = manifest_path
        .parent()
        .ok_or_else(|| format!("'{}' has no parent directory", manifest_path.display()))?
        .to_path_buf();
    let package_manifest = read_cargo_manifest(&manifest_path)?;
    let workspace_root = find_workspace_root(&package_root)?;
    let profile_manifest_path = workspace_root.join("Cargo.toml");
    let profile_manifest_storage = if profile_manifest_path == manifest_path {
        None
    } else {
        Some(read_cargo_manifest(&profile_manifest_path)?)
    };
    let profile_manifest = profile_manifest_storage
        .as_ref()
        .unwrap_or(&package_manifest);
    let release_profile = ReleaseProfileReport::new(
        &profile_manifest_path,
        profile_manifest
            .profile
            .as_ref()
            .and_then(|profile| profile.release.as_ref()),
    );
    let lockfile = read_lockfile_report(&workspace_root)?;

    Ok(CargoReport {
        manifest_path: manifest_path.display().to_string(),
        package_root: package_root.display().to_string(),
        workspace_root: workspace_root.display().to_string(),
        package: package_manifest.package.map(PackageReport::from),
        release_profile,
        lockfile,
    })
}

fn normalize_manifest_path(manifest_path: &Path) -> Result<PathBuf, String> {
    let metadata = fs::metadata(manifest_path).map_err(|error| {
        format!(
            "could not inspect manifest '{}': {error}",
            manifest_path.display()
        )
    })?;

    if !metadata.is_file() {
        return Err(format!(
            "manifest path '{}' is not a file",
            manifest_path.display()
        ));
    }

    fs::canonicalize(manifest_path).map_err(|error| {
        format!(
            "could not resolve manifest '{}': {error}",
            manifest_path.display()
        )
    })
}

fn read_cargo_manifest(path: &Path) -> Result<CargoManifest, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("could not read manifest '{}': {error}", path.display()))?;

    toml::from_str(&text)
        .map_err(|error| format!("could not parse manifest '{}': {error}", path.display()))
}

fn find_workspace_root(package_root: &Path) -> Result<PathBuf, String> {
    let mut current = Some(package_root);

    while let Some(dir) = current {
        let candidate = dir.join("Cargo.toml");

        if candidate.is_file() && manifest_declares_workspace(&candidate)? {
            return Ok(dir.to_path_buf());
        }

        current = dir.parent();
    }

    Ok(package_root.to_path_buf())
}

fn manifest_declares_workspace(path: &Path) -> Result<bool, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("could not read manifest '{}': {error}", path.display()))?;
    let value: toml::Value = toml::from_str(&text)
        .map_err(|error| format!("could not parse manifest '{}': {error}", path.display()))?;

    Ok(value.get("workspace").is_some())
}

fn read_lockfile_report(workspace_root: &Path) -> Result<Option<LockfileReport>, String> {
    let path = workspace_root.join("Cargo.lock");

    if !path.is_file() {
        return Ok(None);
    }

    let text = fs::read_to_string(&path)
        .map_err(|error| format!("could not read lockfile '{}': {error}", path.display()))?;
    let value: toml::Value = toml::from_str(&text)
        .map_err(|error| format!("could not parse lockfile '{}': {error}", path.display()))?;
    let packages = value
        .get("package")
        .and_then(toml::Value::as_array)
        .map(|packages| {
            packages
                .iter()
                .filter_map(LockfilePackageReport::from_toml)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Ok(Some(LockfileReport {
        path: path.display().to_string(),
        package_count: packages.len(),
        packages,
    }))
}

impl From<ManifestPackage> for PackageReport {
    fn from(package: ManifestPackage) -> Self {
        Self {
            name: package.name,
            version: package.version,
            edition: package.edition,
        }
    }
}

impl ReleaseProfileReport {
    fn new(path: &Path, profile: Option<&ManifestReleaseProfile>) -> Self {
        let Some(profile) = profile else {
            return Self {
                profile_manifest_path: path.display().to_string(),
                opt_level: None,
                debug: None,
                lto: None,
                codegen_units: None,
                panic: None,
                strip: None,
            };
        };

        Self {
            profile_manifest_path: path.display().to_string(),
            opt_level: profile.opt_level.as_ref().and_then(ProfileValue::from_toml),
            debug: profile.debug.as_ref().and_then(ProfileValue::from_toml),
            lto: profile.lto.as_ref().and_then(ProfileValue::from_toml),
            codegen_units: profile.codegen_units,
            panic: profile.panic.clone(),
            strip: profile.strip.as_ref().and_then(ProfileValue::from_toml),
        }
    }

    fn has_explicit_settings(&self) -> bool {
        self.opt_level.is_some()
            || self.debug.is_some()
            || self.lto.is_some()
            || self.codegen_units.is_some()
            || self.panic.is_some()
            || self.strip.is_some()
    }
}

impl ProfileValue {
    fn from_toml(value: &toml::Value) -> Option<Self> {
        match value {
            toml::Value::Boolean(value) => Some(Self::Bool(*value)),
            toml::Value::Integer(value) => Some(Self::Integer(*value)),
            toml::Value::String(value) => Some(Self::String(value.clone())),
            _ => None,
        }
    }
}

impl fmt::Display for ProfileValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bool(value) => write!(formatter, "{value}"),
            Self::Integer(value) => write!(formatter, "{value}"),
            Self::String(value) => write!(formatter, "{value}"),
        }
    }
}

impl LockfilePackageReport {
    fn from_toml(value: &toml::Value) -> Option<Self> {
        let package = value.as_table()?;

        Some(Self {
            name: package.get("name")?.as_str()?.to_string(),
            version: package.get("version")?.as_str()?.to_string(),
            source: package
                .get("source")
                .and_then(toml::Value::as_str)
                .map(str::to_string),
        })
    }
}

impl InspectReport {
    fn apply_section_limit(&mut self, limit: Option<usize>) {
        let Some(limit) = limit else {
            return;
        };

        let Some(object) = &mut self.object else {
            return;
        };

        if object.sections.len() <= limit {
            return;
        }

        object.sections_omitted = object.sections.len() - limit;
        object.sections.truncate(limit);
    }
}

fn read_object_report(file: object::File<'_>) -> ObjectReport {
    let mut sections = file
        .sections()
        .filter_map(|section| {
            let size = section.size();

            if size == 0 {
                return None;
            }

            Some(SectionReport {
                name: section.name().unwrap_or("<unnamed>").to_string(),
                address: section.address(),
                size_bytes: size,
            })
        })
        .collect::<Vec<_>>();

    sections.sort_by(|left, right| {
        right
            .size_bytes
            .cmp(&left.size_bytes)
            .then_with(|| left.name.cmp(&right.name))
    });

    ObjectReport {
        format: format!("{:?}", file.format()),
        architecture: format!("{:?}", file.architecture()),
        endianness: format!("{:?}", file.endianness()),
        entry: file.entry(),
        has_debug_symbols: has_debug_sections(&sections),
        total_sections: sections.len(),
        sections_omitted: 0,
        sections,
    }
}

fn has_debug_sections(sections: &[SectionReport]) -> bool {
    sections.iter().any(|section| {
        section.name.starts_with(".debug_")
            || section.name.starts_with("__debug_")
            || section.name == ".zdebug"
    })
}

fn print_text_report(report: &InspectReport) {
    println!("path: {}", report.path);
    println!(
        "size: {} bytes ({:.2} MiB)",
        report.file_size_bytes,
        bytes_to_mib(report.file_size_bytes)
    );

    let Some(object) = &report.object else {
        println!("object: not recognized");
        if let Some(cargo) = &report.cargo {
            print_cargo_report(cargo);
        }
        return;
    };

    println!("object: {}", object.format);
    println!("architecture: {}", object.architecture);
    println!("endianness: {}", object.endianness);
    println!("entry: 0x{:x}", object.entry);
    println!("debug symbols: {}", yes_no(object.has_debug_symbols));

    if object.sections.is_empty() {
        println!("sections: none reported");
    } else {
        println!("sections:");
        for section in &object.sections {
            println!(
                "  {}: {} bytes at 0x{:x}",
                section.name, section.size_bytes, section.address
            );
        }

        if object.sections_omitted > 0 {
            println!("  ... {} more sections omitted", object.sections_omitted);
        }
    }

    if let Some(cargo) = &report.cargo {
        print_cargo_report(cargo);
    }
}

fn print_cargo_report(cargo: &CargoReport) {
    println!("cargo:");
    println!("  manifest: {}", cargo.manifest_path);
    println!("  package root: {}", cargo.package_root);
    println!("  workspace root: {}", cargo.workspace_root);

    if let Some(package) = &cargo.package {
        match &package.edition {
            Some(edition) => println!(
                "  package: {} {} (edition {})",
                package.name, package.version, edition
            ),
            None => println!("  package: {} {}", package.name, package.version),
        }
    }

    match &cargo.lockfile {
        Some(lockfile) => println!(
            "  lockfile: {} ({} packages)",
            lockfile.path, lockfile.package_count
        ),
        None => println!("  lockfile: not found"),
    }

    print_release_profile(&cargo.release_profile);
}

fn print_release_profile(profile: &ReleaseProfileReport) {
    println!("  release profile: {}", profile.profile_manifest_path);

    if !profile.has_explicit_settings() {
        println!("    no explicit [profile.release] settings");
        return;
    }

    if let Some(value) = &profile.opt_level {
        println!("    opt-level: {value}");
    }
    if let Some(value) = &profile.debug {
        println!("    debug: {value}");
    }
    if let Some(value) = &profile.lto {
        println!("    lto: {value}");
    }
    if let Some(value) = profile.codegen_units {
        println!("    codegen-units: {value}");
    }
    if let Some(value) = &profile.panic {
        println!("    panic: {value}");
    }
    if let Some(value) = &profile.strip {
        println!("    strip: {value}");
    }
}

fn bytes_to_mib(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

fn print_help() {
    println!(
        "cargoslim {VERSION}

Explain Rust binary size and produce conservative shrink plans.

Usage:
  cargoslim inspect [--json] [--limit <n>] [--manifest-path <path>] <path>
  cargoslim --help
  cargoslim --version

Commands:
  inspect [--json] [--limit <n>] [--manifest-path <path>] <path>
    Report file size, object section sizes, and optional Cargo context"
    );
}

#[cfg(test)]
mod tests {
    use super::{bytes_to_mib, inspect_path, Command, InspectOptions, OutputFormat};
    use std::path::PathBuf;

    #[test]
    fn converts_bytes_to_mib() {
        assert_eq!(bytes_to_mib(1_048_576), 1.0);
    }

    #[test]
    fn parses_inspect_defaults_to_text() {
        let command = Command::parse(["inspect", "target/release/app"]).unwrap();

        assert_eq!(
            command,
            Command::Inspect(InspectOptions {
                path: PathBuf::from("target/release/app"),
                manifest_path: None,
                output: OutputFormat::Text,
                section_limit: None,
            })
        );
    }

    #[test]
    fn parses_inspect_json_flag() {
        let command = Command::parse(["inspect", "--json", "target/release/app"]).unwrap();

        assert_eq!(
            command,
            Command::Inspect(InspectOptions {
                path: PathBuf::from("target/release/app"),
                manifest_path: None,
                output: OutputFormat::Json,
                section_limit: None,
            })
        );
    }

    #[test]
    fn parses_inspect_section_limit() {
        let command = Command::parse(["inspect", "--limit", "5", "target/release/app"]).unwrap();

        assert_eq!(
            command,
            Command::Inspect(InspectOptions {
                path: PathBuf::from("target/release/app"),
                manifest_path: None,
                output: OutputFormat::Text,
                section_limit: Some(5),
            })
        );
    }

    #[test]
    fn parses_inspect_section_limit_equals_form() {
        let command = Command::parse(["inspect", "--limit=8", "target/release/app"]).unwrap();

        assert_eq!(
            command,
            Command::Inspect(InspectOptions {
                path: PathBuf::from("target/release/app"),
                manifest_path: None,
                output: OutputFormat::Text,
                section_limit: Some(8),
            })
        );
    }

    #[test]
    fn parses_inspect_manifest_path() {
        let command = Command::parse([
            "inspect",
            "--manifest-path",
            "Cargo.toml",
            "target/release/app",
        ])
        .unwrap();

        assert_eq!(
            command,
            Command::Inspect(InspectOptions {
                path: PathBuf::from("target/release/app"),
                manifest_path: Some(PathBuf::from("Cargo.toml")),
                output: OutputFormat::Text,
                section_limit: None,
            })
        );
    }

    #[test]
    fn parses_inspect_manifest_path_equals_form() {
        let command = Command::parse([
            "inspect",
            "--manifest-path=Cargo.toml",
            "target/release/app",
        ])
        .unwrap();

        assert_eq!(
            command,
            Command::Inspect(InspectOptions {
                path: PathBuf::from("target/release/app"),
                manifest_path: Some(PathBuf::from("Cargo.toml")),
                output: OutputFormat::Text,
                section_limit: None,
            })
        );
    }

    #[test]
    fn rejects_zero_section_limit() {
        let error = Command::parse(["inspect", "--limit", "0", "target/release/app"]).unwrap_err();

        assert_eq!(error, "--limit must be greater than zero");
    }

    #[test]
    fn rejects_missing_manifest_path_value() {
        let error = Command::parse(["inspect", "--manifest-path"]).unwrap_err();

        assert_eq!(error, "--manifest-path requires a value");
    }

    #[test]
    fn rejects_duplicate_inspect_paths() {
        let error = Command::parse(["inspect", "a", "b"]).unwrap_err();

        assert_eq!(error, "inspect accepts exactly one path");
    }

    #[test]
    fn inspects_current_test_binary_as_object() {
        let report = inspect_path(std::env::current_exe().unwrap().as_path(), None).unwrap();
        let object = report
            .object
            .expect("test binary should parse as an object");

        assert!(report.file_size_bytes > 0);
        assert!(!object.format.is_empty());
        assert!(!object.architecture.is_empty());
        assert!(!object.sections.is_empty());
        assert!(object.total_sections >= object.sections.len());
    }
}
