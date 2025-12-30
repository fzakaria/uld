import lit.formats
import os

config.name = 'uld'
config.test_format = lit.formats.ShTest(True)

config.suffixes = ['.c']

# Source directory
config.test_source_root = os.path.dirname(__file__)
# Build/Output directory (same as source for now or a temp dir)
config.test_exec_root = os.path.join(config.test_source_root, 'output')

# Substitutions
config.substitutions.append(('%cargo_run', 'cargo run --quiet --'))
config.substitutions.append(('%clang', 'clang'))
config.substitutions.append(('%filecheck', 'FileCheck'))
