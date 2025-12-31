//! Configuration module.
//!
//! This module defines the command-line interface (CLI) for the linker using `clap`.
//! It handles parsing arguments like input files and the output file path.

use clap::Parser;
use std::path::PathBuf;

/// A minimal linker for x86_64 ELF binaries.
///
/// This linker supports combining multiple object files into a single executable.
/// It is designed for educational purposes and currently only supports x86_64 Linux.
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Config {
    /// Input object files (and ignored flags)
    #[arg(required = true, allow_hyphen_values = true, num_args = 1..)]
    pub inputs: Vec<String>,

    /// Output file
    #[arg(short, long, default_value = "a.out", help = "Path to the output executable")]
    pub output: PathBuf,

    /// Log level (error, warn, info, debug, trace)
    #[arg(long, default_value = "info", help = "Set the logging level")]
    pub log_level: String,
}
