#!/bin/bash
# Generate EZKL Proving and Verification Keys
#
# This script generates the cryptographic keys needed for EZKL proof generation.
# It should be run once during setup, and the keys should be stored securely.
#
# Usage:
#   ./scripts/generate_ezkl_keys.sh [--output-dir ./keys]

set -e  # Exit on error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Default output directory
OUTPUT_DIR="./keys"

# Parse arguments
while [[ $# -gt 0 ]]; do
  case $1 in
    --output-dir)
      OUTPUT_DIR="$2"
      shift 2
      ;;
    --help|-h)
      echo "Usage: $0 [--output-dir ./keys]"
      echo ""
      echo "Generate EZKL proving and verification keys"
      echo ""
      echo "Options:"
      echo "  --output-dir DIR    Output directory for keys (default: ./keys)"
      echo "  --help, -h          Show this help message"
      exit 0
      ;;
    *)
      echo -e "${RED}Unknown option: $1${NC}"
      exit 1
      ;;
  esac
done

echo -e "${GREEN}🔐 EZKL Key Generation${NC}"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "Output directory: ${OUTPUT_DIR}"
echo ""

# Create output directory
mkdir -p "${OUTPUT_DIR}"
echo -e "${GREEN}✓${NC} Created output directory"

# Check if keys already exist
PROVING_KEY="${OUTPUT_DIR}/proving_key.bin"
VERIFYING_KEY="${OUTPUT_DIR}/verifying_key.bin"

if [ -f "${PROVING_KEY}" ] || [ -f "${VERIFYING_KEY}" ]; then
  echo -e "${YELLOW}⚠ Warning: Keys already exist${NC}"
  echo ""
  echo "Existing keys:"
  [ -f "${PROVING_KEY}" ] && echo "  - ${PROVING_KEY}"
  [ -f "${VERIFYING_KEY}" ] && echo "  - ${VERIFYING_KEY}"
  echo ""
  read -p "Overwrite existing keys? (y/N): " -n 1 -r
  echo ""
  if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo -e "${YELLOW}✗${NC} Key generation cancelled"
    exit 1
  fi
fi

echo ""
echo -e "${YELLOW}ℹ${NC}  Note: Currently using mock EZKL implementation"
echo "   Real EZKL requires nightly Rust and uncommenting dependencies in Cargo.toml"
echo ""

# Generate keys using Rust program
echo "Generating keys..."
cargo run --quiet --example generate_ezkl_keys -- --output-dir "${OUTPUT_DIR}" 2>&1 || {
  echo -e "${YELLOW}⚠${NC}  Example not found, generating mock keys directly..."

  # Generate mock proving key (1KB placeholder)
  dd if=/dev/urandom of="${PROVING_KEY}" bs=1024 count=1 status=none 2>/dev/null
  # Set first byte to 0xAA for mock validation
  printf '\xAA' | dd of="${PROVING_KEY}" bs=1 count=1 conv=notrunc status=none 2>/dev/null

  # Generate mock verification key (500 bytes placeholder)
  dd if=/dev/urandom of="${VERIFYING_KEY}" bs=500 count=1 status=none 2>/dev/null
  # Set first byte to 0xBB for mock validation
  printf '\xBB' | dd of="${VERIFYING_KEY}" bs=1 count=1 conv=notrunc status=none 2>/dev/null

  echo -e "${GREEN}✓${NC} Generated mock keys"
}

# Verify keys were created
if [ ! -f "${PROVING_KEY}" ] || [ ! -f "${VERIFYING_KEY}" ]; then
  echo -e "${RED}✗ Error: Key generation failed${NC}"
  exit 1
fi

# Display key information
echo ""
echo -e "${GREEN}✓ Key generation complete${NC}"
echo ""
echo "Generated keys:"
echo "  Proving key:      ${PROVING_KEY} ($(stat -f%z "${PROVING_KEY}" 2>/dev/null || stat -c%s "${PROVING_KEY}") bytes)"
echo "  Verification key: ${VERIFYING_KEY} ($(stat -f%z "${VERIFYING_KEY}" 2>/dev/null || stat -c%s "${VERIFYING_KEY}") bytes)"
echo ""

# Security reminder
echo -e "${YELLOW}⚠  SECURITY REMINDERS:${NC}"
echo "  1. Keep proving key secret (required for generating proofs)"
echo "  2. Verification key can be shared publicly"
echo "  3. Back up both keys securely"
echo "  4. Add keys/ directory to .gitignore"
echo ""

# Create .gitignore in keys directory
cat > "${OUTPUT_DIR}/.gitignore" << 'EOF'
# Ignore all key files
*.bin
*.key

# But track this .gitignore file
!.gitignore
EOF

echo -e "${GREEN}✓${NC} Created ${OUTPUT_DIR}/.gitignore to prevent committing keys"
echo ""
echo -e "${GREEN}🎉 Setup complete!${NC}"
echo ""
echo "Next steps:"
echo "  1. Set environment variables:"
echo "     export EZKL_PROVING_KEY_PATH=${PWD}/${PROVING_KEY}"
echo "     export EZKL_VERIFYING_KEY_PATH=${PWD}/${VERIFYING_KEY}"
echo ""
echo "  2. Test key loading:"
echo "     cargo test --lib crypto::ezkl::setup"
echo ""
