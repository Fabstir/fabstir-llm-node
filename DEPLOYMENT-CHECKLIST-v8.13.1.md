# Deployment Checklist: v8.13.1-env-vars-required

**Version**: v8.13.1-env-vars-required
**Date**: February 1, 2026
**Breaking Change**: YES - Requires all contract addresses in .env file

---

## ⚠️ CRITICAL BREAKING CHANGE

**This version will NOT start without a properly configured `.env` file!**

The node will panic with clear error messages if any required environment variables are missing.

---

## Pre-Deployment Checklist

### 1. Extract Tarball on Host

```bash
# Extract tarball
tar -xzf fabstir-llm-node-v8.13.1-env-vars-required.tar.gz

# Verify binary is at root (NOT in target/release/)
ls -lh fabstir-llm-node
```

### 2. Create .env File

```bash
# Copy example to .env
cp .env.contracts .env

# Add RPC URL
echo "BASE_SEPOLIA_RPC_URL=https://sepolia.base.org" >> .env

# Or use your own RPC provider
echo "BASE_SEPOLIA_RPC_URL=https://base-sepolia.g.alchemy.com/v2/YOUR_API_KEY" >> .env
```

### 3. Validate Configuration

```bash
# Run validation script
chmod +x scripts/validate-env.sh
./scripts/validate-env.sh
```

**Expected output**:
```
✅ Checking required environment variables:
  ✅ BASE_SEPOLIA_RPC_URL = https://sepolia.base.org
  ✅ CONTRACT_JOB_MARKETPLACE = 0x95132177F964FF053C1E874b53CF74d819618E06
  ✅ CONTRACT_PROOF_SYSTEM = 0xE8DCa89e1588bbbdc4F7D5F78263632B35401B31
  ...
🔒 Checking for deprecated contract addresses:
  ✅ JobMarketplace: Using AUDIT-F4 remediated contract
  ✅ ProofSystem: Using AUDIT-F4 remediated contract

✅ Environment configuration is valid!
```

### 4. Copy Binary to Docker Expected Location

```bash
# Docker expects binary at target/release/fabstir-llm-node
mkdir -p target/release
cp fabstir-llm-node target/release/
```

### 5. Verify Binary Version

```bash
strings fabstir-llm-node | grep "v8.13"
# Should show: v8.13.1-env-vars-required-2026-02-01
```

---

## Deployment Steps

### 6. Stop Current Node

```bash
docker-compose down
```

### 7. Backup Current Configuration

```bash
# Backup current .env
cp .env .env.backup.$(date +%Y%m%d)

# Backup current binary
cp target/release/fabstir-llm-node target/release/fabstir-llm-node.backup
```

### 8. Deploy New Binary

```bash
# Copy new binary
cp fabstir-llm-node target/release/fabstir-llm-node

# Verify permissions
chmod +x target/release/fabstir-llm-node
```

### 9. Update .env File (if needed)

Ensure your `.env` has all required variables:

```bash
# Required variables
BASE_SEPOLIA_RPC_URL=https://sepolia.base.org
CONTRACT_JOB_MARKETPLACE=0x95132177F964FF053C1E874b53CF74d819618E06
CONTRACT_PROOF_SYSTEM=0xE8DCa89e1588bbbdc4F7D5F78263632B35401B31
CONTRACT_NODE_REGISTRY=0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22
CONTRACT_HOST_EARNINGS=0xE4F33e9e132E60fc3477509f99b9E1340b91Aee0
CONTRACT_MODEL_REGISTRY=0x1a9d91521c85bD252Ac848806Ff5096bBb9ACDb2
USDC_TOKEN=0x036CbD53842c5426634e7929541eC2318f3dCF7e
FAB_TOKEN=0xC78949004B4EB6dEf2D66e49Cd81231472612D62
```

### 10. Start Node

```bash
docker-compose up -d
```

---

## Post-Deployment Verification

### 11. Check Container Status

```bash
docker ps | grep llm-node
# Should show container running
```

### 12. Check Logs for Startup

```bash
docker logs llm-node-prod | tail -50
```

**Look for**:
- ✅ No panic messages about missing env vars
- ✅ Contract addresses in logs match AUDIT-F4 addresses
- ✅ Node started successfully

**Red flags**:
- ❌ `CONTRACT_JOB_MARKETPLACE environment variable is required`
- ❌ `BASE_SEPOLIA_RPC_URL environment variable is required`
- ❌ Any panic messages

### 13. Verify Contract Addresses

```bash
docker logs llm-node-prod | grep "0x9513"
# Should show: 0x95132177F964FF053C1E874b53CF74d819618E06 (JobMarketplace)

docker logs llm-node-prod | grep "0xE8DC"
# Should show: 0xE8DCa89e1588bbbdc4F7D5F78263632B35401B31 (ProofSystem)
```

### 14. Verify Version

```bash
docker exec llm-node-prod ./fabstir-llm-node --version
# Should show: v8.13.1-env-vars-required-2026-02-01
```

### 15. Test API Endpoint

```bash
curl http://localhost:8080/health
# Should return: {"status":"ok"}
```

---

## Rollback Procedure (if needed)

If the node fails to start or has issues:

```bash
# 1. Stop container
docker-compose down

# 2. Restore previous binary
cp target/release/fabstir-llm-node.backup target/release/fabstir-llm-node

# 3. Restore previous .env (if changed)
cp .env.backup.YYYYMMDD .env

# 4. Restart
docker-compose up -d
```

---

## Common Issues

### Issue: Node panics on startup

**Error**: `CONTRACT_JOB_MARKETPLACE environment variable is required`

**Fix**:
```bash
# Check .env file exists
ls -la .env

# Run validation script
./scripts/validate-env.sh

# Ensure all required vars are set
cat .env | grep CONTRACT_
```

### Issue: Using deprecated contract

**Error**: Validation script shows:
```
❌ ERROR: Using deprecated JobMarketplace contract!
   Current:  0x3CaCbf3f448B420918A93a88706B26Ab27a3523E
   Required: 0x95132177F964FF053C1E874b53CF74d819618E06
```

**Fix**:
```bash
# Update .env with AUDIT-F4 addresses
cp .env.contracts .env
echo "BASE_SEPOLIA_RPC_URL=https://sepolia.base.org" >> .env

# Re-validate
./scripts/validate-env.sh
```

### Issue: Docker can't find binary

**Error**: `exec: "./fabstir-llm-node": stat ./fabstir-llm-node: no such file or directory`

**Fix**:
```bash
# Binary must be at target/release/fabstir-llm-node
mkdir -p target/release
cp fabstir-llm-node target/release/
chmod +x target/release/fabstir-llm-node
```

---

## Success Criteria

All of these should be true:

- ✅ `docker ps` shows container running
- ✅ No panic messages in logs
- ✅ Logs show AUDIT-F4 contract addresses (0x9513..., 0xE8DC...)
- ✅ Version shows v8.13.1-env-vars-required
- ✅ Health endpoint returns 200 OK
- ✅ No "Using deprecated contract" warnings
- ✅ Validation script passes

---

## Support

If issues persist:

1. Check full logs: `docker logs llm-node-prod > node.log`
2. Run validation script: `./scripts/validate-env.sh`
3. Verify .env file: `cat .env | grep CONTRACT_`
4. Check binary version: `strings target/release/fabstir-llm-node | grep v8.13`

---

## Tarball Contents

```
fabstir-llm-node                              # Binary (990MB)
scripts/download_embedding_model.sh           # Download all-MiniLM-L6-v2
scripts/validate-env.sh                       # Validate configuration
.env.contracts                                # Contract addresses
.env.prod.example                             # Example .env file
docs/MIGRATION-ENV-VARS-REQUIRED.md          # Migration guide
ALL-FIXES-v8.4.4-ENV-VARS-REQUIRED.md        # Implementation summary
```

---

## Key Changes in v8.13.1

1. **BREAKING**: All contract addresses now REQUIRED via environment variables
2. **BREAKING**: No hardcoded fallbacks - node will panic if any var is missing
3. **NEW**: Validation script to check configuration before deployment
4. **NEW**: Clear error messages showing which variable is missing
5. **AUDIT**: Enforces use of AUDIT-F4 remediated contracts
6. **SECURITY**: Prevents accidental use of deprecated contracts

---

**Status**: Ready for deployment ✅
