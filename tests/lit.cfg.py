import lit.formats
import os

config.name = 'uld'
config.test_format = lit.formats.ShTest(True)

config.suffixes = ['.c', '.s']

# Source directory - only look in cases/
config.test_source_root = os.path.join(os.path.dirname(__file__), 'cases')

# Project root (two levels up from tests/cases/)
project_root = os.path.abspath(os.path.join(config.test_source_root, '..', '..'))

# Build/Output directory - put in target/lit to keep repo clean
config.test_exec_root = os.path.join(project_root, 'target', 'lit')

# Substitutions
uld_path = os.path.join(project_root, 'target', 'debug', 'uld')

# Check if uld exists, warn if not (or let it fail later)
if not os.path.exists(uld_path):
    print(f"Warning: uld binary not found at {uld_path}. Did you run 'cargo build'?")

# Support directory (sibling of cases/)
support_dir = os.path.join(os.path.dirname(__file__), 'support')

config.substitutions.append(('%uld', uld_path))
config.substitutions.append(('%cc', 'musl-gcc'))
config.substitutions.append(('%as', 'as'))
config.substitutions.append(('%start', os.path.join(support_dir, 'start.s')))
config.substitutions.append(('%helper', os.path.join(support_dir, 'c_helper.c')))
config.substitutions.append(('%filecheck', 'filecheck'))

# musl libc CRT files for static linking
# Try common paths: Fedora uses lib64, Ubuntu uses lib
musl_lib_paths = [
    '/usr/x86_64-linux-musl/lib64',  # Fedora
    '/usr/lib/x86_64-linux-musl',     # Ubuntu
]
musl_lib = next((p for p in musl_lib_paths if os.path.exists(p)), musl_lib_paths[0])

config.substitutions.append(('%crt1', os.path.join(musl_lib, 'crt1.o')))
config.substitutions.append(('%crti', os.path.join(musl_lib, 'crti.o')))
config.substitutions.append(('%crtn', os.path.join(musl_lib, 'crtn.o')))
config.substitutions.append(('%libc', os.path.join(musl_lib, 'libc.a')))