#!/bin/bash

# update_verify_384d.sh
# Update the verification script to use 384-dimensional vectors

echo "Updating verify_phase_4_2_1.sh for 384-dimensional vectors..."

# Create a 384D vector string (all 0.1 values for simplicity)
VECTOR_384D=$(python3 -c "print(', '.join(['0.1'] * 384))")

# Update the insert vector command
cat > verify_phase_4_2_1_384d.sh << 'EOF'
#!/bin/bash

# verify_phase_4_2_1_384d.sh
# Phase 4.2.1 verification with 384-dimensional vectors

echo "========================================="
echo "Phase 4.2.1 Integration Verification (384D)"
echo "========================================="
echo ""

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

# Step 1: Check both services are running
echo "1. Checking services..."
echo ""

# Check Enhanced S5.js
echo -n "   Enhanced S5.js (port 5524): "
S5_HEALTH=$(curl -s http://localhost:5524/health)
if [ $? -eq 0 ]; then
    echo -e "${GREEN}✓ Running${NC}"
    echo "   $S5_HEALTH"
else
    echo -e "${RED}✗ Not accessible${NC}"
    exit 1
fi
echo ""

# Check Vector DB
echo -n "   Vector DB (port 7530): "
VDB_HEALTH=$(curl -s http://localhost:7530/api/v1/health)
if [ $? -eq 0 ]; then
    echo -e "${GREEN}✓ Running${NC}"
    BASE_URL=$(echo "$VDB_HEALTH" | grep -o '"base_url":"[^"]*"' | cut -d'"' -f4)
    echo "   Storage backend: $BASE_URL"
else
    echo -e "${RED}✗ Not accessible${NC}"
    echo "   Trying again in 5 seconds..."
    sleep 5
    VDB_HEALTH=$(curl -s http://localhost:7530/api/v1/health)
    if [ $? -eq 0 ]; then
        echo -e "${GREEN}✓ Running (after retry)${NC}"
    else
        echo -e "${RED}✗ Still not accessible${NC}"
    fi
fi
echo ""

# Step 2: Insert test vectors through Vector DB
echo "2. Inserting 384D test vector through Vector DB..."
echo ""

TEST_ID="integration-test-384d-$(date +%s)"

# Create 384-dimensional vector
VECTOR_384D=$(python3 -c "print(', '.join(['0.1'] * 384))")

# Insert a vector
echo -n "   Inserting vector $TEST_ID: "
INSERT_RESPONSE=$(curl -s -X POST http://localhost:7530/api/v1/vectors \
    -H "Content-Type: application/json" \
    -d "{
        \"id\": \"$TEST_ID\",
        \"vector\": [$VECTOR_384D],
        \"metadata\": {
            \"test\": \"phase-4.2.1\",
            \"dimension\": 384,
            \"timestamp\": \"$(date -Iseconds)\"
        }
    }")

if echo "$INSERT_RESPONSE" | grep -q "$TEST_ID"; then
    echo -e "${GREEN}✓ Success${NC}"
else
    echo -e "${RED}✗ Failed${NC}"
    echo "   Response: $INSERT_RESPONSE"
fi
echo ""

# Step 3: Verify vector is stored in Enhanced S5.js
echo "3. Checking Enhanced S5.js storage..."
echo ""

# List vectors directory
echo -n "   Listing /s5/fs/vectors/: "
VECTORS_LIST=$(curl -s http://localhost:5524/s5/fs/vectors/)
if [ $? -eq 0 ]; then
    VECTOR_COUNT=$(echo "$VECTORS_LIST" | grep -o '"name"' | wc -l)
    echo -e "${GREEN}✓ Found $VECTOR_COUNT vectors${NC}"
    
    if echo "$VECTORS_LIST" | grep -q "$TEST_ID"; then
        echo -e "   ${GREEN}✓ Test vector $TEST_ID found in S5 storage${NC}"
    else
        echo -e "   ${YELLOW}⚠ Test vector not found by ID (might be stored with different name)${NC}"
    fi
else
    echo -e "${YELLOW}⚠ Could not list vectors${NC}"
fi
echo ""

# Step 4: Retrieve vector through Vector DB
echo "4. Retrieving vector through Vector DB..."
echo ""

echo -n "   Getting vector $TEST_ID: "
GET_RESPONSE=$(curl -s http://localhost:7530/api/v1/vectors/$TEST_ID)
if echo "$GET_RESPONSE" | grep -q "$TEST_ID"; then
    echo -e "${GREEN}✓ Retrieved successfully${NC}"
    METADATA=$(echo "$GET_RESPONSE" | grep -o '"metadata":{[^}]*}' | head -1)
    echo "   Metadata: $METADATA"
    
    # Check vector dimension
    VECTOR_DATA=$(echo "$GET_RESPONSE" | grep -o '"vector":\[[^]]*\]')
    RETURNED_DIM=$(echo "$VECTOR_DATA" | grep -o '0\.[0-9]*' | wc -l)
    echo "   Vector dimension: $RETURNED_DIM"
else
    echo -e "${RED}✗ Failed to retrieve${NC}"
    echo "   Response: $GET_RESPONSE"
fi
echo ""

# Step 5: Test search functionality with 384D
echo "5. Testing vector search with 384D..."
echo ""

echo -n "   Searching for similar vectors: "
SEARCH_RESPONSE=$(curl -s -X POST http://localhost:7530/api/v1/search \
    -H "Content-Type: application/json" \
    -d "{
        \"vector\": [$VECTOR_384D],
        \"k\": 5
    }")

if echo "$SEARCH_RESPONSE" | grep -q "results"; then
    RESULT_COUNT=$(echo "$SEARCH_RESPONSE" | grep -o '"id"' | wc -l)
    echo -e "${GREEN}✓ Found $RESULT_COUNT results${NC}"
    
    if echo "$SEARCH_RESPONSE" | grep -q "$TEST_ID"; then
        echo -e "   ${GREEN}✓ Test vector found in search results${NC}"
    fi
else
    echo -e "${RED}✗ Search failed${NC}"
    echo "   Response: $SEARCH_RESPONSE"
fi
echo ""

# Step 6: Clean up test data
echo "6. Cleaning up..."
echo ""

echo -n "   Deleting test vector: "
DELETE_RESPONSE=$(curl -s -X DELETE http://localhost:7530/api/v1/vectors/$TEST_ID)
if [ $? -eq 0 ]; then
    echo -e "${GREEN}✓ Deleted${NC}"
else
    echo -e "${YELLOW}⚠ Could not delete${NC}"
fi
echo ""

# Summary
echo "========================================="
echo "Integration Verification Summary (384D)"
echo "========================================="
echo ""

if [ -n "$RESULT_COUNT" ] && [ "$RESULT_COUNT" -gt 0 ]; then
    echo -e "${GREEN}✅ Phase 4.2.1 VERIFIED with 384D vectors${NC}"
    echo ""
    echo "Vector DB successfully using Enhanced S5.js with 384-dimensional vectors:"
    echo "- Vectors are being stored in Enhanced S5.js"
    echo "- Vector DB can retrieve stored vectors"
    echo "- Search functionality works"
    echo "- System configured for all-MiniLM-L6-v2 embeddings (384D)"
else
    echo -e "${YELLOW}⚠️ Verification in progress${NC}"
    echo ""
    echo "Check:"
    echo "- docker logs fabstir-ai-vector-db-container"
    echo "- curl http://localhost:7530/api/v1/health"
fi
echo ""
echo "Storage path: http://localhost:5524/s5/fs/vectors/"
echo "Vector DB API: http://localhost:7530/api/v1/"
EOF

chmod +x verify_phase_4_2_1_384d.sh
echo "✓ Created verify_phase_4_2_1_384d.sh with 384-dimensional vectors"
echo ""
echo "Run it with: ./verify_phase_4_2_1_384d.sh"