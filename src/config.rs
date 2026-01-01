//! Command-line configuration.
//!
//! Accepts all args positionally for clang compatibility.
//! clang -fuse-ld= sends: -o out file1.o -L/path -lc file2.o

use clap::Parser;
use std::path::PathBuf;
use tracing::{info, warn};

use crate::utils::find_library;

#[derive(Parser)]
#[command(author, version, about = "A minimal static linker")]
pub struct Config {
    /// All linker arguments
    #[arg(allow_hyphen_values = true, trailing_var_arg = true)]
    args: Vec<String>,

    /// Log level
    #[arg(long, default_value = "warn")]
    pub log_level: String,
}

impl Config {
    pub fn output(&self) -> PathBuf {
        let mut iter = self.args.iter();
        while let Some(arg) = iter.next() {
            if arg == "-o" {
                if let Some(p) = iter.next() {
                    return PathBuf::from(p);
                }
            }
        }
        PathBuf::from("a.out")
    }

    pub fn input_files(&self) -> Vec<PathBuf> {
        let mut lib_paths = Vec::new();
        let mut files = Vec::new();

        let mut iter = self.args.iter();
        while let Some(arg) = iter.next() {
            if arg == "-o" { iter.next(); continue; }
            if arg.starts_with("--") { continue; } // --start-group etc.

            if let Some(p) = arg.strip_prefix("-L") {
                let path = if p.is_empty() { iter.next().map(|s| s.as_str()).unwrap_or("") } else { p };
                if !path.starts_with('-') { lib_paths.push(PathBuf::from(path)); }
            } else if let Some(n) = arg.strip_prefix("-l") {
                let name = if n.is_empty() { iter.next().map(|s| s.as_str()).unwrap_or("") } else { n };
                match find_library(name, &lib_paths) {
                    Some(p) => { info!("-l{} -> {}", name, p.display()); files.push(p); }
                    None => warn!("-l{} not found", name),
                }
            } else if arg.starts_with('-') {
                continue;
            } else {
                let p = PathBuf::from(arg);
                if p.exists() { files.push(p); }
            }
        }
        files
    }
}
