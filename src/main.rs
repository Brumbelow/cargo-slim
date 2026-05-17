use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use object::{Object, ObjectSection};
use serde::Serialize;

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
        Ok(Command::Inspect(options)) => match inspect_path(&options.path) {
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
        },
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
    let mut section_limit = None;
    let mut path = None;
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
            _ if arg.starts_with('-') => return Err(format!("unknown inspect option '{arg}'")),
            _ if path.is_none() => path = Some(PathBuf::from(arg)),
            _ => return Err("inspect accepts exactly one path".to_string()),
        }
    }

    let path = path.ok_or_else(|| "inspect requires a path".to_string())?;
    Ok(Command::Inspect(InspectOptions {
        path,
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
    file_size_mib: f64,
    object: Option<ObjectReport>,
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

fn inspect_path(path: &Path) -> Result<InspectReport, String> {
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

    Ok(InspectReport {
        path: path.display().to_string(),
        file_size_bytes: metadata.len(),
        file_size_mib: bytes_to_mib(metadata.len()),
        object,
    })
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
        report.file_size_bytes, report.file_size_mib
    );

    let Some(object) = &report.object else {
        println!("object: not recognized");
        return;
    };

    println!("object: {}", object.format);
    println!("architecture: {}", object.architecture);
    println!("endianness: {}", object.endianness);
    println!("entry: 0x{:x}", object.entry);
    println!("debug symbols: {}", yes_no(object.has_debug_symbols));

    if object.sections.is_empty() {
        println!("sections: none reported");
        return;
    }

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
  cargoslim inspect [--json] [--limit <n>] <path>
  cargoslim --help
  cargoslim --version

Commands:
  inspect [--json] [--limit <n>] <path>  Report file size and object section sizes"
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
                output: OutputFormat::Text,
                section_limit: Some(8),
            })
        );
    }

    #[test]
    fn rejects_zero_section_limit() {
        let error = Command::parse(["inspect", "--limit", "0", "target/release/app"]).unwrap_err();

        assert_eq!(error, "--limit must be greater than zero");
    }

    #[test]
    fn rejects_duplicate_inspect_paths() {
        let error = Command::parse(["inspect", "a", "b"]).unwrap_err();

        assert_eq!(error, "inspect accepts exactly one path");
    }

    #[test]
    fn inspects_current_test_binary_as_object() {
        let report = inspect_path(std::env::current_exe().unwrap().as_path()).unwrap();
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
