//! Command-line configuration.
//!
//! When used as a linker backend (via `clang -fuse-ld=uld`), clang passes
//! arguments in order like: `uld crt1.o -L/path -lc file.o -o out`
//!
//! Library order matters: `-lc` only resolves symbols from objects appearing
//! before it. Clap can't preserve this order, so we capture all positional
//! args and parse them ourselves.

use clap::Parser;
use std::path::PathBuf;
use tracing::{info, warn};

/// A minimal static linker for x86_64 ELF binaries.
#[derive(Parser, Debug)]
#[command(author, version, about)]
pub struct Config {
    /// All arguments (files, -L, -l, -o) in order.
    /// Order matters for library resolution.
    #[arg(required = true, allow_hyphen_values = true, num_args = 1..)]
    pub args: Vec<String>,

    /// Log level (error, warn, info, debug, trace)
    #[arg(long, default_value = "warn")]
    pub log_level: String,
}

/// Parsed linker inputs.
pub struct LinkerInputs {
    pub output: PathBuf,
    pub files: Vec<PathBuf>,
}

impl Config {
    /// Parse arguments in order, resolving -l libraries against -L paths.
    pub fn parse_inputs(&self) -> LinkerInputs {
        let mut output = PathBuf::from("a.out");
        let mut search_paths = Vec::new();
        let mut files = Vec::new();

        let mut iter = self.args.iter();
        while let Some(arg) = iter.next() {
            // Output file
            if arg == "-o" {
                if let Some(path) = iter.next() {
                    output = PathBuf::from(path);
                }
                continue;
            }

            // Library search path
            if let Some(rest) = arg.strip_prefix("-L") {
                let path = if rest.is_empty() { iter.next().map(String::as_str) } else { Some(rest) };
                if let Some(p) = path {
                    search_paths.push(PathBuf::from(p));
                }
                continue;
            }

            // Library
            if let Some(rest) = arg.strip_prefix("-l") {
                let name = if rest.is_empty() { iter.next().map(String::as_str) } else { Some(rest) };
                if let Some(n) = name {
                    if let Some(path) = find_library(n, &search_paths) {
                        info!("Found -l{}: {}", n, path.display());
                        files.push(path);
                    } else {
                        warn!("Library -l{} not found in {:?}", n, search_paths);
                    }
                }
                continue;
            }

            // Skip other flags
            if arg.starts_with('-') {
                continue;
            }

            // Regular file
            let path = PathBuf::from(arg);
            if path.exists() {
                files.push(path);
            }
        }

        LinkerInputs { output, files }
    }
}

fn find_library(name: &str, search_paths: &[PathBuf]) -> Option<PathBuf> {
    let filename = format!("lib{}.a", name);
    search_paths.iter()
        .map(|p| p.join(&filename))
        .find(|p| p.exists())
}
