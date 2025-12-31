//! Entry point for the uld linker.
//!
//! Simple flow: parse args → load files → link → write executable.

use anyhow::{Context, Result};
use clap::Parser;
use memmap2::Mmap;
use object::{Architecture as ObjArch, Object};
use std::fs::File;
use tracing::info;
use tracing_subscriber::EnvFilter;

use uld::arch::x86_64::X86_64;
use uld::config::Config;
use uld::linker::Linker;

fn main() -> Result<()> {
    let config = Config::parse();

    // Initialize logging
    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(&config.log_level))
        .unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    // Resolve inputs (handles -l, -L, -o)
    let (output_path, input_paths) = config.resolve_inputs();
    if input_paths.is_empty() {
        anyhow::bail!("no input files");
    }

    // Memory-map input files and verify architecture
    let mut open_files = Vec::new();
    for path in &input_paths {
        info!("Processing input: {}", path.display());
        let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        let mmap = unsafe { Mmap::map(&file)? };

        // Verify x86_64 architecture (skip archives)
        if !mmap.starts_with(b"!<arch>\n") {
            let obj = object::File::parse(&*mmap).context("failed to parse object file")?;
            if obj.architecture() != ObjArch::X86_64 {
                anyhow::bail!(
                    "Unsupported architecture in {}: {:?}. Only X86_64 is supported.",
                    path.display(),
                    obj.architecture()
                );
            }
        }

        open_files.push((path.clone(), mmap));
    }

    // Link
    let mut linker = Linker::new(X86_64);
    for (path, mmap) in &open_files {
        linker.add_file(path.clone(), mmap)?;
    }
    linker.layout()?;
    linker.relocate()?;
    linker.write(&output_path)?;

    info!("Linked successfully to {}", output_path.display());
    Ok(())
}
