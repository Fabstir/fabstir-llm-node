#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1


# Script to start real S5 services with corrected Docker image names
echo "=== Starting Real S5 Backend Integration ==="

# Stop any existing containers
echo "Stopping any existing containers..."
docker-compose -f docker-compose.real.yml down 2>/dev/null || true

# Start services
echo "Starting services with corrected image names..."
docker-compose -f docker-compose.real.yml up -d

# Wait for services to initialize
echo "Waiting for services to initialize..."
sleep 5

# Check running containers
echo -e "\n=== Running Containers ==="
docker ps --format "table {{.Names}}\t{{.Status}}\t{{.Ports}}" | grep -E "real|NAMES"

# Check Enhanced S5 logs
echo -e "\n=== Enhanced S5 Real Logs ==="
docker logs enhanced-s5-real --tail 20 2>&1

# Check Vector DB logs
echo -e "\n=== Vector DB Real Logs ==="
docker logs vector-db-real --tail 20 2>&1

# Test connectivity
echo -e "\n=== Testing Connectivity ==="

# Test Vector DB health
echo "Testing Vector DB health endpoint..."
curl -s http://localhost:8081/api/v1/health | jq '.' 2>/dev/null || echo "Vector DB not responding"

# Test Enhanced S5 health
echo -e "\nTesting Enhanced S5 endpoint..."
curl -s http://localhost:5525/api/v1/health | jq '.' 2>/dev/null || echo "Enhanced S5 not responding"

# Test real S5 portal connectivity
echo -e "\nTesting real S5 portal (https://s5.vup.cx)..."
curl -s https://s5.vup.cx/s5/health | jq '.' 2>/dev/null || echo "Cannot reach S5 portal"

echo -e "\n=== Service Status Summary ==="
echo "Vector DB Real: http://localhost:8081"
echo "Enhanced S5 Real: http://localhost:5525"
echo "S5 Portal: https://s5.vup.cx"
