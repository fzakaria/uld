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
use std::path::PathBuf;
use object::{Object, Architecture as ObjArch};

use uld::arch::x86_64::X86_64;
use uld::config::Config;
use uld::linker::Linker;

fn find_library(name: &str, paths: &[PathBuf]) -> Option<PathBuf> {
    let filename = format!("lib{}.a", name);
    for path in paths {
        let full_path = path.join(&filename);
        if full_path.exists() {
            return Some(full_path);
        }
    }
    None
}

fn main() -> Result<()> {
    let config = Config::parse();
    
    let mut final_output = config.output;
    let mut input_paths = Vec::new();
    let mut search_paths = Vec::new();

    let mut iter = config.inputs.into_iter();
    while let Some(arg) = iter.next() {
        if arg == "-o" {
            if let Some(path) = iter.next() {
                final_output = PathBuf::from(path);
            }
            continue;
        }
        
        if arg.starts_with("-L") {
            if arg == "-L" {
                if let Some(path) = iter.next() {
                    search_paths.push(PathBuf::from(path));
                }
            } else {
                search_paths.push(PathBuf::from(&arg[2..]));
            }
            continue;
        }

        if arg.starts_with("-l") {
            let name = if arg == "-l" {
                iter.next()
            } else {
                Some(arg[2..].to_string())
            };
            
            if let Some(name) = name {
                if let Some(path) = find_library(&name, &search_paths) {
                    println!("Found library -l{}: {}", name, path.display());
                    input_paths.push(path);
                } else {
                    println!("Warning: Library -l{} not found in search paths", name);
                    println!("Search paths: {:?}", search_paths);
                }
            }
            continue;
        }
        
        if arg.starts_with("-") {
            continue; // Ignore other flags
        }
        
        let path = PathBuf::from(&arg);
        if !path.exists() {
             // Ignore non-existent files (assumed flag args)
             continue; 
        }
        
        input_paths.push(path);
    }

    if input_paths.is_empty() {
        anyhow::bail!("no input files");
    }

    // Map input files into memory
    let mut open_files = Vec::new();
    for path in &input_paths {
        println!("Processing input: {}", path.display());
        let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        let mmap = unsafe { Mmap::map(&file)? };
        
        // Architecture Check
        // We only check objects, skip archives for now (archives can contain objects of different arch, theoretically)
        // But checking archive magic is better.
        let magic = &mmap[..8.min(mmap.len())];
        if !magic.starts_with(b"!<arch>\n") {
             let obj = object::File::parse(&*mmap).context("failed to parse object file")?;
             if obj.architecture() != ObjArch::X86_64 {
                 anyhow::bail!("Unsupported architecture in {}: {:?}. Only X86_64 is supported.", path.display(), obj.architecture());
             }
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
    linker.write(&final_output)?;

    println!("Linked successfully to {}", final_output.display());
    Ok(())
}
