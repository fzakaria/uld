//! Configuration module.
//!
//! Handles command-line parsing using `clap`, including:
//! - Input files (object files, archives)
//! - Library search paths (-L)
//! - Library names (-l)
//! - Output file path (-o)

use clap::Parser;
use std::path::PathBuf;
use tracing::{info, warn};

/// A minimal linker for x86_64 ELF binaries.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Config {
    /// Input files (object files, archives, or flags to ignore)
    #[arg(required = true, allow_hyphen_values = true, num_args = 1..)]
    pub inputs: Vec<String>,

    /// Output executable path
    #[arg(short, long, default_value = "a.out")]
    pub output: PathBuf,

    /// Library search paths (can be specified multiple times)
    #[arg(short = 'L', action = clap::ArgAction::Append)]
    pub library_paths: Vec<PathBuf>,

    /// Libraries to link (can be specified multiple times)
    #[arg(short = 'l', action = clap::ArgAction::Append)]
    pub libraries: Vec<String>,

    /// Log level (error, warn, info, debug, trace)
    #[arg(long, default_value = "debug")]
    pub log_level: String,
}

impl Config {
    /// Resolve all input files, expanding -l libraries and handling -L/-o overrides.
    /// Returns the final output path and list of resolved input file paths.
    pub fn resolve_inputs(&self) -> (PathBuf, Vec<PathBuf>) {
        let mut output = self.output.clone();
        let mut search_paths = self.library_paths.clone();
        let mut input_paths = Vec::new();

        let mut iter = self.inputs.iter().peekable();
        while let Some(arg) = iter.next() {
            // Handle -o override (some toolchains pass it in inputs)
            if arg == "-o" {
                if let Some(path) = iter.next() {
                    output = PathBuf::from(path);
                }
                continue;
            }

            // Handle -L in inputs
            if let Some(path) = arg.strip_prefix("-L") {
                let path = if path.is_empty() {
                    iter.next().map(|s| s.as_str()).unwrap_or("")
                } else {
                    path
                };
                if !path.is_empty() {
                    search_paths.push(PathBuf::from(path));
                }
                continue;
            }

            // Handle -l in inputs
            if let Some(name) = arg.strip_prefix("-l") {
                let name = if name.is_empty() {
                    iter.next().map(|s| s.as_str()).unwrap_or("")
                } else {
                    name
                };
                if let Some(path) = self.find_library(name, &search_paths) {
                    info!("Found library -l{}: {}", name, path.display());
                    input_paths.push(path);
                } else if !name.is_empty() {
                    warn!("Library -l{} not found in search paths: {:?}", name, search_paths);
                }
                continue;
            }

            // Skip unknown flags
            if arg.starts_with('-') {
                continue;
            }

            // Regular file path
            let path = PathBuf::from(arg);
            if path.exists() {
                input_paths.push(path);
            }
        }

        // Also resolve libraries from -l flags
        for name in &self.libraries {
            if let Some(path) = self.find_library(name, &search_paths) {
                info!("Found library -l{}: {}", name, path.display());
                input_paths.push(path);
            } else {
                warn!("Library -l{} not found in search paths: {:?}", name, search_paths);
            }
        }

        (output, input_paths)
    }

    fn find_library(&self, name: &str, search_paths: &[PathBuf]) -> Option<PathBuf> {
        let filename = format!("lib{}.a", name);
        for path in search_paths {
            let full_path = path.join(&filename);
            if full_path.exists() {
                return Some(full_path);
            }
        }
        None
    }
}
