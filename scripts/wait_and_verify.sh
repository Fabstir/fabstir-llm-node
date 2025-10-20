#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1


# wait_and_verify.sh
# Wait for Vector DB to start and verify it's working

echo "========================================="
echo "Waiting for Vector DB to Start"
echo "========================================="
echo ""

# Maximum wait time (60 seconds)
MAX_WAIT=60
WAITED=0

echo "Waiting for Vector DB to be ready..."
while [ $WAITED -lt $MAX_WAIT ]; do
    # Try health check
    if curl -s http://localhost:7530/api/v1/health 2>/dev/null | grep -q "healthy"; then
        echo "✅ Vector DB is ready!"
        echo ""
        
        # Show health status
        echo "Health Status:"
        curl -s http://localhost:7530/api/v1/health | python3 -m json.tool
        echo ""
        
        # Test vector dimensions
        echo "Testing supported dimensions:"
        
        # Test 3D (should fail now)
        echo -n "  3D vectors: "
        RESPONSE=$(curl -s -X POST http://localhost:7530/api/v1/vectors \
            -H "Content-Type: application/json" \
            -d '{"id": "test-3d", "vector": [0.1, 0.2, 0.3], "metadata": {}}' 2>/dev/null)
        if echo "$RESPONSE" | grep -q "error"; then
            echo "❌ Rejected (expected)"
        else
            echo "✅ Accepted"
            curl -s -X DELETE http://localhost:7530/api/v1/vectors/test-3d >/dev/null 2>&1
        fi
        
        # Test 384D (should work)
        echo -n "  384D vectors: "
        VECTOR_384=$(python3 -c "print(','.join(['0.1'] * 384))")
        RESPONSE=$(curl -s -X POST http://localhost:7530/api/v1/vectors \
            -H "Content-Type: application/json" \
            -d "{\"id\": \"test-384d\", \"vector\": [$VECTOR_384], \"metadata\": {}}" 2>/dev/null)
        if echo "$RESPONSE" | grep -q "error"; then
            echo "❌ Rejected"
            echo "    Error: $(echo $RESPONSE | grep -o '"error":"[^"]*"')"
        else
            echo "✅ Accepted (correct!)"
            curl -s -X DELETE http://localhost:7530/api/v1/vectors/test-384d >/dev/null 2>&1
        fi
        
        echo ""
        echo "========================================="
        echo "Vector DB is operational!"
        echo "========================================="
        echo ""
        echo "Next steps:"
        echo "1. Run: ./verify_phase_4_2_1_384d.sh"
        echo "2. Or run: ./test_dimensions.sh"
        
        exit 0
    fi
    
    # Show progress
    echo -n "."
    sleep 2
    WAITED=$((WAITED + 2))
done

echo ""
echo "❌ Vector DB failed to start after ${MAX_WAIT} seconds"
echo ""
echo "Check logs with:"
echo "  docker logs fabstir-ai-vector-db-container --tail 100"
echo ""
echo "Or check docker-compose logs:"
echo "  cd /root/dev/Fabstir/fabstir-vectordb"
echo "  docker-compose logs --tail 100 fabstir-ai-vector-db"

exit 1