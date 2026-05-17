use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    let mut args = env::args().skip(1);

    match args.next().as_deref() {
        None | Some("-h") | Some("--help") => {
            print_help();
            ExitCode::SUCCESS
        }
        Some("-V") | Some("--version") => {
            println!("cargoslim {VERSION}");
            ExitCode::SUCCESS
        }
        Some("inspect") => match args.next() {
            Some(path) if args.next().is_none() => inspect_path(Path::new(&path)),
            Some(_) => {
                eprintln!("error: inspect accepts exactly one path");
                ExitCode::from(2)
            }
            None => {
                eprintln!("error: inspect requires a path");
                ExitCode::from(2)
            }
        },
        Some(command) => {
            eprintln!("error: unknown command '{command}'");
            eprintln!();
            print_help();
            ExitCode::from(2)
        }
    }
}

fn inspect_path(path: &Path) -> ExitCode {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => {
            let bytes = metadata.len();
            println!("path: {}", path.display());
            println!("size: {} bytes ({:.2} MiB)", bytes, bytes_to_mib(bytes));
            ExitCode::SUCCESS
        }
        Ok(_) => {
            eprintln!("error: '{}' is not a file", path.display());
            ExitCode::from(1)
        }
        Err(error) => {
            eprintln!("error: could not inspect '{}': {error}", path.display());
            ExitCode::from(1)
        }
    }
}

fn bytes_to_mib(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0
}

fn print_help() {
    println!(
        "cargoslim {VERSION}

Explain Rust binary size and produce conservative shrink plans.

Usage:
  cargoslim inspect <path>
  cargoslim --help
  cargoslim --version

Commands:
  inspect <path>  Report the size of a binary or file"
    );
}

#[cfg(test)]
mod tests {
    use super::bytes_to_mib;

    #[test]
    fn converts_bytes_to_mib() {
        assert_eq!(bytes_to_mib(1_048_576), 1.0);
    }
}
