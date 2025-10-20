#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1


# fix_vector_db.sh
# Diagnose and fix Vector DB issues after reset

echo "========================================="
echo "Vector DB Diagnostic & Fix"
echo "========================================="
echo ""

# Step 1: Check container status
echo "1. Checking container status..."
if docker ps | grep -q fabstir-ai-vector-db-container; then
    echo "✓ Container is running"
    CONTAINER_ID=$(docker ps | grep fabstir-ai-vector-db-container | awk '{print $1}')
    echo "  Container ID: $CONTAINER_ID"
else
    echo "✗ Container is not running"
    echo ""
    echo "Checking if container exists but stopped..."
    if docker ps -a | grep -q fabstir-ai-vector-db-container; then
        echo "Container exists but stopped. Starting it..."
        docker start fabstir-ai-vector-db-container
        sleep 5
    else
        echo "Container doesn't exist. Need to recreate..."
        
        # Recreate with proper settings
        echo "Creating new container with 384D configuration..."
        docker run -d \
            --name fabstir-ai-vector-db-container \
            --network host \
            -e S5_MODE=real \
            -e S5_PORTAL_URL=http://localhost:5524 \
            -e S5_MOCK_SERVER_URL=http://localhost:5524 \
            -e VECTOR_DIMENSION=384 \
            -e HAMT_ACTIVATION_THRESHOLD=1000 \
            -e RUST_LOG=info \
            fabstir-vectordb-fabstir-ai-vector-db:latest
        
        echo "Waiting for startup..."
        sleep 15
    fi
fi
echo ""

# Step 2: Check logs for errors
echo "2. Checking recent logs..."
echo "----------------------------"
docker logs fabstir-ai-vector-db-container --tail 10 2>&1 | head -20
echo "----------------------------"
echo ""

# Step 3: Test connectivity
echo "3. Testing API connectivity..."
for i in {1..5}; do
    echo -n "  Attempt $i: "
    RESPONSE=$(curl -s -w "\n%{http_code}" http://localhost:7530/api/v1/health 2>/dev/null)
    HTTP_CODE=$(echo "$RESPONSE" | tail -1)
    
    if [ "$HTTP_CODE" = "200" ]; then
        echo "✓ Success! API is responding"
        echo "  Health response:"
        echo "$RESPONSE" | head -n -1 | python3 -m json.tool 2>/dev/null || echo "$RESPONSE" | head -n -1
        break
    elif [ "$HTTP_CODE" = "000" ]; then
        echo "✗ Connection refused (service may be starting)"
    else
        echo "✗ HTTP $HTTP_CODE"
    fi
    
    if [ $i -lt 5 ]; then
        echo "  Waiting 5 seconds before retry..."
        sleep 5
    fi
done
echo ""

# Step 4: Check port binding
echo "4. Checking port bindings..."
if netstat -tuln 2>/dev/null | grep -q ":7530"; then
    echo "✓ Port 7530 is listening"
else
    echo "✗ Port 7530 is not listening"
    echo "  Checking what's using the port..."
    lsof -i :7530 2>/dev/null || echo "  No process found on port 7530"
fi
echo ""

# Step 5: Test with a simple vector operation
echo "5. Testing vector operations..."
TEST_RESPONSE=$(curl -s -X POST http://localhost:7530/api/v1/vectors \
    -H "Content-Type: application/json" \
    -d '{
        "id": "test-fix-'$(date +%s)'",
        "vector": ['$(python3 -c "print(','.join(['0.1'] * 384))")'],
        "metadata": {"test": "diagnostic"}
    }' 2>/dev/null)

if echo "$TEST_RESPONSE" | grep -q "error"; then
    echo "✗ Vector insertion failed"
    echo "  Error: $TEST_RESPONSE"
else
    echo "✓ Vector operations working"
fi
echo ""

# Step 6: Summary and recommendations
echo "========================================="
echo "Diagnostic Summary"
echo "========================================="

# Check final status
if curl -s http://localhost:7530/api/v1/health | grep -q "healthy"; then
    echo "✅ Vector DB is now operational!"
    echo ""
    echo "Next steps:"
    echo "1. Run: ./verify_phase_4_2_1_384d.sh"
    echo "2. Or run: ./test_dimensions.sh"
else
    echo "⚠️ Vector DB still having issues"
    echo ""
    echo "Try these fixes:"
    echo "1. Restart with docker-compose:"
    echo "   cd /root/dev/Fabstir/fabstir-vectordb"
    echo "   docker-compose down"
    echo "   docker-compose up -d"
    echo ""
    echo "2. Or check for port conflicts:"
    echo "   lsof -i :7530"
    echo ""
    echo "3. Check full logs:"
    echo "   docker logs fabstir-ai-vector-db-container"
fi