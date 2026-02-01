#!/bin/bash
# validate-env.sh
# Validates that all required environment variables are set for Fabstir LLM Node
# Usage: ./scripts/validate-env.sh

set -e

echo "🔍 Validating Fabstir LLM Node environment configuration..."
echo ""

ERRORS=0

# Required environment variables
REQUIRED_VARS=(
    "BASE_SEPOLIA_RPC_URL"
    "CONTRACT_JOB_MARKETPLACE"
    "CONTRACT_PROOF_SYSTEM"
    "CONTRACT_NODE_REGISTRY"
    "CONTRACT_HOST_EARNINGS"
    "CONTRACT_MODEL_REGISTRY"
    "USDC_TOKEN"
    "FAB_TOKEN"
)

# AUDIT-F4 Remediated addresses (correct values)
AUDIT_F4_JOB_MARKETPLACE="0x95132177F964FF053C1E874b53CF74d819618E06"
AUDIT_F4_PROOF_SYSTEM="0xE8DCa89e1588bbbdc4F7D5F78263632B35401B31"

# Deprecated addresses (should NOT be used)
DEPRECATED_JOB_MARKETPLACE="0x3CaCbf3f448B420918A93a88706B26Ab27a3523E"
DEPRECATED_PROOF_SYSTEM="0x5afB91977e69Cc5003288849059bc62d47E7deeb"

# Check each required variable
echo "✅ Checking required environment variables:"
for VAR in "${REQUIRED_VARS[@]}"; do
    if [ -z "${!VAR}" ]; then
        echo "  ❌ $VAR is NOT set"
        ERRORS=$((ERRORS + 1))
    else
        echo "  ✅ $VAR = ${!VAR}"
    fi
done

echo ""

# Check for deprecated contract addresses
echo "🔒 Checking for deprecated contract addresses:"
if [ "$CONTRACT_JOB_MARKETPLACE" = "$DEPRECATED_JOB_MARKETPLACE" ]; then
    echo "  ❌ ERROR: Using deprecated JobMarketplace contract!"
    echo "     Current:  $CONTRACT_JOB_MARKETPLACE"
    echo "     Required: $AUDIT_F4_JOB_MARKETPLACE (AUDIT-F4 remediated)"
    ERRORS=$((ERRORS + 1))
elif [ "$CONTRACT_JOB_MARKETPLACE" = "$AUDIT_F4_JOB_MARKETPLACE" ]; then
    echo "  ✅ JobMarketplace: Using AUDIT-F4 remediated contract"
fi

if [ "$CONTRACT_PROOF_SYSTEM" = "$DEPRECATED_PROOF_SYSTEM" ]; then
    echo "  ❌ ERROR: Using deprecated ProofSystem contract!"
    echo "     Current:  $CONTRACT_PROOF_SYSTEM"
    echo "     Required: $AUDIT_F4_PROOF_SYSTEM (AUDIT-F4 remediated)"
    ERRORS=$((ERRORS + 1))
elif [ "$CONTRACT_PROOF_SYSTEM" = "$AUDIT_F4_PROOF_SYSTEM" ]; then
    echo "  ✅ ProofSystem: Using AUDIT-F4 remediated contract"
fi

echo ""

# Check optional variables
echo "ℹ️  Checking optional environment variables:"
if [ -z "$MULTICALL3_ADDRESS" ]; then
    echo "  ⚠️  MULTICALL3_ADDRESS not set (will use default: 0xcA11...)"
else
    echo "  ✅ MULTICALL3_ADDRESS = $MULTICALL3_ADDRESS"
fi

echo ""

# Summary
if [ $ERRORS -eq 0 ]; then
    echo "✅ Environment configuration is valid!"
    echo ""
    echo "You can now start the node:"
    echo "  cargo run --release --features real-ezkl -j 4"
    exit 0
else
    echo "❌ Found $ERRORS error(s) in environment configuration"
    echo ""
    echo "To fix:"
    echo "  1. Copy .env.contracts to .env if not exists"
    echo "  2. Add BASE_SEPOLIA_RPC_URL to .env"
    echo "  3. Verify all addresses match AUDIT-F4 remediated contracts"
    echo ""
    echo "See docs/MIGRATION-ENV-VARS-REQUIRED.md for details"
    exit 1
fi
