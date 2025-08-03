#!/bin/bash
# Completely disable all caching mechanisms

# Unset all cache-related variables
unset RUSTC_WRAPPER
unset SCCACHE_DIR
unset SCCACHE_CACHE_SIZE
unset CMAKE_C_COMPILER_LAUNCHER
unset CMAKE_CXX_COMPILER_LAUNCHER

# Disable incremental compilation
export CARGO_INCREMENTAL=0

# Tell CMake not to use ccache/sccache
export GGML_CCACHE=OFF

# Remove sccache from PATH temporarily
export PATH=$(echo $PATH | tr ':' '\n' | grep -v sccache | tr '\n' ':')

echo "All caching disabled"
echo "RUSTC_WRAPPER=${RUSTC_WRAPPER:-unset}"
echo "GGML_CCACHE=${GGML_CCACHE}"
echo "PATH does not contain sccache"

# Run the command
exec "$@"