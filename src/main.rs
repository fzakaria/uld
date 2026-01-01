//! Entry point for the uld linker.

use anyhow::{Context, Result};
use clap::Parser;
use memmap2::Mmap;
use std::fs::File;
use tracing::info;
use tracing_subscriber::EnvFilter;

use uld::arch::x86_64::X86_64;
use uld::config::Config;
use uld::linker::Linker;

fn main() -> Result<()> {
    let config = Config::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_new(&config.log_level).unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();

    let files = config.input_files();
    if files.is_empty() {
        anyhow::bail!("no input files");
    }

    // Memory-map files
    let mmaps: Vec<_> = files
        .iter()
        .map(|p| {
            info!("Loading: {}", p.display());
            let f = File::open(p).with_context(|| format!("open {}", p.display()))?;
            let m = unsafe { Mmap::map(&f)? };
            Ok((p, m))
        })
        .collect::<Result<Vec<_>>>()?;

    // Link
    let mut linker = Linker::new(X86_64);
    for (p, m) in &mmaps {
        linker.add_file(p, m)?;
    }
    linker.link()?;
    linker.write(&config.output())?;

    info!("Wrote: {}", config.output().display());
    Ok(())
}
