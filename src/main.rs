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

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_new(&config.log_level).unwrap_or_else(|_| EnvFilter::new("warn")))
        .init();

    let (output, files) = config.parse_args();
    if files.is_empty() {
        anyhow::bail!("no input files");
    }

    // Memory-map files
    let mmaps: Vec<_> = files.iter().map(|p| {
        info!("Loading: {}", p.display());
        let f = File::open(p).with_context(|| format!("open {}", p.display()))?;
        let m = unsafe { Mmap::map(&f)? };
        // Check architecture (skip archives)
        if !m.starts_with(b"!<arch>\n") {
            let obj = object::File::parse(&*m)?;
            if obj.architecture() != ObjArch::X86_64 {
                anyhow::bail!("unsupported: {:?}", obj.architecture());
            }
        }
        Ok((p.clone(), m))
    }).collect::<Result<Vec<_>>>()?;

    // Link
    let mut linker = Linker::new(X86_64);
    for (p, m) in &mmaps {
        linker.add_file(p.clone(), m)?;
    }
    linker.link()?;
    linker.write(&output)?;

    info!("Wrote: {}", output.display());
    Ok(())
}
