use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode};

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
        Ok(Command::Attribution(options)) => match crate_attribution(&options) {
            Ok(mut report) => {
                report.apply_crate_limit(options.crate_limit);

                if options.output == OutputFormat::Json {
                    match serde_json::to_string_pretty(&report) {
                        Ok(json) => println!("{json}"),
                        Err(error) => {
                            eprintln!("error: could not serialize report: {error}");
                            return ExitCode::from(1);
                        }
                    }
                } else {
                    print_attribution_report(&report);
                }

                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::from(1)
            }
        },
        Ok(Command::Diff(options)) => match diff_paths(&options.old_path, &options.new_path) {
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
                    print_diff_report(&report);
                }

                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("error: {error}");
                ExitCode::from(1)
            }
        },
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
    Attribution(AttributionOptions),
    Diff(DiffOptions),
    Inspect(InspectOptions),
}

#[derive(Debug, Eq, PartialEq)]
struct AttributionOptions {
    manifest_path: PathBuf,
    binary_name: Option<String>,
    output: OutputFormat,
    crate_limit: Option<usize>,
}

#[derive(Debug, Eq, PartialEq)]
struct DiffOptions {
    old_path: PathBuf,
    new_path: PathBuf,
    output: OutputFormat,
    section_limit: Option<usize>,
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
            Some("attribution") => parse_attribution(args),
            Some("diff") => parse_diff(args),
            Some("inspect") => parse_inspect(args),
            Some(command) => Err(format!("unknown command '{command}'")),
        }
    }
}

fn parse_attribution<I>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = String>,
{
    let mut output = OutputFormat::Text;
    let mut crate_limit = Some(20);
    let mut manifest_path = PathBuf::from("Cargo.toml");
    let mut binary_name = None;
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => output = OutputFormat::Json,
            "--bin" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--bin requires a value".to_string())?;
                binary_name = Some(value);
            }
            "--manifest-path" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--manifest-path requires a value".to_string())?;
                manifest_path = PathBuf::from(value);
            }
            "--limit" => {
                let value = args
                    .next()
                    .ok_or_else(|| "--limit requires a value".to_string())?;
                crate_limit = Some(parse_section_limit(&value)?);
            }
            "-h" | "--help" => return Ok(Command::Help),
            _ if arg.starts_with("--bin=") => {
                let value = arg
                    .strip_prefix("--bin=")
                    .expect("argument should have --bin= prefix");
                binary_name = Some(value.to_string());
            }
            _ if arg.starts_with("--limit=") => {
                let value = arg
                    .strip_prefix("--limit=")
                    .expect("argument should have --limit= prefix");
                crate_limit = Some(parse_section_limit(value)?);
            }
            _ if arg.starts_with("--manifest-path=") => {
                let value = arg
                    .strip_prefix("--manifest-path=")
                    .expect("argument should have --manifest-path= prefix");
                manifest_path = PathBuf::from(value);
            }
            _ if arg.starts_with('-') => return Err(format!("unknown attribution option '{arg}'")),
            _ => return Err("attribution does not accept positional paths".to_string()),
        }
    }

    Ok(Command::Attribution(AttributionOptions {
        manifest_path,
        binary_name,
        output,
        crate_limit,
    }))
}

fn parse_diff<I>(args: I) -> Result<Command, String>
where
    I: IntoIterator<Item = String>,
{
    let mut output = OutputFormat::Text;
    let mut section_limit = None;
    let mut paths = Vec::new();
    let mut args = args.into_iter();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--json" => output = OutputFormat::Json,
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
            _ if arg.starts_with('-') => return Err(format!("unknown diff option '{arg}'")),
            _ if paths.len() < 2 => paths.push(PathBuf::from(arg)),
            _ => return Err("diff accepts exactly two paths".to_string()),
        }
    }

    if paths.len() != 2 {
        return Err("diff requires two paths".to_string());
    }

    let new_path = paths.pop().expect("new path should be present");
    let old_path = paths.pop().expect("old path should be present");
    Ok(Command::Diff(DiffOptions {
        old_path,
        new_path,
        output,
        section_limit,
    }))
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
struct BinaryReport {
    path: String,
    file_size_bytes: u64,
    object: Option<ObjectReport>,
}

#[derive(Debug, Serialize)]
struct InspectReport {
    path: String,
    file_size_bytes: u64,
    object: Option<ObjectReport>,
    cargo: Option<CargoReport>,
    suggestions: Vec<SuggestionReport>,
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
struct DiffReport {
    old: DiffBinaryReport,
    new: DiffBinaryReport,
    file_size_delta_bytes: i64,
    total_section_deltas: usize,
    section_deltas_omitted: usize,
    section_deltas: Vec<SectionDeltaReport>,
}

#[derive(Debug, Serialize)]
struct AttributionReport {
    manifest_path: String,
    package_root: String,
    tool: String,
    scope: String,
    file_size_bytes: u64,
    text_section_size_bytes: u64,
    total_crates: usize,
    crates_omitted: usize,
    crates: Vec<CrateAttributionReport>,
}

#[derive(Debug, Serialize)]
struct CrateAttributionReport {
    name: String,
    size_bytes: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct CargoBloatCrateOutput {
    file_size: u64,
    text_section_size: u64,
    crates: Vec<CargoBloatCrate>,
}

#[derive(Debug, Deserialize)]
struct CargoBloatCrate {
    name: String,
    size: u64,
}

#[derive(Debug, Serialize)]
struct DiffBinaryReport {
    path: String,
    file_size_bytes: u64,
    object: Option<DiffObjectReport>,
}

#[derive(Debug, Serialize)]
struct DiffObjectReport {
    format: String,
    architecture: String,
    endianness: String,
    entry: u64,
    has_debug_symbols: bool,
    total_sections: usize,
}

#[derive(Debug, Serialize)]
struct SectionDeltaReport {
    name: String,
    old_size_bytes: u64,
    new_size_bytes: u64,
    delta_bytes: i64,
    status: SectionDeltaStatus,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "lowercase")]
enum SectionDeltaStatus {
    Added,
    Removed,
    Changed,
}

#[derive(Debug, Serialize)]
struct CargoReport {
    manifest_path: String,
    package_root: String,
    workspace_root: String,
    package: Option<PackageReport>,
    release_profile: ReleaseProfileReport,
    lockfile: Option<LockfileReport>,
    direct_dependencies: Vec<ManifestDependencyReport>,
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

#[derive(Debug, Serialize)]
struct ManifestDependencyReport {
    name: String,
    default_features: bool,
}

#[derive(Debug, Serialize)]
struct SuggestionReport {
    title: String,
    confidence: SuggestionConfidence,
    evidence: String,
    tradeoff: String,
    action: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum SuggestionConfidence {
    High,
    Medium,
    Low,
}

#[derive(Debug, Deserialize)]
struct CargoManifest {
    package: Option<ManifestPackage>,
    profile: Option<ManifestProfiles>,
    dependencies: Option<BTreeMap<String, toml::Value>>,
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
    let binary = read_binary_report(path)?;
    let cargo = manifest_path.map(read_cargo_report).transpose()?;
    let suggestions = build_suggestions(binary.object.as_ref(), cargo.as_ref());

    Ok(InspectReport {
        path: binary.path,
        file_size_bytes: binary.file_size_bytes,
        object: binary.object,
        cargo,
        suggestions,
    })
}

fn diff_paths(old_path: &Path, new_path: &Path) -> Result<DiffReport, String> {
    let old = read_binary_report(old_path)?;
    let new = read_binary_report(new_path)?;
    let section_deltas = diff_sections(old.object.as_ref(), new.object.as_ref());
    let total_section_deltas = section_deltas.len();
    let file_size_delta_bytes = byte_delta(old.file_size_bytes, new.file_size_bytes);
    let old = DiffBinaryReport::from_binary(&old);
    let new = DiffBinaryReport::from_binary(&new);

    Ok(DiffReport {
        old,
        new,
        file_size_delta_bytes,
        total_section_deltas,
        section_deltas_omitted: 0,
        section_deltas,
    })
}

fn crate_attribution(options: &AttributionOptions) -> Result<AttributionReport, String> {
    let manifest_path = normalize_manifest_path(&options.manifest_path)?;
    let package_root = manifest_path
        .parent()
        .ok_or_else(|| format!("'{}' has no parent directory", manifest_path.display()))?
        .to_path_buf();
    let cargo_bloat = env::var("CARGOSLIM_CARGO_BLOAT").unwrap_or_else(|_| "cargo".to_string());
    let mut command = ProcessCommand::new(&cargo_bloat);

    if cargo_bloat == "cargo" {
        command.arg("bloat");
    }

    command
        .current_dir(&package_root)
        .arg("--release")
        .arg("--crates")
        .arg("--message-format")
        .arg("json")
        .arg("-n")
        .arg("0");

    if let Some(binary_name) = &options.binary_name {
        command.arg("--bin").arg(binary_name);
    }

    let output = command
        .output()
        .map_err(|error| format!("could not run cargo-bloat: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            return Err("cargo-bloat failed without diagnostic output".to_string());
        }
        return Err(format!("cargo-bloat failed: {stderr}"));
    }

    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("cargo-bloat output was not utf-8: {error}"))?;
    let parsed: CargoBloatCrateOutput = serde_json::from_str(stdout.trim())
        .map_err(|error| format!("could not parse cargo-bloat JSON output: {error}"))?;
    let crates = parsed
        .crates
        .into_iter()
        .map(|krate| CrateAttributionReport {
            name: krate.name,
            size_bytes: krate.size,
        })
        .collect::<Vec<_>>();
    let total_crates = crates.len();

    Ok(AttributionReport {
        manifest_path: manifest_path.display().to_string(),
        package_root: package_root.display().to_string(),
        tool: "cargo-bloat".to_string(),
        scope: ".text section crate attribution from cargo-bloat --crates".to_string(),
        file_size_bytes: parsed.file_size,
        text_section_size_bytes: parsed.text_section_size,
        total_crates,
        crates_omitted: 0,
        crates,
    })
}

fn read_binary_report(path: &Path) -> Result<BinaryReport, String> {
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

    Ok(BinaryReport {
        path: path.display().to_string(),
        file_size_bytes: metadata.len(),
        object,
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
        direct_dependencies: read_direct_dependencies(&package_manifest),
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

fn read_direct_dependencies(manifest: &CargoManifest) -> Vec<ManifestDependencyReport> {
    let Some(dependencies) = &manifest.dependencies else {
        return Vec::new();
    };

    dependencies
        .iter()
        .filter_map(|(name, value)| ManifestDependencyReport::from_toml(name, value))
        .collect()
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

impl ManifestDependencyReport {
    fn from_toml(name: &str, value: &toml::Value) -> Option<Self> {
        let default_features = match value {
            toml::Value::String(_) => true,
            toml::Value::Table(table) => {
                if table
                    .get("workspace")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(false)
                {
                    return None;
                }

                table
                    .get("default-features")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(true)
            }
            _ => return None,
        };

        Some(Self {
            name: name.to_string(),
            default_features,
        })
    }
}

fn build_suggestions(
    object: Option<&ObjectReport>,
    cargo: Option<&CargoReport>,
) -> Vec<SuggestionReport> {
    let Some(cargo) = cargo else {
        return Vec::new();
    };

    let mut suggestions = Vec::new();
    add_profile_suggestions(&mut suggestions, object, &cargo.release_profile);
    add_duplicate_dependency_suggestions(&mut suggestions, cargo.lockfile.as_ref());
    add_default_feature_suggestions(&mut suggestions, &cargo.direct_dependencies);
    suggestions.sort_by_key(|suggestion| suggestion.confidence.rank());
    suggestions
}

fn add_profile_suggestions(
    suggestions: &mut Vec<SuggestionReport>,
    object: Option<&ObjectReport>,
    profile: &ReleaseProfileReport,
) {
    let has_debug_sections = object
        .map(|object| object.has_debug_symbols)
        .unwrap_or(false);
    let debug_enabled = profile
        .debug
        .as_ref()
        .map(profile_value_enabled)
        .unwrap_or(false);

    if has_debug_sections || debug_enabled {
        suggestions.push(SuggestionReport {
            title: "Remove release debug information".to_string(),
            confidence: SuggestionConfidence::High,
            evidence: match (has_debug_sections, &profile.debug) {
                (true, Some(value)) => format!(
                    "The inspected object has debug sections and [profile.release].debug is {value}."
                ),
                (true, None) => "The inspected object has debug sections.".to_string(),
                (false, Some(value)) => format!("[profile.release].debug is {value}."),
                (false, None) => unreachable!("debug evidence requires profile or object evidence"),
            },
            tradeoff: "Disabling debug info reduces symbol detail in release artifacts; keep separate debuginfo if production debugging needs it.".to_string(),
            action: "Set [profile.release] debug = false for size-focused release builds and compare the binary.".to_string(),
        });
    }

    if profile
        .strip
        .as_ref()
        .map(profile_value_disabled)
        .unwrap_or(true)
    {
        suggestions.push(SuggestionReport {
            title: "Strip symbols from release binaries".to_string(),
            confidence: SuggestionConfidence::Medium,
            evidence: profile_setting_evidence("strip", profile.strip.as_ref(), "is not set"),
            tradeoff: "Stripping symbols makes ad hoc debugging harder unless symbols are preserved separately.".to_string(),
            action: "Set [profile.release] strip = \"symbols\" or strip = true, then inspect the resulting binary.".to_string(),
        });
    }

    if profile
        .lto
        .as_ref()
        .map(profile_value_disabled)
        .unwrap_or(true)
    {
        suggestions.push(SuggestionReport {
            title: "Compare release builds with LTO enabled".to_string(),
            confidence: SuggestionConfidence::Medium,
            evidence: profile_setting_evidence("lto", profile.lto.as_ref(), "is not set"),
            tradeoff: "LTO often increases release build time and can change optimization behavior, so measure the result.".to_string(),
            action: "Try [profile.release] lto = \"thin\" first, then compare size and runtime behavior.".to_string(),
        });
    }

    if profile.codegen_units.map(|value| value > 1).unwrap_or(true) {
        suggestions.push(SuggestionReport {
            title: "Compare codegen-units = 1".to_string(),
            confidence: SuggestionConfidence::Medium,
            evidence: match profile.codegen_units {
                Some(value) => format!("[profile.release].codegen-units is {value}."),
                None => "[profile.release].codegen-units is not set.".to_string(),
            },
            tradeoff: "A single codegen unit usually slows release builds because it gives LLVM less parallelism.".to_string(),
            action: "Set [profile.release] codegen-units = 1 for a size-focused build and compare the output.".to_string(),
        });
    }

    if profile
        .panic
        .as_deref()
        .map(|value| value != "abort")
        .unwrap_or(true)
    {
        suggestions.push(SuggestionReport {
            title: "Evaluate panic = \"abort\"".to_string(),
            confidence: SuggestionConfidence::Medium,
            evidence: match &profile.panic {
                Some(value) => format!("[profile.release].panic is {value}."),
                None => "[profile.release].panic is not set.".to_string(),
            },
            tradeoff: "Abort-on-panic removes unwinding but is not appropriate when code relies on catching panics across unwind boundaries.".to_string(),
            action: "Set [profile.release] panic = \"abort\" only if aborting on panic fits the application.".to_string(),
        });
    }

    if profile
        .opt_level
        .as_ref()
        .map(opt_level_favors_speed)
        .unwrap_or(true)
    {
        suggestions.push(SuggestionReport {
            title: "Compare a size-focused opt-level".to_string(),
            confidence: SuggestionConfidence::Low,
            evidence: profile_setting_evidence("opt-level", profile.opt_level.as_ref(), "is not set"),
            tradeoff: "Size-focused optimization can reduce throughput or change latency-sensitive code paths.".to_string(),
            action: "Compare [profile.release] opt-level = \"z\" or \"s\" against the current release build before keeping it.".to_string(),
        });
    }
}

fn add_duplicate_dependency_suggestions(
    suggestions: &mut Vec<SuggestionReport>,
    lockfile: Option<&LockfileReport>,
) {
    let Some(lockfile) = lockfile else {
        return;
    };

    let mut versions_by_name = BTreeMap::<&str, BTreeSet<&str>>::new();
    for package in &lockfile.packages {
        versions_by_name
            .entry(&package.name)
            .or_default()
            .insert(&package.version);
    }

    let duplicates = versions_by_name
        .into_iter()
        .filter(|(_, versions)| versions.len() > 1)
        .map(|(name, versions)| {
            let versions = versions.into_iter().collect::<Vec<_>>().join(", ");
            format!("{name} ({versions})")
        })
        .collect::<Vec<_>>();

    if duplicates.is_empty() {
        return;
    }

    suggestions.push(SuggestionReport {
        title: "Review duplicate dependency versions".to_string(),
        confidence: SuggestionConfidence::High,
        evidence: format!(
            "Cargo.lock contains multiple versions for: {}.",
            duplicates.join("; ")
        ),
        tradeoff: "Aligning versions can require dependency upgrades and compatibility review.".to_string(),
        action: "Use cargo tree -d, then update or constrain dependencies so Cargo can resolve fewer duplicate versions.".to_string(),
    });
}

fn add_default_feature_suggestions(
    suggestions: &mut Vec<SuggestionReport>,
    dependencies: &[ManifestDependencyReport],
) {
    let dependencies_with_defaults = dependencies
        .iter()
        .filter(|dependency| dependency.default_features)
        .map(|dependency| dependency.name.as_str())
        .collect::<Vec<_>>();

    if dependencies_with_defaults.is_empty() {
        return;
    }

    suggestions.push(SuggestionReport {
        title: "Audit direct dependency default features".to_string(),
        confidence: SuggestionConfidence::Low,
        evidence: format!(
            "Direct dependencies with default features enabled or unspecified: {}.",
            dependencies_with_defaults.join(", ")
        ),
        tradeoff: "Disabling default features can remove APIs, protocol support, or platform behavior that the application needs.".to_string(),
        action: "For dependencies whose defaults are unnecessary, set default-features = false and list the needed features explicitly.".to_string(),
    });
}

fn profile_value_enabled(value: &ProfileValue) -> bool {
    match value {
        ProfileValue::Bool(value) => *value,
        ProfileValue::Integer(value) => *value > 0,
        ProfileValue::String(value) => value != "false" && value != "0" && value != "none",
    }
}

fn profile_value_disabled(value: &ProfileValue) -> bool {
    match value {
        ProfileValue::Bool(value) => !*value,
        ProfileValue::Integer(value) => *value == 0,
        ProfileValue::String(value) => value == "false" || value == "0" || value == "none",
    }
}

fn opt_level_favors_speed(value: &ProfileValue) -> bool {
    match value {
        ProfileValue::Integer(value) => *value >= 2,
        ProfileValue::String(value) => value == "2" || value == "3",
        ProfileValue::Bool(_) => false,
    }
}

fn profile_setting_evidence(
    key: &str,
    value: Option<&ProfileValue>,
    missing_text: &'static str,
) -> String {
    match value {
        Some(value) => format!("[profile.release].{key} is {value}."),
        None => format!("[profile.release].{key} {missing_text}."),
    }
}

impl fmt::Display for SuggestionConfidence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::High => write!(formatter, "high"),
            Self::Medium => write!(formatter, "medium"),
            Self::Low => write!(formatter, "low"),
        }
    }
}

impl SuggestionConfidence {
    fn rank(self) -> u8 {
        match self {
            Self::High => 0,
            Self::Medium => 1,
            Self::Low => 2,
        }
    }
}

impl DiffBinaryReport {
    fn from_binary(binary: &BinaryReport) -> Self {
        Self {
            path: binary.path.clone(),
            file_size_bytes: binary.file_size_bytes,
            object: binary.object.as_ref().map(DiffObjectReport::from_object),
        }
    }
}

impl DiffObjectReport {
    fn from_object(object: &ObjectReport) -> Self {
        Self {
            format: object.format.clone(),
            architecture: object.architecture.clone(),
            endianness: object.endianness.clone(),
            entry: object.entry,
            has_debug_symbols: object.has_debug_symbols,
            total_sections: object.total_sections,
        }
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

impl DiffReport {
    fn apply_section_limit(&mut self, limit: Option<usize>) {
        let Some(limit) = limit else {
            return;
        };

        if self.section_deltas.len() <= limit {
            return;
        }

        self.section_deltas_omitted = self.section_deltas.len() - limit;
        self.section_deltas.truncate(limit);
    }
}

impl AttributionReport {
    fn apply_crate_limit(&mut self, limit: Option<usize>) {
        let Some(limit) = limit else {
            return;
        };

        if self.crates.len() <= limit {
            return;
        }

        self.crates_omitted = self.crates.len() - limit;
        self.crates.truncate(limit);
    }
}

fn diff_sections(
    old_object: Option<&ObjectReport>,
    new_object: Option<&ObjectReport>,
) -> Vec<SectionDeltaReport> {
    let old_sections = aggregate_section_sizes(old_object);
    let new_sections = aggregate_section_sizes(new_object);
    let names = old_sections
        .keys()
        .chain(new_sections.keys())
        .cloned()
        .collect::<BTreeSet<_>>();

    let mut deltas = names
        .into_iter()
        .filter_map(|name| {
            let old_size = old_sections.get(&name).copied().unwrap_or(0);
            let new_size = new_sections.get(&name).copied().unwrap_or(0);

            if old_size == new_size {
                return None;
            }

            let status = match (old_size, new_size) {
                (0, _) => SectionDeltaStatus::Added,
                (_, 0) => SectionDeltaStatus::Removed,
                _ => SectionDeltaStatus::Changed,
            };

            Some(SectionDeltaReport {
                name,
                old_size_bytes: old_size,
                new_size_bytes: new_size,
                delta_bytes: byte_delta(old_size, new_size),
                status,
            })
        })
        .collect::<Vec<_>>();

    deltas.sort_by(|left, right| {
        right
            .delta_bytes
            .unsigned_abs()
            .cmp(&left.delta_bytes.unsigned_abs())
            .then_with(|| left.name.cmp(&right.name))
    });
    deltas
}

fn aggregate_section_sizes(object: Option<&ObjectReport>) -> BTreeMap<String, u64> {
    let mut sections = BTreeMap::<String, u64>::new();

    if let Some(object) = object {
        for section in &object.sections {
            let total = sections.entry(section.name.clone()).or_default();
            *total = total.saturating_add(section.size_bytes);
        }
    }

    sections
}

fn byte_delta(old: u64, new: u64) -> i64 {
    if new >= old {
        (new - old).min(i64::MAX as u64) as i64
    } else {
        -((old - new).min(i64::MAX as u64) as i64)
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

fn print_diff_report(report: &DiffReport) {
    print_binary_summary("old", &report.old);
    print_binary_summary("new", &report.new);
    println!(
        "file size delta: {:+} bytes ({:+.2} MiB)",
        report.file_size_delta_bytes,
        signed_bytes_to_mib(report.file_size_delta_bytes)
    );

    if report.section_deltas.is_empty() {
        if report.old.object.is_some() || report.new.object.is_some() {
            println!("section deltas: none");
        } else {
            println!("section deltas: unavailable (neither input is a recognized object)");
        }
        return;
    }

    println!("section deltas:");
    for delta in &report.section_deltas {
        println!(
            "  {}: {:+} bytes ({} -> {}, {})",
            delta.name,
            delta.delta_bytes,
            delta.old_size_bytes,
            delta.new_size_bytes,
            delta.status.as_str()
        );
    }

    if report.section_deltas_omitted > 0 {
        println!(
            "  ... {} more section deltas omitted",
            report.section_deltas_omitted
        );
    }
}

fn print_attribution_report(report: &AttributionReport) {
    println!("manifest: {}", report.manifest_path);
    println!("package root: {}", report.package_root);
    println!("tool: {}", report.tool);
    println!("scope: {}", report.scope);
    println!(
        "file size: {} bytes ({:.2} MiB)",
        report.file_size_bytes,
        bytes_to_mib(report.file_size_bytes)
    );
    println!(
        ".text size: {} bytes ({:.2} MiB)",
        report.text_section_size_bytes,
        bytes_to_mib(report.text_section_size_bytes)
    );

    if report.crates.is_empty() {
        println!("crate attribution: none reported");
        return;
    }

    println!("crate attribution:");
    for krate in &report.crates {
        println!("  {}: {} bytes", krate.name, krate.size_bytes);
    }

    if report.crates_omitted > 0 {
        println!("  ... {} more crates omitted", report.crates_omitted);
    }
}

fn print_binary_summary(label: &str, binary: &DiffBinaryReport) {
    println!("{label}: {}", binary.path);
    println!(
        "  size: {} bytes ({:.2} MiB)",
        binary.file_size_bytes,
        bytes_to_mib(binary.file_size_bytes)
    );

    match &binary.object {
        Some(object) => {
            println!(
                "  object: {} {} {}",
                object.format, object.architecture, object.endianness
            );
            println!("  sections: {}", object.total_sections);
        }
        None => println!("  object: not recognized"),
    }
}

fn print_text_report(report: &InspectReport) {
    println!("path: {}", report.path);
    println!(
        "size: {} bytes ({:.2} MiB)",
        report.file_size_bytes,
        bytes_to_mib(report.file_size_bytes)
    );

    if let Some(object) = &report.object {
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
    } else {
        println!("object: not recognized");
    }

    if let Some(cargo) = &report.cargo {
        print_cargo_report(cargo);
    }

    print_suggestions(&report.suggestions);
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

fn print_suggestions(suggestions: &[SuggestionReport]) {
    if suggestions.is_empty() {
        return;
    }

    println!("suggestions:");
    for (index, suggestion) in suggestions.iter().enumerate() {
        println!("  {}. {}", index + 1, suggestion.title);
        println!("     confidence: {}", suggestion.confidence);
        println!("     evidence: {}", suggestion.evidence);
        println!("     tradeoff: {}", suggestion.tradeoff);
        println!("     action: {}", suggestion.action);
    }
}

fn bytes_to_mib(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0
}

fn signed_bytes_to_mib(bytes: i64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "yes"
    } else {
        "no"
    }
}

impl SectionDeltaStatus {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Changed => "changed",
        }
    }
}

fn print_help() {
    println!(
        "cargoslim {VERSION}

Explain Rust binary size and produce conservative shrink plans.

Usage:
  cargoslim attribution [--json] [--limit <n>] [--manifest-path <path>] [--bin <name>]
  cargoslim diff [--json] [--limit <n>] <old> <new>
  cargoslim inspect [--json] [--limit <n>] [--manifest-path <path>] <path>
  cargoslim --help
  cargoslim --version

Commands:
  attribution [--json] [--limit <n>] [--manifest-path <path>] [--bin <name>]
    Report crate-level .text section attribution through cargo-bloat
  diff [--json] [--limit <n>] <old> <new>
    Compare file size and object section sizes between two binaries
  inspect [--json] [--limit <n>] [--manifest-path <path>] <path>
    Report file size, object section sizes, Cargo context, and conservative suggestions"
    );
}

#[cfg(test)]
mod tests {
    use super::{
        byte_delta, bytes_to_mib, inspect_path, AttributionOptions, Command, DiffOptions,
        InspectOptions, OutputFormat,
    };
    use std::path::PathBuf;

    #[test]
    fn converts_bytes_to_mib() {
        assert_eq!(bytes_to_mib(1_048_576), 1.0);
    }

    #[test]
    fn calculates_signed_byte_delta() {
        assert_eq!(byte_delta(10, 15), 5);
        assert_eq!(byte_delta(15, 10), -5);
        assert_eq!(byte_delta(10, 10), 0);
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
    fn parses_diff_defaults_to_text() {
        let command = Command::parse(["diff", "old", "new"]).unwrap();

        assert_eq!(
            command,
            Command::Diff(DiffOptions {
                old_path: PathBuf::from("old"),
                new_path: PathBuf::from("new"),
                output: OutputFormat::Text,
                section_limit: None,
            })
        );
    }

    #[test]
    fn parses_attribution_defaults() {
        let command = Command::parse(["attribution"]).unwrap();

        assert_eq!(
            command,
            Command::Attribution(AttributionOptions {
                manifest_path: PathBuf::from("Cargo.toml"),
                binary_name: None,
                output: OutputFormat::Text,
                crate_limit: Some(20),
            })
        );
    }

    #[test]
    fn parses_attribution_options() {
        let command = Command::parse([
            "attribution",
            "--json",
            "--limit=4",
            "--manifest-path",
            "fixtures/Cargo.toml",
            "--bin=cargoslim",
        ])
        .unwrap();

        assert_eq!(
            command,
            Command::Attribution(AttributionOptions {
                manifest_path: PathBuf::from("fixtures/Cargo.toml"),
                binary_name: Some("cargoslim".to_string()),
                output: OutputFormat::Json,
                crate_limit: Some(4),
            })
        );
    }

    #[test]
    fn parses_diff_json_and_section_limit() {
        let command = Command::parse(["diff", "--json", "--limit=4", "old", "new"]).unwrap();

        assert_eq!(
            command,
            Command::Diff(DiffOptions {
                old_path: PathBuf::from("old"),
                new_path: PathBuf::from("new"),
                output: OutputFormat::Json,
                section_limit: Some(4),
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
    fn rejects_missing_diff_paths() {
        let error = Command::parse(["diff", "old"]).unwrap_err();

        assert_eq!(error, "diff requires two paths");
    }

    #[test]
    fn rejects_extra_diff_paths() {
        let error = Command::parse(["diff", "old", "new", "extra"]).unwrap_err();

        assert_eq!(error, "diff accepts exactly two paths");
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
