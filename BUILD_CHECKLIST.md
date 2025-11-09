# CRITICAL BUILD CHECKLIST

## ⚠️ BEFORE EVERY BUILD - READ THIS FIRST ⚠️

### MANDATORY Build Command

```bash
cargo build --release --features real-ezkl -j 4
```

**WHY EACH FLAG IS CRITICAL:**

1. `--release` - Optimized production binary
2. `--features real-ezkl` - **CRITICAL!** Embeds Risc0 guest program for STARK proofs
   - Without this: Checkpoint submissions FAIL with "No such file or directory"
   - Binary size: ~900MB with flag, ~150MB without (proves it's embedded)
3. `-j 4` - Limits parallel jobs to avoid OOM during Risc0 compilation

### ❌ NEVER DO THIS:
```bash
cargo build --release  # WRONG! Missing real-ezkl flag!
```

### Version Update Checklist (BEFORE BUILD)

**ALWAYS UPDATE ALL 3 FILES IN THIS ORDER:**

1. ✅ `/workspace/VERSION` file
2. ✅ `/workspace/src/version.rs`:
   - `VERSION` constant
   - `VERSION_NUMBER` constant
   - `VERSION_PATCH` constant
   - `BUILD_DATE` constant
   - Test assertions (3 tests)
3. ✅ Update `BREAKING_CHANGES` array

**THEN BUILD!**

### Post-Build Verification

```bash
# 1. Check binary size
ls -lh target/release/fabstir-llm-node
# Expected: ~900MB (if smaller, Risc0 NOT embedded!)

# 2. Verify Risc0 guest program
strings target/release/fabstir-llm-node | grep "commitment_guest"
# Expected: commitment_guest.XXXXX (if empty, REBUILD WITH --features real-ezkl!)

# 3. Verify version
strings target/release/fabstir-llm-node | grep "v8.3"
# Expected: See your version string

# 4. Verify chat templates
strings target/release/fabstir-llm-node | grep "MODEL_CHAT_TEMPLATE"
# Expected: MODEL_CHAT_TEMPLATE followed by defaultllama2vicunaharmonychatml
```

### Tarball Creation

```bash
tar -czf fabstir-llm-node-vX.X.X-description.tar.gz target/release/fabstir-llm-node
```

### Build History

- v8.3.6: ✅ Built with --features real-ezkl
- v8.3.7 (first): ❌ Built WITHOUT --features real-ezkl (WRONG!)
- v8.3.7 (fixed): ✅ Built with --features real-ezkl
- v8.3.8 (current): ??? CHECK NOW!

---

## 🚨 IF YOU FORGET --features real-ezkl 🚨

**Symptoms:**
- Binary size ~150MB instead of ~900MB
- `strings target/release/fabstir-llm-node | grep commitment_guest` returns NOTHING
- Production logs show: "Prover execution failed: No such file or directory"
- Checkpoint submissions FAIL

**Solution:**
- DELETE the binary
- Re-run: `cargo build --release --features real-ezkl -j 4`
- VERIFY with checklist above
- Create new tarball with DIFFERENT version number

---

**PRINT THIS AND TAPE IT TO YOUR MONITOR!**
