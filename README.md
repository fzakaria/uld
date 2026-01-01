# uld - A Minimal Rust Static Linker

`uld` is a minimal static linker written in Rust for educational purposes. It targets **x86_64 Linux ELF** binaries.

## Features

- **Static linking** of object files (`.o`) and archives (`.a`)
- **musl libc** support for fully static executables
- **Works as a clang backend** via `-fuse-ld=/path/to/uld`
- **Symbol resolution**: global, weak, and local symbols
- **Relocations**: `R_X86_64_64`, `R_X86_64_PC32`, `R_X86_64_PLT32`, `R_X86_64_GOT*`
- **GOT (Global Offset Table)** generation
- **Selective archive linking**: only pulls in needed members

## Design Philosophy

- **Minimalism**: Core linking logic without legacy cruft
- **Educational**: Code is structured to be readable
- **Static-only**: No dynamic linking, no PLT trampolines
- **Safe Rust**: Uses `object` crate for parsing, safe code throughout

## Building

```bash
cargo build
```

## Usage

### Direct invocation
```bash
./target/debug/uld -o output crt1.o crti.o main.o -L/path -lc crtn.o
```

### Via gcc driver (recommended)
```bash
# Compile and link a static binary using musl-gcc
musl-gcc -fuse-ld=/path/to/uld -static -o hello hello.c
```

## Project Structure

```
src/
├── main.rs      # Entry point
├── config.rs    # CLI argument handling
├── linker.rs    # Core linking: load → layout → relocate
├── symbol.rs    # Symbol table management
├── layout.rs    # Section/Segment structures
├── arch/        # Architecture-specific relocation handling
│   └── x86_64.rs
├── writer.rs    # ELF output generation
└── utils.rs     # Utilities (alignment)
```

### Linking Phases

1. **Load**: Parse object files and archives, build symbol table
2. **Layout**: Map sections into segments, assign virtual addresses
3. **Resolve**: Compute final address for each symbol
4. **Relocate**: Patch code/data with resolved addresses
5. **Write**: Generate ELF executable

## Testing

Uses [LLVM lit](https://llvm.org/docs/CommandGuide/lit.html) for integration tests:

```bash
# Run all tests
lit tests/

# Run with verbose output
lit tests/ -v
```

### Test Categories

| Test | Description |
|------|-------------|
| `exit_42.s` | Minimal assembly, syscall exit |
| `function_call.s` | Assembly function calls |
| `c_return_42.c` | Basic C with custom start.s |
| `libc_printf.c` | C with musl libc (manual CRT) |
| `libc_printf_clang_driver.c` | C via clang driver |
| `recursive_fib.c` | Recursive functions |
| `large_bss_array.c` | Large BSS arrays |
| `string_ops.c` | String operations |
| `argc_argv.c` | Command-line arguments |

## Requirements

- Rust (stable)
- musl-gcc (for libc tests)
- LLVM lit and FileCheck (for running tests)

## Limitations

- x86_64 Linux only
- No dynamic linking
- No debug info (DWARF)
- No linker scripts
- No LTO

## License

MIT
