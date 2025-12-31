import lit.formats
import os

config.name = 'uld'
config.test_format = lit.formats.ShTest(True)

config.suffixes = ['.c', '.s']

# Source directory
config.test_source_root = os.path.dirname(__file__)

# Project root (one level up from tests/)
project_root = os.path.abspath(os.path.join(config.test_source_root, '..'))

# Build/Output directory - put in target/lit to keep repo clean
config.test_exec_root = os.path.join(project_root, 'target', 'lit')

# Substitutions
uld_path = os.path.join(project_root, 'target', 'debug', 'uld')

# Check if uld exists, warn if not (or let it fail later)
if not os.path.exists(uld_path):
    print(f"Warning: uld binary not found at {uld_path}. Did you run 'cargo build'?")

config.substitutions.append(('%uld', uld_path))
config.substitutions.append(('%clang', 'musl-clang'))
config.substitutions.append(('%as', 'as'))
config.substitutions.append(('%filecheck', 'filecheck'))