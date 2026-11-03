#!/usr/bin/env bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
#
# Release build of fabstir-llm-node with every build path remapped to a fixed prefix, so
# the binary does not depend on where the checkout, the target directory or the cargo home
# happen to live (Phase 5 gap G-21, docs/development/PHASE5-KNOWN-GAPS-TOFU.md). Measured
# 2026-09-28: without this, two builds of the same commit in different directories differ
# (the source path is embedded about 29 times); with it they agree everywhere except the
# CUDA kernels, whose nvcc temp names carry a process id (the open part of G-21).
#
#   scripts/build-release.sh            # = cargo build --locked --release --features real-ezkl -j 4
#   JOBS=2 scripts/build-release.sh     # a from-scratch build in the 8 GiB dev container
#                                       # (risc0-circuit-keccak-sys OOMs at -j 4)
#
# --locked: Cargo.lock is committed and the build must not re-resolve it.
# RISC0_BUILD_LOCKED=1: the risc0 guest build honours methods/guest/Cargo.lock too.
# Any RUSTFLAGS/CFLAGS/CXXFLAGS already set are kept; the remaps are appended.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
CARGO_HOME_DIR="${CARGO_HOME:-$HOME/.cargo}"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"

# rustc applies the LAST matching remap, so the target dir (which may sit inside ROOT) goes
# after ROOT; both map to the same place when it does.
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$ROOT=/build --remap-path-prefix=$TARGET_DIR=/build/target --remap-path-prefix=$CARGO_HOME_DIR=/cargo --remap-path-prefix=$HOME=/home"
MAP="-ffile-prefix-map=$ROOT=/build -ffile-prefix-map=$TARGET_DIR=/build/target -ffile-prefix-map=$CARGO_HOME_DIR=/cargo"
export CFLAGS="${CFLAGS:-} $MAP"
export CXXFLAGS="${CXXFLAGS:-} $MAP"
export RISC0_BUILD_LOCKED=1

exec cargo build --locked --release --features real-ezkl -j "${JOBS:-4}" "$@"
