# Deploy V5 with Payment Settlement Fix

## What's Fixed in V5
- Added `completeSessionJob()` call when WebSocket disconnects
- This triggers payment distribution to HostEarnings contract
- Payments now settle automatically when session ends

## Version
- v5-payment-settlement-2024-09-22-04:45

## Deployment Steps (Run in Host Terminal)

```bash
# 1. Copy binary from dev container to host
docker cp fabstir-llm-marketplace-node-dev-1:/workspace/target/release/fabstir-llm-node target/release/fabstir-llm-node

# 2. Deploy to production nodes
docker cp target/release/fabstir-llm-node llm-node-prod-1:/usr/local/bin/fabstir-llm-node
docker cp target/release/fabstir-llm-node llm-node-prod-2:/usr/local/bin/fabstir-llm-node

# 3. Restart nodes
docker restart llm-node-prod-1 llm-node-prod-2

# 4. Wait 10 seconds
sleep 10

# 5. Verify v5 is running
docker logs llm-node-prod-1 2>&1 | grep "v5-payment-settlement"
```

## Testing Payment Settlement

1. Start a new session in the UI
2. Generate 100+ tokens (triggers checkpoint)
3. End the session (close WebSocket or click "End Session")
4. Check logs for payment settlement:
   ```bash
   docker logs -f llm-node-prod-1 2>&1 | grep -E "💰|payment|complete|settlement"
   ```

## What to Look For in Logs

When session ends, you should see:
```
💰 WebSocket closing - triggering payment settlement for job XXX
💰 Completing session job XXX to trigger payment settlement...
Transaction sent for completing job XXX
✅ Session completed and payments distributed for job XXX
  - Host earnings (97.5%) sent to HostEarnings contract
  - Treasury fee (2.5%) collected
  - Unused deposit refunded to user
```

## Contract Details
- JobMarketplace: 0x1273E6358aa52Bb5B160c34Bf2e617B745e4A944
- HostEarnings: 0x908962e8c6CE72610021586f85ebDE09aAc97776

Host earnings should appear in the HostEarnings contract after session ends.