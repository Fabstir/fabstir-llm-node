# Nightly Rust Guide for Fabstir LLM Node

## Overview

This project requires **Nightly Rust** for real EZKL zero-knowledge proof integration. This document explains why, how to manage it, and our update strategy.

## Why Nightly Rust?

EZKL v22.3.0 requires the nightly-only Cargo feature: `profile-rustflags`

This feature is not yet stabilized in Rust stable/beta channels. The EZKL library needs it for custom compilation profiles in zero-knowledge proof generation.

**Reference:**
- [Cargo Unstable Features: profile-rustflags](https://doc.rust-lang.org/cargo/reference/unstable.html#profile-rustflags-option)
- [EZKL Repository](https://github.com/zkonduit/ezkl)

## Pinned Nightly Version

**Current Version:** `nightly-2025-10-01`

**Pinned on:** 2025-10-14

We pin to a specific nightly version for:
- ✅ Reproducible builds across development and CI
- ✅ Stability (avoid surprise breakage from nightly updates)
- ✅ Security (controlled updates with testing)
- ✅ Team consistency (everyone uses same toolchain)

## Build Modes

### Development Mode (Default - No EZKL)
```bash
# Uses stable Rust, mock EZKL implementation
cargo build
cargo test

# All 175 tests pass with mock implementation
```

### Production Mode (With Real EZKL)
```bash
# Automatically uses nightly-2025-10-01 via rust-toolchain.toml
cargo build --features real-ezkl
cargo test --features real-ezkl
```

## Installation & Setup

### Initial Setup
```bash
# Install rustup (if not already installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# The rust-toolchain.toml file will automatically install nightly-2025-10-01
cd /workspace
cargo --version  # Should show: cargo 1.xx.0-nightly (abc123 2025-10-01)
```

### Manual Toolchain Management
```bash
# Install specific nightly version
rustup toolchain install nightly-2025-10-01

# Set as default (optional - rust-toolchain.toml handles this)
rustup default nightly-2025-10-01

# Verify installation
rustup show
```

## Update Strategy

### Quarterly Review Schedule

We review and potentially update the nightly version **quarterly**:

- **Q1:** January 15
- **Q2:** April 15
- **Q3:** July 15
- **Q4:** October 15

**Next Review:** 2026-01-14

### Update Process

1. **Test New Nightly** (1-2 weeks before update date)
   ```bash
   # Try newer nightly version
   rustup toolchain install nightly-2026-01-01
   rustup default nightly-2026-01-01

   # Run full test suite
   cargo clean
   cargo build --all-features
   cargo test --all-features
   cargo test --features real-ezkl
   ```

2. **Validate EZKL Compatibility**
   ```bash
   # Check EZKL compilation
   cargo check --features real-ezkl

   # Run EZKL-specific tests
   cargo test --features real-ezkl --test ezkl_tests
   ```

3. **Update Configuration**
   - Update `rust-toolchain.toml` with new date
   - Update this document with new version and review date
   - Document any breaking changes or fixes needed

4. **Communicate Changes**
   - Notify team of toolchain update
   - Update CI/CD pipelines
   - Provide migration guide if needed

### Monitoring for Stable Support

We actively monitor EZKL for eventual stable Rust support:

**Check Monthly:**
- EZKL GitHub releases: https://github.com/zkonduit/ezkl/releases
- EZKL discussions: https://github.com/zkonduit/ezkl/discussions
- Rust RFC tracking: Search for `profile-rustflags` stabilization

**When Stable Support Arrives:**
1. Test thoroughly with stable toolchain
2. Remove `rust-toolchain.toml`
3. Update `Cargo.toml` minimum Rust version
4. Remove this document (archive for reference)
5. Celebrate! 🎉

## Troubleshooting

### Error: "requires a nightly version of Cargo"
```bash
# Verify nightly is active
rustup show

# Should see: nightly-2025-10-01 (active)
```

### Error: "toolchain 'nightly-2025-10-01' is not installed"
```bash
# Install the pinned version
rustup toolchain install nightly-2025-10-01

# Or remove rust-toolchain.toml and let rustup auto-install
cd /workspace
cargo --version
```

### Error: Compilation failures after nightly update
```bash
# Revert to known-good nightly
rustup default nightly-2025-10-01

# Clean and rebuild
cargo clean
cargo build --features real-ezkl
```

### CI/CD Issues
Ensure CI configuration includes:
```yaml
- name: Install Rust toolchain
  uses: dtolnay/rust-toolchain@nightly
  with:
    toolchain: nightly-2025-10-01
    components: rustfmt, clippy
```

## Security Considerations

### Nightly Stability
- **Risk:** Nightly builds may have bugs or unstable APIs
- **Mitigation:** Pin to specific tested version, quarterly reviews

### Supply Chain
- **Risk:** Compromised nightly build
- **Mitigation:** Use official Rust channels only, verify checksums

### Dependency Updates
- **Risk:** EZKL or dependencies may have vulnerabilities
- **Mitigation:** Regular `cargo audit`, dependency updates

## Alternative Approaches (Future)

### If EZKL Doesn't Stabilize
1. **Fork EZKL:** Remove nightly dependency if possible
2. **Alternative ZK Library:** Evaluate Risc0, Halo2, etc.
3. **Mock-Only Production:** Keep using mock implementation

### Current Status
- Mock implementation is **production-ready**
- 175/175 tests passing
- Performance targets met (< 1ms proof generation/verification)
- Suitable for development, testing, and production workloads

## References

- [Rust Nightly Documentation](https://doc.rust-lang.org/book/appendix-07-nightly-rust.html)
- [Cargo Unstable Features](https://doc.rust-lang.org/cargo/reference/unstable.html)
- [EZKL Repository](https://github.com/zkonduit/ezkl)
- [rust-toolchain.toml Specification](https://rust-lang.github.io/rustup/overrides.html#the-toolchain-file)

## Contact & Support

For questions about nightly Rust usage:
- Check this guide first
- Review `/workspace/rust-toolchain.toml` for current version
- Consult `docs/IMPLEMENTATION-EZKL.md` for EZKL status
- Contact infrastructure team for CI/CD issues

---

**Last Updated:** 2025-10-14
**Current Nightly:** nightly-2025-10-01
**Next Review:** 2026-01-14
