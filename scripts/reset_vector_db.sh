#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1


# reset_vector_db.sh
# Reset Vector DB to use 384-dimensional vectors (for all-MiniLM-L6-v2)

echo "========================================="
echo "Resetting Vector DB to 384 Dimensions"
echo "========================================="
echo ""

# Step 1: Stop current Vector DB
echo "1. Stopping current Vector DB container..."
docker stop fabstir-ai-vector-db-container
docker rm fabstir-ai-vector-db-container
echo "✓ Container stopped and removed"
echo ""

# Step 2: Clear any persistent volume data
echo "2. Clearing persistent data..."
docker volume rm fabstir-vectordb_vector-data 2>/dev/null || true
echo "✓ Volume cleared"
echo ""

# Step 3: Start fresh Vector DB with 384 dimensions
echo "3. Starting Vector DB with 384-dimensional vectors..."
docker run -d \
    --name fabstir-ai-vector-db-container \
    --network host \
    -e S5_MODE=real \
    -e S5_PORTAL_URL=http://localhost:5524 \
    -e S5_MOCK_SERVER_URL=http://localhost:5524 \
    -e VECTOR_DIMENSION=384 \
    -e HAMT_ACTIVATION_THRESHOLD=1000 \
    -e RUST_LOG=debug \
    -p 7530:7530 \
    -p 7531:7531 \
    -p 7532:7532 \
    fabstir-vectordb-fabstir-ai-vector-db:latest

echo "✓ Container started"
echo ""

# Step 4: Wait for startup
echo "4. Waiting for Vector DB to initialize..."
sleep 10

# Step 5: Check health
echo "5. Checking Vector DB health..."
HEALTH=$(curl -s http://localhost:7530/api/v1/health)
if echo "$HEALTH" | grep -q "healthy"; then
    echo "✓ Vector DB is healthy"
    echo "$HEALTH" | python3 -m json.tool 2>/dev/null || echo "$HEALTH"
else
    echo "✗ Vector DB not responding properly"
    echo "Check logs: docker logs fabstir-ai-vector-db-container"
fi
echo ""

# Step 6: Test 384-dimensional vector
echo "6. Testing 384-dimensional vector insertion..."
VECTOR=$(python3 -c "print(','.join(['0.1'] * 384))")
TEST_ID="test-384d-$(date +%s)"

RESPONSE=$(curl -s -X POST http://localhost:7530/api/v1/vectors \
    -H "Content-Type: application/json" \
    -d "{
        \"id\": \"$TEST_ID\",
        \"vector\": [$VECTOR],
        \"metadata\": {\"test\": \"dimension-check\"}
    }")

if echo "$RESPONSE" | grep -q "error"; then
    echo "✗ Failed to insert 384D vector"
    echo "Error: $RESPONSE"
else
    echo "✓ Successfully inserted 384-dimensional vector"
    echo "Vector ID: $TEST_ID"
fi
echo ""

echo "========================================="
echo "Reset Complete"
echo "========================================="
echo ""
echo "Vector DB is now configured for 384-dimensional vectors"
echo "This matches all-MiniLM-L6-v2 embedding model"
echo ""
echo "Next steps:"
echo "1. Re-run integration tests with 384D vectors"
echo "2. Update your code to use 384-dimensional embeddings"