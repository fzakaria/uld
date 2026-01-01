//! Minimal Linker Library.
//!
//! This library provides the core components for the `uld` linker.
//! It is organized into several modules:
//! - `config`: CLI configuration.
//! - `arch`: Architecture-specific backend logic.
//! - `linker`: The main linking orchestration.
//! - `layout`: Output memory layout management.
//! - `symbol`: Symbol table management.
//! - `writer`: ELF file writing.

pub mod arch;
pub mod config;
pub mod layout;
pub mod linker;
pub mod symbol;
pub mod utils;
pub mod writer;
