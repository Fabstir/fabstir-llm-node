#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1


# check_vector_config.sh
# Check Vector DB configuration including dimension settings

echo "========================================="
echo "Vector DB Configuration Check"
echo "========================================="
echo ""

# Get health status
HEALTH=$(curl -s http://localhost:7530/api/v1/health)
echo "Health Response:"
echo "$HEALTH" | python3 -m json.tool 2>/dev/null || echo "$HEALTH"
echo ""

# Test different vector dimensions to find what's configured
echo "Testing vector dimensions..."
echo ""

for DIM in 3 128 384 768 1536; do
    echo -n "Testing $DIM dimensions: "
    
    # Create a vector of the specified dimension
    VECTOR=$(python3 -c "print(','.join(['0.1'] * $DIM))")
    
    # Try to insert
    RESPONSE=$(curl -s -X POST http://localhost:7530/api/v1/vectors \
        -H "Content-Type: application/json" \
        -d "{
            \"id\": \"dim-test-$DIM\",
            \"vector\": [$VECTOR],
            \"metadata\": {\"test_dim\": $DIM}
        }")
    
    if echo "$RESPONSE" | grep -q "error"; then
        echo "❌ Failed"
    else
        echo "✅ Success - Vector DB accepts $DIM-dimensional vectors"
        # Clean up successful test
        curl -s -X DELETE http://localhost:7530/api/v1/vectors/dim-test-$DIM > /dev/null
        break
    fi
done
echo ""

# Check existing vectors
echo "Checking existing vectors in Enhanced S5.js..."
VECTORS=$(curl -s http://localhost:5524/s5/fs/vectors/)
if echo "$VECTORS" | grep -q "entries"; then
    COUNT=$(echo "$VECTORS" | grep -o '"name"' | wc -l)
    echo "Found $COUNT vectors stored in Enhanced S5.js"
    
    if [ "$COUNT" -gt 0 ]; then
        echo ""
        echo "First few vector IDs:"
        echo "$VECTORS" | grep -o '"name":"[^"]*"' | head -5 | cut -d'"' -f4
    fi
else
    echo "No vectors found or unable to list"
fi
echo ""

# Docker configuration check
echo "Checking Docker environment variables..."
docker exec fabstir-ai-vector-db-container env | grep -E "VECTOR_DIMENSION|S5_|HAMT" | sort
echo ""

echo "========================================="
echo "Summary"
echo "========================================="
if docker exec fabstir-ai-vector-db-container env | grep -q "VECTOR_DIMENSION=3"; then
    echo "⚠️  Vector DB is configured for 3-dimensional vectors"
    echo "   This is likely a test configuration"
    echo ""
    echo "To use standard dimensions (e.g., 384 for all-MiniLM-L6-v2):"
    echo "  1. Stop the container"
    echo "  2. Restart with VECTOR_DIMENSION=384"
else
    DIM=$(docker exec fabstir-ai-vector-db-container env | grep VECTOR_DIMENSION | cut -d'=' -f2)
    echo "Vector DB is configured for $DIM-dimensional vectors"
fi