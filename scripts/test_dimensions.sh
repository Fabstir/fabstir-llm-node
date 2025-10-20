#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1


# test_dimensions.sh
# Test what vector dimensions the Vector DB actually accepts

echo "========================================="
echo "Vector Dimension Testing"
echo "========================================="
echo ""

# Test various dimensions
for DIM in 3 384 768 1536; do
    echo "Testing $DIM-dimensional vector:"
    
    # Create vector of specified dimension
    VECTOR=""
    for ((i=1; i<=DIM; i++)); do
        if [ $i -eq 1 ]; then
            VECTOR="0.$i"
        else
            VECTOR="$VECTOR, 0.$i"
        fi
    done
    
    # Try to insert
    TEST_ID="dim-test-$DIM-$(date +%s)"
    echo -n "  Inserting: "
    INSERT_RESPONSE=$(curl -s -X POST http://localhost:7530/api/v1/vectors \
        -H "Content-Type: application/json" \
        -d "{
            \"id\": \"$TEST_ID\",
            \"vector\": [$VECTOR],
            \"metadata\": {\"dimension\": $DIM}
        }")
    
    if echo "$INSERT_RESPONSE" | grep -q "error"; then
        ERROR=$(echo "$INSERT_RESPONSE" | grep -o '"error":"[^"]*"' | cut -d'"' -f4)
        echo "❌ Failed - $ERROR"
    else
        echo "✅ Success"
        
        # Try to retrieve it
        echo -n "  Retrieving: "
        GET_RESPONSE=$(curl -s http://localhost:7530/api/v1/vectors/$TEST_ID)
        if echo "$GET_RESPONSE" | grep -q "$TEST_ID"; then
            echo "✅ Success"
            
            # Check the actual vector dimension returned
            VECTOR_DATA=$(echo "$GET_RESPONSE" | grep -o '"vector":\[[^]]*\]')
            RETURNED_DIM=$(echo "$VECTOR_DATA" | grep -o '[0-9]\.' | wc -l)
            echo "  Returned dimension: $RETURNED_DIM"
        else
            echo "❌ Failed to retrieve"
        fi
        
        # Clean up
        curl -s -X DELETE http://localhost:7530/api/v1/vectors/$TEST_ID > /dev/null
    fi
    echo ""
done

echo "========================================="
echo "Testing Complete"
echo "========================================="