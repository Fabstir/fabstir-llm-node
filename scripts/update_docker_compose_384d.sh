#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1


# update_docker_compose_384d.sh
# Update docker-compose to use 384-dimensional vectors

echo "========================================="
echo "Updating Vector DB to 384 Dimensions"
echo "========================================="
echo ""

# Step 1: Go to vector DB directory
cd /root/dev/Fabstir/fabstir-vectordb

# Step 2: Check current VECTOR_DIMENSION setting
echo "Current VECTOR_DIMENSION setting:"
grep -n "VECTOR_DIMENSION" docker-compose.yml docker-compose.override.yml 2>/dev/null || echo "Not found in docker-compose files"
echo ""

# Step 3: Create or update docker-compose.override.yml with 384D setting
echo "Creating docker-compose.override.yml with 384D configuration..."
cat > docker-compose.override.yml << 'EOF'
# docker-compose.override.yml
# Override configuration for 384-dimensional vectors

services:
  fabstir-ai-vector-db:
    environment:
      # Override to use 384-dimensional vectors (for all-MiniLM-L6-v2)
      - VECTOR_DIMENSION=384
      # Ensure connection to Enhanced S5.js
      - S5_MODE=real
      - S5_PORTAL_URL=http://host.docker.internal:5524
      - S5_MOCK_SERVER_URL=http://host.docker.internal:5524
      # Keep other settings
      - HAMT_ACTIVATION_THRESHOLD=1000
      - RUST_LOG=info
EOF

echo "✓ Created docker-compose.override.yml with VECTOR_DIMENSION=384"
echo ""

# Step 4: Stop and remove current containers
echo "Restarting Vector DB with new configuration..."
docker-compose down

# Step 5: Clear the volume to reset the index
echo "Clearing old vector index..."
docker volume rm fabstir-vectordb_vector-data 2>/dev/null || true

# Step 6: Start with new configuration
echo "Starting Vector DB with 384D configuration..."
docker-compose up -d

echo "Waiting for Vector DB to initialize (20 seconds)..."
sleep 20

# Step 7: Verify the change
echo ""
echo "Verifying configuration..."
echo "----------------------------"

# Check health
HEALTH=$(curl -s http://localhost:7530/api/v1/health 2>/dev/null)
if echo "$HEALTH" | grep -q "healthy"; then
    echo "✓ Vector DB is healthy"
    echo "$HEALTH" | python3 -m json.tool 2>/dev/null || echo "$HEALTH"
else
    echo "✗ Vector DB not responding yet"
    echo "  Waiting 10 more seconds..."
    sleep 10
    HEALTH=$(curl -s http://localhost:7530/api/v1/health 2>/dev/null)
    echo "$HEALTH" | python3 -m json.tool 2>/dev/null || echo "$HEALTH"
fi
echo ""

# Test 384D vector
echo "Testing 384-dimensional vector..."
VECTOR_384=$(python3 -c "print(','.join(['0.1'] * 384))")
TEST_ID="test-384d-$(date +%s)"

RESPONSE=$(curl -s -X POST http://localhost:7530/api/v1/vectors \
    -H "Content-Type: application/json" \
    -d "{
        \"id\": \"$TEST_ID\",
        \"vector\": [$VECTOR_384],
        \"metadata\": {\"test\": \"384d-verification\"}
    }" 2>/dev/null)

if echo "$RESPONSE" | grep -q "error"; then
    echo "✗ 384D vectors still not accepted"
    echo "  Error: $RESPONSE"
    echo ""
    echo "  Checking docker environment..."
    docker exec fabstir-ai-vector-db-container env | grep VECTOR_DIMENSION
else
    echo "✅ SUCCESS! 384D vectors now accepted"
    echo "  Vector ID: $TEST_ID"
    
    # Clean up test vector
    curl -s -X DELETE http://localhost:7530/api/v1/vectors/$TEST_ID >/dev/null 2>&1
fi

echo ""
echo "========================================="
echo "Update Complete"
echo "========================================="
echo ""
echo "Vector DB should now accept 384-dimensional vectors."
echo "Return to working directory with:"
echo "  cd /root/dev/Fabstir/fabstir-llm-marketplace/fabstir-llm-node"
echo ""
echo "Then run:"
echo "  ./verify_phase_4_2_1_384d.sh"