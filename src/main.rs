//! Entry point for the uld linker.
//!
//! This file handles high-level application flow:
//! 1. Parse command-line arguments using `clap`.
//! 2. Initialize the linker with the `X86_64` backend (the only supported architecture).
//! 3. Verify input files match the target architecture.
//! 4. Execute the linking steps: load, resolve, layout, relocate, write.
//!
//! Error handling is done via `anyhow`.

use anyhow::{Context, Result};
use clap::Parser;
use memmap2::Mmap;
use std::fs::File;
use object::{Object, Architecture as ObjArch};

use uld::arch::x86_64::X86_64;
use uld::config::Config;
use uld::linker::Linker;

fn main() -> Result<()> {
    let config = Config::parse();

    // Map input files into memory
    let mut open_files = Vec::new();
    for path_str in &config.inputs {
        if path_str.starts_with('-') {
            // Ignore flags passed by gcc/clang that we don't support yet
            // println!("Warning: Ignoring flag {}", path_str);
            continue;
        }
        
        let path = std::path::PathBuf::from(path_str);
        let file = File::open(&path).with_context(|| format!("failed to open {}", path.display()))?;
        let mmap = unsafe { Mmap::map(&file)? };
        
        // Architecture Check
        let obj = object::File::parse(&*mmap).context("failed to parse object file")?;
        if obj.architecture() != ObjArch::X86_64 {
            anyhow::bail!("Unsupported architecture in {}: {:?}. Only X86_64 is supported.", path.display(), obj.architecture());
        }

        open_files.push((path.clone(), mmap));
    }

    // Initialize Linker with x86_64 backend
    let mut linker = Linker::new(X86_64);

    // 1. Add files (Parses symbols)
    for (path, mmap) in &open_files {
        linker.add_file(path.clone(), mmap)?;
    }

    // 2. Verify all symbols are resolved
    linker.verify_unresolved()?;

    // 3. Layout sections in memory
    linker.layout()?;

    // 4. Apply relocations
    linker.relocate()?;

    // 5. Write final executable
    linker.write(&config.output)?;

    println!("Linked successfully to {}", config.output.display());
    Ok(())
}