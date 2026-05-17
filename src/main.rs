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
            Ok(report) => {
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
    let mut path = None;

    for arg in args {
        match arg.as_str() {
            "--json" => output = OutputFormat::Json,
            "-h" | "--help" => return Ok(Command::Help),
            _ if arg.starts_with('-') => return Err(format!("unknown inspect option '{arg}'")),
            _ if path.is_none() => path = Some(PathBuf::from(arg)),
            _ => return Err("inspect accepts exactly one path".to_string()),
        }
    }

    let path = path.ok_or_else(|| "inspect requires a path".to_string())?;
    Ok(Command::Inspect(InspectOptions { path, output }))
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
  cargoslim inspect [--json] <path>
  cargoslim --help
  cargoslim --version

Commands:
  inspect [--json] <path>  Report file size and object section sizes"
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
            })
        );
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
    }
}
