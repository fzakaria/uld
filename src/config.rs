//! Command-line configuration.
//!
//! Accepts all arguments as positional since clang's `-fuse-ld=` passes
//! everything that way. We parse -L, -l, -o from the stream.
//! Note: library order doesn't matter since we iterate until fixpoint.

use clap::Parser;
use std::path::PathBuf;
use tracing::{info, warn};

use crate::utils::find_library;

#[derive(Parser)]
#[command(author, version, about = "A minimal static linker")]
pub struct Config {
    /// Input files and flags (parsed manually since clang sends everything positional)
    #[arg(allow_hyphen_values = true)]
    args: Vec<String>,

    /// Log level
    #[arg(long, default_value = "warn")]
    pub log_level: String,
}

impl Config {
    /// Parse args into output path and input files.
    pub fn parse_args(&self) -> (PathBuf, Vec<PathBuf>) {
        let mut output = PathBuf::from("a.out");
        let mut lib_paths: Vec<PathBuf> = Vec::new();
        let mut files: Vec<PathBuf> = Vec::new();

        let mut args = self.args.iter().peekable();
        while let Some(arg) = args.next() {
            if arg == "-o" {
                output = args.next().map(PathBuf::from).unwrap_or(output);
            } else if let Some(p) = arg.strip_prefix("-L") {
                lib_paths.push(PathBuf::from(if p.is_empty() {
                    args.next().map(String::as_str).unwrap_or("")
                } else { p }));
            } else if let Some(name) = arg.strip_prefix("-l") {
                let name = if name.is_empty() { args.next().map(String::as_str).unwrap_or("") } else { name };
                match find_library(name, &lib_paths) {
                    Some(p) => { info!("Found -l{}: {}", name, p.display()); files.push(p); }
                    None => warn!("-l{} not found", name),
                }
            } else if arg.starts_with('-') {
                // Skip unknown flags
            } else {
                let p = PathBuf::from(arg);
                if p.exists() { files.push(p); }
            }
        }
        (output, files)
    }
}
