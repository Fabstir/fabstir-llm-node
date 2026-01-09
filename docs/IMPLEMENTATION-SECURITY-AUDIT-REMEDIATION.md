# IMPLEMENTATION - Security Audit Remediation (January 2026)

## Status: Phase 1 Pending

**Version**: v8.10.2-security-audit-remediation (target)
**Current Version**: v8.10.1-incremental-content-hash
**Start Date**: 2026-01-09
**Approach**: Strict TDD with bounded autonomy - one sub-phase at a time

---

## Overview

This implementation updates the node software for the January 9, 2026 contract breaking changes from the security audit remediation. The main change is a **new JobMarketplace proxy address** due to a clean slate deployment.

### What's Already Done (v8.9.1)

| Component | Status | Notes |
|-----------|--------|-------|
| Proof signing | ✅ Complete | `src/crypto/proof_signer.rs` |
| submitProofOfWork with signature | ✅ Complete | 65-byte signature parameter |
| EIP-191 personal_sign prefix | ✅ Complete | v8.9.1 hotfix |
| verifyHostSignature rename | N/A | Node doesn't call ProofSystem directly |
| Updated ABIs | ✅ Complete | `docs/compute-contracts-reference/client-abis/` |

### What Needs Updating

| Component | Current | Required |
|-----------|---------|----------|
| JobMarketplace address | `0xeebEEbc9BCD35e81B06885b63f980FeC71d56e2D` | `0x3CaCbf3f448B420918A93a88706B26Ab27a3523E` |
| Hardcoded fallback in chain_config.rs | Old address | New address |
| CLAUDE.md documentation | Old address | New address |
| Various docs | Old address | New address |
| Version | 8.9.1 | 8.9.2 |

---

## Contract Address Change Summary

```
┌─────────────────────────────────────────────────────────────────────────┐
│  JobMarketplace Proxy Address Change (January 9, 2026)                  │
├─────────────────────────────────────────────────────────────────────────┤
│                                                                         │
│  OLD (December 14, 2025):                                               │
│    0xeebEEbc9BCD35e81B06885b63f980FeC71d56e2D                           │
│    └── DEPRECATED - Do not use                                          │
│                                                                         │
│  NEW (January 9, 2026 - Security Audit Clean Slate):                    │
│    0x3CaCbf3f448B420918A93a88706B26Ab27a3523E                           │
│    └── ACTIVE - Use this address                                        │
│                                                                         │
│  Other contracts UNCHANGED:                                             │
│    NodeRegistry:   0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22           │
│    ProofSystem:    0x5afB91977e69Cc5003288849059bc62d47E7deeb           │
│    HostEarnings:   0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0           │
│    ModelRegistry:  0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2           │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

---

## Implementation Status

| Phase | Sub-phase | Description | Status | Tests |
|-------|-----------|-------------|--------|-------|
| 1 | 1.1 | Update chain_config.rs address | ✅ Complete | 7/7 |
| 1 | 1.2 | Update chain_config.rs tests | ✅ Complete | 7/7 |
| 2 | 2.1 | Update CLAUDE.md | ✅ Complete | N/A |
| 2 | 2.2 | Update client-abis docs | ✅ Complete | N/A |
| 2 | 2.3 | Update other documentation | ✅ Complete | N/A |
| 3 | 3.1 | Bump version files | ✅ Complete | 3/3 |
| 3 | 3.2 | Run test suite | ✅ Complete | 17/17 |
| 4 | 4.1 | Build release binary | ⏳ Pending | N/A |
| 4 | 4.2 | Create release tarball | ⏳ Pending | N/A |
| **Total** | | | **80%** | **17/17** |

---

## Phase 1: Update Contract Address in Code

### Sub-phase 1.1: Update chain_config.rs Address

**Goal**: Update the hardcoded JobMarketplace address to the new January 2026 address

**Status**: ✅ Complete (2026-01-09)

**File**: `src/blockchain/chain_config.rs`

**Tasks**:
- [x] Update line 47 comment to reference January 9, 2026 Security Audit
- [x] Update line 48 `job_marketplace` address to `0x3CaCbf3f448B420918A93a88706B26Ab27a3523E`
- [x] Run `cargo check` to verify code compiles
- [x] Run `cargo test chain_config` - 7/7 tests passing

**Before**:
```rust
// Line 47-48
// Updated December 14, 2025 for UUPS Upgradeable proxy contracts (v8.5.0)
job_marketplace: "0xeebEEbc9BCD35e81B06885b63f980FeC71d56e2D".to_string(),
```

**After**:
```rust
// Line 47-48
// Updated January 9, 2026 for Security Audit Remediation (clean slate deployment)
job_marketplace: "0x3CaCbf3f448B420918A93a88706B26Ab27a3523E".to_string(),
```

---

### Sub-phase 1.2: Update chain_config.rs Tests

**Goal**: Update test constants and assertions to use new address

**Status**: ✅ Complete (2026-01-09)

**File**: `src/blockchain/chain_config.rs`

**Tasks**:
- [x] Update line 161-162 `NEW_JOB_MARKETPLACE` constant to new address
- [x] Update line 162 comment to reference January 9, 2026 Security Audit
- [x] Update line 171 test assertion message
- [x] Run `cargo test chain_config` - 7/7 tests passing

**Before**:
```rust
// Lines 161-162
/// New JobMarketplace contract address (UUPS Proxy)
const NEW_JOB_MARKETPLACE: &str = "0xeebEEbc9BCD35e81B06885b63f980FeC71d56e2D";
```

**After**:
```rust
// Lines 161-162
/// New JobMarketplace contract address (UUPS Proxy - Jan 9, 2026 Security Audit)
const NEW_JOB_MARKETPLACE: &str = "0x3CaCbf3f448B420918A93a88706B26Ab27a3523E";
```

**Test Assertions to Update**:
```rust
// Line 170-172
assert_eq!(
    config.contracts.job_marketplace, NEW_JOB_MARKETPLACE,
    "JobMarketplace should be updated to Jan 9, 2026 security audit contract"
);
```

**Verification**:
```bash
cargo test chain_config -- --nocapture
# Expected: 6/6 tests passing
```

---

## Phase 2: Update Documentation

### Sub-phase 2.1: Update CLAUDE.md

**Goal**: Update the main project documentation with new address

**Status**: ✅ Complete (2026-01-09)

**File**: `/workspace/CLAUDE.md`

**Tasks**:
- [x] Update line 264 version reference to v8.9.2
- [x] Update line 266 CONTRACT_JOB_MARKETPLACE address to `0x3CaCbf3f448B420918A93a88706B26Ab27a3523E`
- [x] Add old address to deprecated list

**Before**:
```markdown
- **CONTRACT_JOB_MARKETPLACE**: `0xeebEEbc9BCD35e81B06885b63f980FeC71d56e2D` (UUPS Proxy)
```

**After**:
```markdown
- **CONTRACT_JOB_MARKETPLACE**: `0x3CaCbf3f448B420918A93a88706B26Ab27a3523E` (UUPS Proxy - Jan 9, 2026)
```

---

### Sub-phase 2.2: Update client-abis Documentation

**Goal**: Update ABI documentation with new address and changelog entry

**Status**: ⏳ Pending

**Files**:
- `docs/compute-contracts-reference/client-abis/README.md`
- `docs/compute-contracts-reference/client-abis/CHANGELOG.md`

**Tasks**:
- [ ] Update README.md line 13 proxy address
- [ ] Update README.md line 85 JavaScript example address
- [ ] Add new CHANGELOG.md entry for January 9, 2026 deployment

**CHANGELOG Entry to Add**:
```markdown
## January 9, 2026 - Security Audit Remediation

### Contract Changes

| Contract | Proxy Address | Implementation |
|----------|---------------|----------------|
| JobMarketplace | `0x3CaCbf3f448B420918A93a88706B26Ab27a3523E` ⚠️ NEW | `0x26f27C19F80596d228D853dC39A204f0f6C45C7E` |

### Breaking Changes
- **JobMarketplace proxy address changed** - Clean slate deployment for security audit
- **submitProofOfWork** now requires 5th parameter: 65-byte signature (already in v8.9.1)

### ABI Updates
- `JobMarketplaceWithModelsUpgradeable-CLIENT-ABI.json` - submitProofOfWork signature parameter
- `ProofSystemUpgradeable-CLIENT-ABI.json` - verifyEKZL renamed to verifyHostSignature
```

---

### Sub-phase 2.3: Update Other Documentation

**Goal**: Update remaining documentation files with new address

**Status**: ⏳ Pending

**Files**:
- `docs/compute-contracts-reference/SECURITY-AUDIT-NODE-MIGRATION.md`

**Tasks**:
- [ ] Update line 153 (TypeScript example address)
- [ ] Update line 231 (Python example address)
- [ ] Update line 571 (verification address)
- [ ] Verify no other files reference old address: `grep -r "0xeebEEbc9BCD35e81B06885b63f980FeC71d56e2D" docs/`

---

## Phase 3: Version Update

### Sub-phase 3.1: Bump Version Files

**Goal**: Update version to 8.9.2 to reflect contract address change

**Status**: ⏳ Pending

**Files**:
- `/workspace/VERSION`
- `/workspace/src/version.rs`

**Tasks**:
- [ ] Update `VERSION` file to `8.9.2-security-audit-remediation`
- [ ] Update `src/version.rs` VERSION constant
- [ ] Update `src/version.rs` VERSION_NUMBER to "8.9.2"
- [ ] Update `src/version.rs` VERSION_PATCH to 2
- [ ] Add BREAKING_CHANGES entry for contract address change
- [ ] Update test assertions in version.rs

**VERSION file**:
```
8.9.2-security-audit-remediation
```

**src/version.rs changes**:
```rust
pub const VERSION: &str = "v8.9.2-security-audit-remediation-2026-01-09";
pub const VERSION_NUMBER: &str = "8.9.2";
pub const VERSION_PATCH: u32 = 2;

// Add to BREAKING_CHANGES array:
"CONTRACT: JobMarketplace address updated to 0x3CaC...523E (Jan 9, 2026 Security Audit)",
```

---

### Sub-phase 3.2: Run Full Test Suite

**Goal**: Verify all tests pass with updated addresses

**Status**: ⏳ Pending

**Tasks**:
- [ ] Run `cargo test --lib` - unit tests
- [ ] Run `cargo test chain_config` - chain config tests (4 tests)
- [ ] Run `cargo test proof_signer` - proof signing tests (10 tests)
- [ ] Run `cargo test --test contracts_tests` - contract integration tests
- [ ] Verify total test count matches expectations

**Expected Test Results**:
```bash
cargo test --lib
# Expected: All tests pass

cargo test chain_config
# Expected: 6/6 tests pass

cargo test proof_signer
# Expected: 10/10 tests pass
```

---

## Phase 4: Build and Release

### Sub-phase 4.1: Build Release Binary

**Goal**: Build production binary with new address

**Status**: ⏳ Pending

**Tasks**:
- [ ] Run `cargo build --release --features real-ezkl -j 4`
- [ ] Verify version in binary: `strings target/release/fabstir-llm-node | grep "v8.9.2"`
- [ ] Verify new address in binary: `strings target/release/fabstir-llm-node | grep "0x3CaCbf3f448B420918A93a88706B26Ab27a3523E"`
- [ ] Verify old address NOT in binary: `strings target/release/fabstir-llm-node | grep "0xeebEEbc9BCD35e81B06885b63f980FeC71d56e2D"` (should return empty)

**Build Command**:
```bash
cargo build --release --features real-ezkl -j 4
```

**Verification**:
```bash
# Check version
strings target/release/fabstir-llm-node | grep "v8.9.2"
# Expected: v8.9.2-security-audit-remediation-2026-01-09

# Check new address is embedded
strings target/release/fabstir-llm-node | grep "0x3CaCbf3f448B420918A93a88706B26Ab27a3523E"
# Expected: Address found

# Check old address is NOT embedded
strings target/release/fabstir-llm-node | grep "0xeebEEbc9BCD35e81B06885b63f980FeC71d56e2D"
# Expected: No output (old address removed)
```

---

### Sub-phase 4.2: Create Release Tarball

**Goal**: Package release binary for deployment

**Status**: ⏳ Pending

**Tasks**:
- [ ] Copy binary to root: `cp target/release/fabstir-llm-node ./fabstir-llm-node`
- [ ] Create tarball with correct structure (binary at root, NOT in target/release/)
- [ ] Verify tarball contents
- [ ] Clean up temporary binary

**Tarball Command**:
```bash
# Copy binary to root first (CRITICAL!)
cp target/release/fabstir-llm-node ./fabstir-llm-node

# Create tarball with binary at root
tar -czvf fabstir-llm-node-v8.9.2-security-audit-remediation.tar.gz \
  fabstir-llm-node \
  scripts/download_florence_model.sh \
  scripts/download_ocr_models.sh \
  scripts/download_embedding_model.sh \
  scripts/setup_models.sh

# Verify contents
tar -tzvf fabstir-llm-node-v8.9.2-security-audit-remediation.tar.gz

# Clean up
rm ./fabstir-llm-node
```

**Expected Tarball Contents**:
```
fabstir-llm-node                       (binary at ROOT - CRITICAL!)
scripts/download_florence_model.sh
scripts/download_ocr_models.sh
scripts/download_embedding_model.sh
scripts/setup_models.sh
```

---

## Post-Deployment Verification

### Verify Node Connects to Correct Contract

```bash
# Start node and check logs for:
#   JobMarketplace: 0x3CaCbf3f448B420918A93a88706B26Ab27a3523E
```

### Verify Contract On-Chain

```bash
# Check contract is responding
cast call 0x3CaCbf3f448B420918A93a88706B26Ab27a3523E "nextJobId()" --rpc-url https://sepolia.base.org

# Check contract owner (should be valid address)
cast call 0x3CaCbf3f448B420918A93a88706B26Ab27a3523E "owner()" --rpc-url https://sepolia.base.org
```

---

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Wrong address deployed | Low | High | Verify with cast call before release |
| Tests fail with new address | Medium | Low | Tests use constants, update together |
| Old sessions on old contract | Expected | None | New proxy = clean slate, expected behavior |
| Env var overrides fallback | N/A | None | `CONTRACT_JOB_MARKETPLACE` env var takes precedence |

---

## Files Modified Summary

| File | Phase | Change Type |
|------|-------|-------------|
| `src/blockchain/chain_config.rs` | 1.1, 1.2 | Code + Tests |
| `CLAUDE.md` | 2.1 | Documentation |
| `docs/compute-contracts-reference/client-abis/README.md` | 2.2 | Documentation |
| `docs/compute-contracts-reference/client-abis/CHANGELOG.md` | 2.2 | Documentation |
| `docs/compute-contracts-reference/SECURITY-AUDIT-NODE-MIGRATION.md` | 2.3 | Documentation |
| `VERSION` | 3.1 | Version |
| `src/version.rs` | 3.1 | Version + Tests |

---

## Estimated Time

| Phase | Sub-phase | Description | Est. Time |
|-------|-----------|-------------|-----------|
| 1 | 1.1 | Update chain_config.rs address | 10 min |
| 1 | 1.2 | Update chain_config.rs tests | 10 min |
| 2 | 2.1 | Update CLAUDE.md | 5 min |
| 2 | 2.2 | Update client-abis docs | 15 min |
| 2 | 2.3 | Update other documentation | 10 min |
| 3 | 3.1 | Bump version files | 15 min |
| 3 | 3.2 | Run full test suite | 20 min |
| 4 | 4.1 | Build release binary | 30 min |
| 4 | 4.2 | Create release tarball | 10 min |
| **Total** | | | **~2 hours** |

---

## References

- Migration Guide: `docs/compute-contracts-reference/NODE-MIGRATION-JAN2026.md`
- Breaking Changes: `docs/compute-contracts-reference/BREAKING_CHANGES.md`
- Updated ABIs: `docs/compute-contracts-reference/client-abis/`
- Proof Signing Implementation: `docs/IMPLEMENTATION_PROOF_SIGNING.md`
- Environment Config: `.env.local.test`
