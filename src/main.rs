//! Entry point for the uld linker.

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
        .unwrap_or_else(|_| EnvFilter::new("warn"));
    tracing_subscriber::fmt().with_env_filter(filter).init();

    // Parse inputs (handles -l, -L, -o in order)
    let inputs = config.parse_inputs();
    if inputs.files.is_empty() {
        anyhow::bail!("no input files");
    }

    // Memory-map and verify architecture
    let mut mmaps = Vec::new();
    for path in &inputs.files {
        info!("Processing: {}", path.display());
        let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
        let mmap = unsafe { Mmap::map(&file)? };

        if !mmap.starts_with(b"!<arch>\n") {
            let obj = object::File::parse(&*mmap)?;
            if obj.architecture() != ObjArch::X86_64 {
                anyhow::bail!("unsupported architecture: {:?}", obj.architecture());
            }
        }
        mmaps.push((path.clone(), mmap));
    }

    // Link
    let mut linker = Linker::new(X86_64);
    for (path, mmap) in &mmaps {
        linker.add_file(path.clone(), mmap)?;
    }
    linker.link()?;
    linker.write(&inputs.output)?;

    info!("Linked: {}", inputs.output.display());
    Ok(())
}
