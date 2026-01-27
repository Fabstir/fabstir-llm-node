# IMPLEMENTATION - Stake Slashing Monitoring

## Status: Phase 1 - IN PROGRESS

**Status**: Phase 1 - Event Types - **IN PROGRESS**
**Version**: v8.13.0-slash-monitoring (pending)
**Start Date**: 2026-01-16
**Approach**: Strict TDD bounded autonomy - one sub-phase at a time
**Tests Passing**: 0 (starting fresh)

**Contract Update (2026-01-16):**
- NodeRegistry implementation upgraded: `0xF2D98D38B2dF95f4e8e4A49750823C415E795377`
- Proxy unchanged: `0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22`
- New capability: Stake slashing for misbehavior (invalid proofs, overclaiming)

**Priority**: Important for host awareness - Enables monitoring of stake risk

---

## Overview

Implementation plan for stake slashing monitoring in fabstir-llm-node. The node monitors `SlashExecuted` and `HostAutoUnregistered` events from NodeRegistry contract, queries `lastSlashTime`, and exposes status via `/v1/slash-status` endpoint.

**Key Features:**
1. Event monitoring - Poll for SlashExecuted and HostAutoUnregistered events
2. Health integration - Query lastSlashTime, mark health as "degraded" if slashed
3. API endpoint - `/v1/slash-status` returns comprehensive slash status

**Contract Constants:**
- `MAX_SLASH_PERCENTAGE = 50%` - Max 50% stake slashed per incident
- `MIN_STAKE_AFTER_SLASH = 100 FAB` - Auto-unregister threshold
- `SLASH_COOLDOWN = 86400 seconds (24h)` - Cooldown between slashes

**References:**
- Contract ABI: `docs/compute-contracts-reference/client-abis/NodeRegistryWithModelsUpgradeable-CLIENT-ABI.json`
- API Reference: `docs/compute-contracts-reference/API_REFERENCE.md`
- Existing Registry Monitor: `src/contracts/registry_monitor.rs`
- Existing Event Types: `src/contracts/types.rs`

---

## Dependencies

### Already Available (No Changes Needed)
```toml
[dependencies]
ethers = { version = "2.0", features = [...] }   # Contract interaction, EthEvent macro
tokio = { version = "1", features = ["full"] }   # Async runtime
serde = { version = "1.0", features = ["derive"] }  # Serialization
serde_json = "1.0"                               # JSON
tracing = "0.1"                                  # Logging
anyhow = "1.0"                                   # Error handling
axum = "0.7"                                     # HTTP server
```

### Existing Infrastructure
- `NodeRegistry` abigen contract - Contract interaction
- `RegistryMonitor` - Pattern for event polling
- `ApiServer` with Axum Router - API endpoint pattern
- `Arc<RwLock<T>>` - Thread-safe state management

---

## Phase 1: Event Types (TDD)

### Sub-phase 1.1: Add SlashExecuted Event Type

**Goal**: Define SlashExecutedEvent struct with EthEvent derive

**Status**: NOT STARTED

#### Tasks
- [ ] Write test `test_slash_executed_event_signature` - Verify keccak256 signature
- [ ] Write test `test_slash_executed_event_parsing` - Parse from raw log
- [ ] Write test `test_slash_executed_indexed_fields` - Verify host and executor indexed
- [ ] Implement `SlashExecutedEvent` struct in `src/contracts/types.rs`
- [ ] Run tests: `cargo test slash_executed` - All pass

**Test File:** `tests/contracts/test_slash_events.rs`

```rust
// Test: Event signature matches contract
#[test]
fn test_slash_executed_event_signature() {
    // keccak256("SlashExecuted(address,uint256,uint256,string,string,address,uint256)")
    let expected = "0x..."; // compute expected hash
    // Verify SlashExecutedEvent::signature() matches
}

// Test: Parse event from raw log data
#[test]
fn test_slash_executed_event_parsing() {
    // Create mock log with topics and data
    // Parse into SlashExecutedEvent
    // Verify all fields populated correctly
}
```

**Implementation:** `src/contracts/types.rs`

```rust
#[derive(Debug, Clone, EthEvent)]
#[ethevent(
    name = "SlashExecuted",
    abi = "SlashExecuted(address indexed,uint256,uint256,string,string,address indexed,uint256)"
)]
pub struct SlashExecutedEvent {
    #[ethevent(indexed)]
    pub host: Address,
    pub amount: U256,
    pub remaining_stake: U256,
    pub evidence_cid: String,
    pub reason: String,
    #[ethevent(indexed)]
    pub executor: Address,
    pub timestamp: U256,
}
```

---

### Sub-phase 1.2: Add HostAutoUnregistered Event Type

**Goal**: Define HostAutoUnregisteredEvent struct with EthEvent derive

**Status**: NOT STARTED

#### Tasks
- [ ] Write test `test_host_auto_unregistered_event_signature` - Verify signature
- [ ] Write test `test_host_auto_unregistered_event_parsing` - Parse from raw log
- [ ] Implement `HostAutoUnregisteredEvent` struct in `src/contracts/types.rs`
- [ ] Run tests: `cargo test host_auto_unregistered` - All pass

**Implementation:** `src/contracts/types.rs`

```rust
#[derive(Debug, Clone, EthEvent)]
#[ethevent(
    name = "HostAutoUnregistered",
    abi = "HostAutoUnregistered(address indexed,uint256,uint256,string)"
)]
pub struct HostAutoUnregisteredEvent {
    #[ethevent(indexed)]
    pub host: Address,
    pub slashed_amount: U256,
    pub returned_amount: U256,
    pub reason: String,
}
```

---

### Sub-phase 1.3: Add lastSlashTime to NodeRegistry abigen

**Goal**: Add lastSlashTime view function to NodeRegistry contract binding

**Status**: NOT STARTED

#### Tasks
- [ ] Write test `test_last_slash_time_function_exists` - Verify function in ABI
- [ ] Add `lastSlashTime` function to `abigen!` macro in `src/contracts/types.rs`
- [ ] Run tests: `cargo test last_slash_time` - All pass

**Implementation:** `src/contracts/types.rs` (add to existing abigen!)

```rust
abigen!(
    NodeRegistry,
    r#"[
        // ... existing functions ...
        {
            "inputs": [{"internalType": "address", "name": "host", "type": "address"}],
            "name": "lastSlashTime",
            "outputs": [{"internalType": "uint256", "name": "", "type": "uint256"}],
            "stateMutability": "view",
            "type": "function"
        }
    ]"#
);
```

---

## Phase 2: SlashMonitor Implementation (TDD)

### Sub-phase 2.1: Create SlashMonitor Module Structure

**Goal**: Create slash_monitor module with data structures

**Status**: NOT STARTED

#### Tasks
- [ ] Create `src/contracts/slash_monitor.rs` with module structure
- [ ] Define `SlashRecord` struct for storing slash events
- [ ] Define `AutoUnregisterRecord` struct
- [ ] Define `SlashHistory` struct with cache fields
- [ ] Define `SlashMonitor` struct with contract and cache
- [ ] Add `pub mod slash_monitor;` to `src/contracts/mod.rs`
- [ ] Run `cargo check` - Compiles without errors

**Implementation:** `src/contracts/slash_monitor.rs`

```rust
use ethers::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashRecord {
    pub amount: U256,
    pub remaining_stake: U256,
    pub evidence_cid: String,
    pub reason: String,
    pub executor: Address,
    pub timestamp: u64,
    pub block_number: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoUnregisterRecord {
    pub slashed_amount: U256,
    pub returned_amount: U256,
    pub reason: String,
    pub detected_at: u64,
    pub block_number: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SlashHistory {
    pub slash_events: Vec<SlashRecord>,
    pub auto_unregister_events: Vec<AutoUnregisterRecord>,
    pub last_slash_time: Option<u64>,
    pub total_slashed: U256,
    pub is_auto_unregistered: bool,
}

pub struct SlashMonitor {
    contract: NodeRegistry<Provider<Http>>,
    host_address: Address,
    chain_id: u64,
    cache: Arc<RwLock<SlashHistory>>,
    monitoring_handle: Option<JoinHandle<()>>,
}
```

---

### Sub-phase 2.2: Implement SlashMonitor Constructor and Getters

**Goal**: Implement new() and getter methods

**Status**: NOT STARTED

#### Tasks
- [ ] Write test `test_slash_monitor_new_empty_cache` - Cache starts empty
- [ ] Write test `test_slash_monitor_get_history_returns_cache` - Returns cache
- [ ] Write test `test_slash_monitor_is_slashed_false_initially` - Not slashed initially
- [ ] Implement `SlashMonitor::new()` constructor
- [ ] Implement `get_slash_history()` method
- [ ] Implement `is_host_slashed()` method
- [ ] Implement `is_host_auto_unregistered()` method
- [ ] Run tests: `cargo test slash_monitor` - All pass

---

### Sub-phase 2.3: Implement lastSlashTime Query

**Goal**: Query contract for last slash timestamp

**Status**: NOT STARTED

#### Tasks
- [ ] Write test `test_query_last_slash_time_returns_zero_never_slashed` - Returns 0
- [ ] Write test `test_query_last_slash_time_returns_timestamp` - Returns timestamp
- [ ] Implement `query_last_slash_time()` async method
- [ ] Run tests: `cargo test last_slash_time` - All pass

---

### Sub-phase 2.4: Implement Event Handlers

**Goal**: Implement handlers for slash and auto-unregister events

**Status**: NOT STARTED

#### Tasks
- [ ] Write test `test_handle_slash_event_updates_cache` - Cache updated
- [ ] Write test `test_handle_slash_event_accumulates_total` - Total increases
- [ ] Write test `test_handle_auto_unregister_sets_flag` - Flag set true
- [ ] Write test `test_handle_slash_event_logs_warning` - Log emitted
- [ ] Implement `handle_slash_event()` async method
- [ ] Implement `handle_auto_unregister_event()` async method
- [ ] Run tests: `cargo test handle_` - All pass

**Logging:**
```rust
warn!("🚨 HOST SLASHED: {} FAB for reason: {}", amount, reason);
error!("🚨🚨 HOST AUTO-UNREGISTERED: Stake fell below {} FAB minimum", MIN_STAKE);
```

---

### Sub-phase 2.5: Implement Event Polling Loop

**Goal**: Implement start_monitoring() with 5-second polling

**Status**: NOT STARTED

#### Tasks
- [ ] Write test `test_start_monitoring_spawns_task` - Task spawned
- [ ] Write test `test_monitoring_queries_events` - Events queried
- [ ] Write test `test_monitoring_updates_current_block` - Block advances
- [ ] Implement `start_monitoring()` async method
- [ ] Implement `stop_monitoring()` method
- [ ] Run tests: `cargo test monitoring` - All pass

**Implementation Pattern (from RegistryMonitor):**
```rust
pub async fn start_monitoring(&mut self, from_block: Option<u64>) -> Result<()> {
    let contract = self.contract.clone();
    let cache = self.cache.clone();
    let host = self.host_address;

    let handle = tokio::spawn(async move {
        let mut current_block = from_block.unwrap_or(0);

        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;

            let latest = contract.client().get_block_number().await?;
            if current_block >= latest.as_u64() { continue; }

            // Query SlashExecuted events filtered by host
            let slash_events = contract
                .event::<SlashExecutedEvent>()
                .from_block(current_block)
                .to_block(latest)
                .topic1(host)  // Filter by indexed host
                .query()
                .await?;

            for event in slash_events {
                Self::handle_slash_event(&cache, event).await;
            }

            // Query HostAutoUnregistered events filtered by host
            // ... similar pattern ...

            current_block = latest.as_u64() + 1;
        }
    });

    self.monitoring_handle = Some(handle);
    Ok(())
}
```

---

## Phase 3: API Endpoint (TDD)

### Sub-phase 3.1: Add SlashStatusResponse Types

**Goal**: Define API response structs

**Status**: NOT STARTED

#### Tasks
- [ ] Write test `test_slash_status_response_serialization` - JSON format correct
- [ ] Write test `test_slash_event_summary_serialization` - Summary format
- [ ] Write test `test_cooldown_calculation` - Cooldown = last_slash + 86400
- [ ] Implement `SlashStatusResponse` in `src/api/handlers.rs`
- [ ] Implement `SlashEventSummary` in `src/api/handlers.rs`
- [ ] Run tests: `cargo test slash_status_response` - All pass

**Implementation:** `src/api/handlers.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashStatusResponse {
    pub host: String,
    pub chain_id: u64,
    pub is_slashed: bool,
    pub is_auto_unregistered: bool,
    pub last_slash_time: Option<u64>,
    pub total_slashed: String,
    pub remaining_stake: Option<String>,
    pub slash_count: usize,
    pub slash_history: Vec<SlashEventSummary>,
    pub cooldown_ends_at: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashEventSummary {
    pub amount: String,
    pub reason: String,
    pub evidence_cid: String,
    pub timestamp: u64,
}
```

---

### Sub-phase 3.2: Add SlashMonitor to ApiServer

**Goal**: Wire SlashMonitor into ApiServer

**Status**: NOT STARTED

#### Tasks
- [ ] Add `slash_monitor: Arc<RwLock<Option<SlashMonitor>>>` to `ApiServer` struct
- [ ] Update `ApiServer::new()` to accept slash_monitor parameter
- [ ] Update `ApiServer::new()` calls in codebase (likely main.rs)
- [ ] Run `cargo check` - Compiles without errors

---

### Sub-phase 3.3: Implement /v1/slash-status Endpoint

**Goal**: Add route and handler for slash status

**Status**: NOT STARTED

#### Tasks
- [ ] Write test `test_slash_status_endpoint_returns_200` - Returns OK
- [ ] Write test `test_slash_status_never_slashed_response` - Clean host
- [ ] Write test `test_slash_status_with_history_response` - Has history
- [ ] Write test `test_slash_status_auto_unregistered_response` - Flag true
- [ ] Add route `.route("/v1/slash-status", get(slash_status_handler))` to router
- [ ] Implement `slash_status_handler()` function
- [ ] Implement `ApiServer::get_slash_status()` method
- [ ] Run tests: `cargo test slash_status` - All pass

**Implementation:** `src/api/server.rs`

```rust
async fn slash_status_handler(
    State(server): State<Arc<ApiServer>>,
) -> impl IntoResponse {
    match server.get_slash_status().await {
        Ok(status) => (StatusCode::OK, Json(status)).into_response(),
        Err(e) => {
            error!("Failed to get slash status: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({
                "error": "Failed to retrieve slash status"
            }))).into_response()
        }
    }
}
```

---

## Phase 4: Integration (TDD)

### Sub-phase 4.1: Initialize SlashMonitor in main.rs

**Goal**: Create and start SlashMonitor during node startup

**Status**: NOT STARTED

#### Tasks
- [ ] Add SlashMonitor initialization in `main.rs` (requires HOST_PRIVATE_KEY)
- [ ] Start monitoring after node registration
- [ ] Pass SlashMonitor to ApiServer
- [ ] Run `cargo build --release` - Build succeeds
- [ ] Manual test: Start node, verify `/v1/slash-status` returns response

---

### Sub-phase 4.2: Health Check Integration

**Goal**: Include slash status in health endpoint

**Status**: NOT STARTED

#### Tasks
- [ ] Write test `test_health_includes_slash_warning` - Warning in issues
- [ ] Write test `test_health_degraded_when_recently_slashed` - Status degraded
- [ ] Modify `health_check()` to query slash status
- [ ] Add warning if slashed within 24 hours
- [ ] Run tests: `cargo test health` - All pass

---

### Sub-phase 4.3: Version Update

**Goal**: Update version to v8.13.0-slash-monitoring

**Status**: NOT STARTED

#### Tasks
- [ ] Update `/workspace/VERSION` to `8.13.0-slash-monitoring`
- [ ] Update `/workspace/src/version.rs` version constants
- [ ] Add `slash-monitoring` to features list
- [ ] Run version tests: `cargo test version` - All pass
- [ ] Build release: `cargo build --release --features real-ezkl -j 4`

---

## Verification Checklist

### Unit Tests
```bash
cargo test --test test_slash_events -- --nocapture
cargo test --test test_slash_monitor -- --nocapture
cargo test --test test_slash_endpoint -- --nocapture
```

### Integration Test
```bash
# Start node
cargo run --release

# Check slash status (should show clean host)
curl http://localhost:8080/v1/slash-status | jq

# Expected response:
{
  "host": "0x...",
  "chain_id": 84532,
  "is_slashed": false,
  "is_auto_unregistered": false,
  "last_slash_time": null,
  "total_slashed": "0",
  "slash_count": 0,
  "slash_history": [],
  "cooldown_ends_at": null
}

# Check health endpoint
curl http://localhost:8080/health | jq
```

### Log Verification
When a slash occurs (testnet simulation), verify logs show:
- `🚨 HOST SLASHED: X FAB for reason: Y`
- `🚨🚨 HOST AUTO-UNREGISTERED: Stake fell below minimum` (if applicable)

---

## Contract Reference

**Proxy Address:** `0x8BC0Af4aAa2dfb99699B1A24bA85E507de10Fd22`
**Implementation:** `0xF2D98D38B2dF95f4e8e4A49750823C415E795377`

**Constants:**
- `MAX_SLASH_PERCENTAGE = 50%`
- `MIN_STAKE_AFTER_SLASH = 100 FAB`
- `SLASH_COOLDOWN = 86400 seconds (24h)`

**View Function:**
```solidity
function lastSlashTime(address host) external view returns (uint256)
```

**Events:**
```solidity
event SlashExecuted(
    address indexed host,
    uint256 amount,
    uint256 remainingStake,
    string evidenceCID,
    string reason,
    address indexed executor,
    uint256 timestamp
)

event HostAutoUnregistered(
    address indexed host,
    uint256 slashedAmount,
    uint256 returnedAmount,
    string reason
)
```
