# uld - A Minimal Rust Linker

`uld` is a minimal linker written in Rust for educational purposes. It targets **x86_64 Linux ELF** binaries.

## Design Philosophy

*   **Minimalism**: We focus on the core logic of linking (resolution, layout, relocation) without the burden of supporting every legacy feature or architecture.
*   **Educational**: The code is structured to be readable and understandable.
*   **"Dumb"**: We assume modern defaults (e.g., `BIND_NOW`) and do not perform complex relaxations or optimizations.
*   **Safety**: Written in Safe Rust where possible (using `object` crate for parsing).

## Features

*   **ELF64 x86_64** support.
*   **Static Linking** of object files.
*   **Symbol Resolution**: Handles global/weak symbols.
*   **Relocations**: Supports `R_X86_64_64`, `R_X86_64_PC32`, `R_X86_64_PLT32`.
*   **Layout**: Maps `.text`, `.rodata`, `.data`, `.bss` sections.

## Building and Running

```bash
cargo build
```

To link object files:

```bash
cargo run -- -o output_binary file1.o file2.o
```

## Structure

*   `src/arch`: Architecture-specific logic (relocations).
*   `src/linker.rs`: Core linking orchestration.
*   `src/layout.rs`: Output section layout.
*   `src/symbol.rs`: Symbol table definitions.

## Testing

This project uses Python-based integration tests (simulating `lit`) to compile C code and link it with `uld`.
