# Multi-Chain/Multi-Wallet Implementation Plan

## Overview

This implementation plan adds multi-chain and multi-wallet support to the Fabstir LLM Node following strict TDD (Test-Driven Development) with bounded autonomy. Each sub-phase is self-contained with clear deliverables and test requirements.

## Core Requirements

- **Multi-Chain Support**: Base Sepolia (ETH) + opBNB Testnet (BNB) initially
- **Wallet Agnostic**: Support both EOA and Smart Contract wallets
- **Chain-Aware Settlement**: Automatic payment settlement on correct chain
- **Backwards Compatible**: Maintain existing single-chain functionality
- **Production Ready**: Handle chain-specific gas, RPC endpoints, and native tokens

## Phase 1: Chain Configuration Foundation

### Sub-phase 1.1: Chain Registry Infrastructure ✅
**Goal**: Create foundational chain configuration system

**Tasks**:
- [x] Create `src/config/chains.rs` module
- [x] Define `ChainConfig` struct with chain metadata
- [x] Define `TokenInfo` struct for native tokens
- [x] Define `ContractAddresses` struct for per-chain contracts
- [x] Implement `ChainRegistry` with Base Sepolia and opBNB configs
- [x] Add chain validation utilities

**Test Files** (TDD - Write First):
- `tests/chains/test_chain_config.rs`
  - test_base_sepolia_config()
  - test_opbnb_config()
  - test_chain_registry_initialization()
  - test_get_chain_by_id()
  - test_invalid_chain_id()
- `tests/chains/test_token_info.rs`
  - test_eth_token_info()
  - test_bnb_token_info()
  - test_token_decimals()

**Success Criteria**:
- All tests pass
- Chain configurations load correctly
- Registry returns correct config for each chain ID

### Sub-phase 1.2: Environment Configuration ✅
**Goal**: Load multi-chain settings from environment

**Tasks**:
- [x] Update `.env.local.test` with multi-chain variables
- [x] Create `ChainConfigLoader` for environment parsing
- [x] Implement fallback to hardcoded defaults
- [x] Add RPC URL validation
- [x] Support override contract addresses per chain

**Test Files** (TDD - Write First):
- `tests/chains/test_env_config.rs`
  - test_load_base_sepolia_from_env()
  - test_load_opbnb_from_env()
  - test_rpc_url_validation()
  - test_contract_override()
  - test_missing_env_fallback()

**Success Criteria**:
- Environment variables correctly parsed
- Fallbacks work when env vars missing
- Invalid configurations rejected

### Sub-phase 1.3: Multi-Provider Management ✅
**Goal**: Manage multiple blockchain providers

**Tasks**:
- [x] Create `MultiChainProvider` struct
- [x] Initialize providers for each chain
- [x] Implement provider health checks
- [x] Add RPC failover support
- [x] Create provider pooling for efficiency

**Test Files** (TDD - Write First):
- `tests/chains/test_multi_provider.rs`
  - test_provider_initialization()
  - test_get_provider_by_chain()
  - test_provider_health_check()
  - test_rpc_failover()
  - test_concurrent_providers()

**Success Criteria**:
- Providers initialize for all chains
- Health checks detect failures
- Failover switches to backup RPC

## Phase 2: Session Chain Tracking

### Sub-phase 2.1: Session Data Structure Updates ✅
**Goal**: Add chain tracking to session management

**Tasks**:
- [x] Update `WebSocketSession` struct with `chain_id` field
- [x] Migrate existing sessions (default to Base Sepolia)
- [x] Create `SessionChainInfo` for chain metadata
- [x] Update session serialization/deserialization
- [x] Add chain validation on session creation

**Test Files** (TDD - Write First):
- `tests/sessions/test_session_chain.rs`
  - test_session_with_chain_id()
  - test_session_chain_validation()
  - test_legacy_session_migration()
  - test_session_serialization()
  - test_invalid_chain_rejection()

**Success Criteria**:
- Sessions store chain ID
- Legacy sessions default correctly
- Invalid chains rejected

### Sub-phase 2.2: Session Manager Enhancement ✅
**Goal**: Make session manager chain-aware

**Tasks**:
- [x] Update `SessionManager::create_session()` with chain_id
- [x] Implement `get_session_chain()` method
- [x] Add `list_sessions_by_chain()` method
- [x] Create session chain statistics
- [x] Implement session chain migration

**Test Files** (TDD - Write First):
- `tests/sessions/test_session_manager_chain.rs`
  - test_create_session_with_chain()
  - test_get_session_chain()
  - test_list_sessions_by_chain()
  - test_session_chain_stats()
  - test_cross_chain_session_query()

**Success Criteria**:
- Session manager tracks chains correctly
- Can query sessions by chain
- Statistics accurate per chain

### Sub-phase 2.3: Session Persistence ✅
**Goal**: Persist chain information across restarts

**Tasks**:
- [x] Update session storage schema
- [x] Add chain_id to session cache
- [x] Implement chain-aware session recovery
- [x] Create session backup per chain
- [x] Add chain migration utilities

**Test Files** (TDD - Write First):
- `tests/sessions/test_session_persistence.rs`
  - test_save_session_with_chain()
  - test_load_session_with_chain()
  - test_session_recovery_after_restart()
  - test_chain_specific_backup()
  - test_migrate_session_chain()
  - test_list_sessions_by_chain()
  - test_delete_expired_sessions()
  - test_restore_from_backup()

**Success Criteria**:
- Sessions persist with chain info
- Recovery maintains chain association
- Backups organized by chain

## Phase 3: Multi-Chain Settlement

### Sub-phase 3.1: Settlement Manager Core ✅
**Goal**: Create chain-aware settlement system

**Tasks**:
- [x] Create `SettlementManager` struct
- [x] Store signers per chain
- [x] Implement chain-specific gas estimation
- [x] Add settlement transaction builders
- [x] Create settlement queue per chain

**Test Files** (TDD - Write First):
- `tests/settlement/test_settlement_manager.rs`
  - test_settlement_manager_init()
  - test_signer_per_chain()
  - test_gas_estimation_base()
  - test_gas_estimation_opbnb()
  - test_settlement_queue()
  - test_settlement_queue_retry()
  - test_gas_estimation_unknown_chain()
  - test_settlement_manager_health_check()

**Success Criteria**:
- Settlement manager initializes
- Correct signer used per chain
- Gas estimates reasonable

### Sub-phase 3.2: Automatic Settlement Integration ✅
**Goal**: Connect settlement to WebSocket disconnect

**Tasks**:
- [x] Update WebSocket disconnect handler
- [x] Implement `settle_session()` with chain lookup
- [x] Add settlement retry logic
- [x] Create settlement event logging
- [x] Handle settlement failures gracefully

**Test Files** (TDD - Write First):
- `tests/settlement/test_auto_settlement.rs`
  - test_settlement_on_disconnect()
  - test_settlement_correct_chain()
  - test_settlement_retry_logic()
  - test_settlement_failure_handling()
  - test_concurrent_settlements()
  - test_disconnect_handler_integration()
  - test_settlement_event_logging()

**Success Criteria**:
- Disconnect triggers settlement
- Correct chain used for settlement
- Retries work on failure

### Sub-phase 3.3: Payment Distribution ✅
**Goal**: Handle chain-specific payment flows

**Tasks**:
- [x] Implement host earnings accumulation per chain
- [x] Track treasury fees per chain
- [x] Handle different payment tokens
- [x] Create refund logic per chain
- [x] Add payment verification

**Test Files** (TDD - Write First):
- `tests/settlement/test_payment_distribution.rs`
  - test_host_earnings_base_sepolia()
  - test_host_earnings_opbnb()
  - test_treasury_accumulation()
  - test_user_refund_calculation()
  - test_payment_verification()
  - test_different_payment_tokens()
  - test_chain_payment_statistics()
  - test_multi_chain_payment_tracking()

**Success Criteria**:
- Payments distributed correctly
- Fees accumulated properly
- Refunds calculated accurately

## Phase 4: WebSocket Protocol Updates

### Sub-phase 4.1: Message Protocol Enhancement
**Goal**: Add chain awareness to WebSocket messages

**Tasks**:
- [ ] Update `SessionInitMessage` with `chain_id` field
- [ ] Add chain info to response messages
- [ ] Create chain validation in message handlers
- [ ] Update message serialization
- [ ] Maintain backwards compatibility

**Test Files** (TDD - Write First):
- `tests/websocket/test_chain_messages.rs`
  - test_session_init_with_chain()
  - test_response_includes_chain()
  - test_invalid_chain_in_message()
  - test_legacy_message_compatibility()
  - test_message_serialization()

**Success Criteria**:
- Messages include chain info
- Legacy messages still work
- Invalid chains rejected

### Sub-phase 4.2: Session Handler Updates
**Goal**: Make WebSocket handlers chain-aware

**Tasks**:
- [ ] Update `handle_session_init()` for chain
- [ ] Verify job on specified chain
- [ ] Add chain to session context
- [ ] Update streaming responses with chain info
- [ ] Handle chain switching requests

**Test Files** (TDD - Write First):
- `tests/websocket/test_session_handlers.rs`
  - test_init_handler_with_chain()
  - test_job_verification_on_chain()
  - test_streaming_with_chain_context()
  - test_chain_switch_request()
  - test_cross_chain_session_rejection()

**Success Criteria**:
- Handlers process chain correctly
- Job verification uses right chain
- Streaming maintains chain context

### Sub-phase 4.3: Connection Management
**Goal**: Track connections per chain

**Tasks**:
- [ ] Create per-chain connection pools
- [ ] Implement connection statistics by chain
- [ ] Add chain-specific rate limiting
- [ ] Create connection health monitoring
- [ ] Handle chain-specific disconnects

**Test Files** (TDD - Write First):
- `tests/websocket/test_connection_chain.rs`
  - test_connection_pool_per_chain()
  - test_connection_stats_by_chain()
  - test_rate_limiting_per_chain()
  - test_connection_health_check()
  - test_chain_specific_disconnect()

**Success Criteria**:
- Connections tracked per chain
- Statistics accurate by chain
- Rate limits enforced per chain

## Phase 5: Multi-Chain Registration

### Sub-phase 5.1: Node Registration System
**Goal**: Register node on multiple chains

**Tasks**:
- [ ] Create `MultiChainRegistrar` struct
- [ ] Implement registration on Base Sepolia
- [ ] Implement registration on opBNB
- [ ] Add registration verification
- [ ] Create registration status tracking

**Test Files** (TDD - Write First):
- `tests/registration/test_multi_registration.rs`
  - test_register_on_base_sepolia()
  - test_register_on_opbnb()
  - test_registration_verification()
  - test_registration_status()
  - test_concurrent_registration()

**Success Criteria**:
- Node registers on all chains
- Registration verified on-chain
- Status tracked correctly

### Sub-phase 5.2: Registration CLI
**Goal**: Create command-line registration tools

**Tasks**:
- [ ] Create `register-node` CLI command
- [ ] Add `--chain` parameter support
- [ ] Implement `--all-chains` option
- [ ] Add registration status command
- [ ] Create registration update command

**Test Files** (TDD - Write First):
- `tests/registration/test_registration_cli.rs`
  - test_register_single_chain_cli()
  - test_register_all_chains_cli()
  - test_status_command()
  - test_update_registration()
  - test_invalid_chain_cli()

**Success Criteria**:
- CLI commands work correctly
- All chains option registers everywhere
- Status shows all registrations

### Sub-phase 5.3: Registration Monitoring
**Goal**: Monitor registration health

**Tasks**:
- [ ] Create registration health checks
- [ ] Implement auto-renewal logic
- [ ] Add registration expiry warnings
- [ ] Create registration metrics
- [ ] Handle registration failures

**Test Files** (TDD - Write First):
- `tests/registration/test_registration_health.rs`
  - test_registration_health_check()
  - test_auto_renewal()
  - test_expiry_warnings()
  - test_registration_metrics()
  - test_failure_recovery()

**Success Criteria**:
- Health checks detect issues
- Auto-renewal works
- Warnings issued before expiry

## Phase 6: API Enhancements

### Sub-phase 6.1: HTTP API Chain Support
**Goal**: Add chain parameters to HTTP endpoints

**Tasks**:
- [ ] Update `/v1/models` with chain parameter
- [ ] Add chain_id to inference requests
- [ ] Create `/v1/chains` endpoint
- [ ] Update `/v1/session/info` with chain data
- [ ] Add chain statistics endpoint

**Test Files** (TDD - Write First):
- `tests/api/test_chain_endpoints.rs`
  - test_models_by_chain()
  - test_inference_with_chain()
  - test_chains_endpoint()
  - test_session_info_chain()
  - test_chain_stats_endpoint()

**Success Criteria**:
- Endpoints accept chain parameter
- Correct data returned per chain
- Statistics accurate

### Sub-phase 6.2: API Response Updates
**Goal**: Include chain info in API responses

**Tasks**:
- [ ] Add chain_id to inference responses
- [ ] Include native token in responses
- [ ] Add chain name to session info
- [ ] Update error messages with chain context
- [ ] Create chain-aware response formatting

**Test Files** (TDD - Write First):
- `tests/api/test_chain_responses.rs`
  - test_inference_response_chain()
  - test_native_token_in_response()
  - test_chain_name_included()
  - test_error_with_chain_context()
  - test_response_formatting()

**Success Criteria**:
- Responses include chain info
- Native token correctly identified
- Errors provide chain context

### Sub-phase 6.3: API Documentation
**Goal**: Update API docs for multi-chain

**Tasks**:
- [ ] Update OpenAPI specification
- [ ] Add chain parameter examples
- [ ] Document chain-specific behaviors
- [ ] Create migration guide
- [ ] Add troubleshooting section

**Test Files** (TDD - Write First):
- `tests/api/test_api_docs.rs`
  - test_openapi_spec_valid()
  - test_example_requests()
  - test_documentation_completeness()

**Success Criteria**:
- OpenAPI spec validates
- Examples work correctly
- Documentation comprehensive

## Phase 7: Gas Management

### Sub-phase 7.1: Gas Estimation System
**Goal**: Implement chain-specific gas estimation

**Tasks**:
- [ ] Create `GasEstimator` trait
- [ ] Implement Base Sepolia estimator
- [ ] Implement opBNB estimator
- [ ] Add gas price monitoring
- [ ] Create gas limit calculations

**Test Files** (TDD - Write First):
- `tests/gas/test_gas_estimation.rs`
  - test_base_sepolia_gas_estimate()
  - test_opbnb_gas_estimate()
  - test_gas_price_monitoring()
  - test_gas_limit_calculation()
  - test_gas_spike_handling()

**Success Criteria**:
- Gas estimates accurate
- Price monitoring works
- Limits appropriate per chain

### Sub-phase 7.2: Gas Optimization
**Goal**: Optimize gas usage per chain

**Tasks**:
- [ ] Implement transaction batching
- [ ] Add gas price multipliers
- [ ] Create priority fee logic
- [ ] Implement MEV protection
- [ ] Add gas saving strategies

**Test Files** (TDD - Write First):
- `tests/gas/test_gas_optimization.rs`
  - test_transaction_batching()
  - test_gas_multiplier_application()
  - test_priority_fee_calculation()
  - test_mev_protection()
  - test_gas_saving_strategies()

**Success Criteria**:
- Batching reduces gas costs
- Multipliers applied correctly
- MEV protection active

### Sub-phase 7.3: Balance Monitoring
**Goal**: Monitor native token balances

**Tasks**:
- [ ] Create balance monitoring service
- [ ] Add low balance alerts
- [ ] Implement balance thresholds per chain
- [ ] Create top-up notifications
- [ ] Add balance metrics

**Test Files** (TDD - Write First):
- `tests/gas/test_balance_monitoring.rs`
  - test_balance_check_all_chains()
  - test_low_balance_alert()
  - test_threshold_configuration()
  - test_topup_notification()
  - test_balance_metrics()

**Success Criteria**:
- Balances monitored on all chains
- Alerts triggered at thresholds
- Notifications sent correctly

## Phase 8: Error Handling & Recovery

### Sub-phase 8.1: Chain-Specific Error Handling
**Goal**: Handle errors per chain gracefully

**Tasks**:
- [ ] Create `ChainError` enum
- [ ] Implement error mapping per chain
- [ ] Add retry logic with backoff
- [ ] Create error recovery strategies
- [ ] Implement fallback mechanisms

**Test Files** (TDD - Write First):
- `tests/errors/test_chain_errors.rs`
  - test_chain_error_types()
  - test_error_mapping()
  - test_retry_with_backoff()
  - test_recovery_strategies()
  - test_fallback_activation()

**Success Criteria**:
- Errors handled appropriately
- Retries work with backoff
- Fallbacks activate on failure

### Sub-phase 8.2: Transaction Recovery
**Goal**: Recover from failed transactions

**Tasks**:
- [ ] Implement transaction monitoring
- [ ] Create stuck transaction detection
- [ ] Add transaction replacement logic
- [ ] Implement nonce management
- [ ] Create transaction history tracking

**Test Files** (TDD - Write First):
- `tests/errors/test_tx_recovery.rs`
  - test_transaction_monitoring()
  - test_stuck_tx_detection()
  - test_tx_replacement()
  - test_nonce_management()
  - test_tx_history()

**Success Criteria**:
- Stuck transactions detected
- Replacements work correctly
- Nonce issues resolved

### Sub-phase 8.3: System Recovery
**Goal**: Recover from system failures

**Tasks**:
- [ ] Create state recovery on restart
- [ ] Implement session recovery per chain
- [ ] Add checkpoint system
- [ ] Create backup mechanisms
- [ ] Implement disaster recovery

**Test Files** (TDD - Write First):
- `tests/errors/test_system_recovery.rs`
  - test_state_recovery()
  - test_session_recovery_all_chains()
  - test_checkpoint_restore()
  - test_backup_mechanisms()
  - test_disaster_recovery()

**Success Criteria**:
- State recovered after restart
- Sessions restored correctly
- Checkpoints work reliably

## Phase 9: Monitoring & Metrics

### Sub-phase 9.1: Chain-Specific Metrics
**Goal**: Track metrics per chain

**Tasks**:
- [ ] Create Prometheus metrics per chain
- [ ] Add session metrics by chain
- [ ] Track settlement metrics
- [ ] Monitor gas usage metrics
- [ ] Create performance metrics

**Test Files** (TDD - Write First):
- `tests/monitoring/test_chain_metrics.rs`
  - test_prometheus_metrics()
  - test_session_metrics_by_chain()
  - test_settlement_metrics()
  - test_gas_usage_tracking()
  - test_performance_metrics()

**Success Criteria**:
- Metrics exported correctly
- Data accurate per chain
- Prometheus can scrape

### Sub-phase 9.2: Health Monitoring
**Goal**: Monitor health per chain

**Tasks**:
- [ ] Create health check endpoints
- [ ] Add RPC health monitoring
- [ ] Implement contract health checks
- [ ] Create alert system
- [ ] Add health dashboards

**Test Files** (TDD - Write First):
- `tests/monitoring/test_health.rs`
  - test_health_endpoints()
  - test_rpc_health_check()
  - test_contract_health()
  - test_alert_triggering()
  - test_dashboard_data()

**Success Criteria**:
- Health checks accurate
- Alerts trigger correctly
- Dashboards show real data

### Sub-phase 9.3: Logging Enhancement
**Goal**: Improve logging for multi-chain

**Tasks**:
- [ ] Add chain context to logs
- [ ] Create structured logging
- [ ] Implement log aggregation
- [ ] Add transaction tracing
- [ ] Create audit logging

**Test Files** (TDD - Write First):
- `tests/monitoring/test_logging.rs`
  - test_chain_context_in_logs()
  - test_structured_logging()
  - test_log_aggregation()
  - test_transaction_tracing()
  - test_audit_logs()

**Success Criteria**:
- Logs include chain context
- Structured format consistent
- Tracing works end-to-end

## Phase 10: Integration Testing

### Sub-phase 10.1: End-to-End Testing
**Goal**: Test complete multi-chain flow

**Tasks**:
- [ ] Create E2E test framework
- [ ] Test Base Sepolia flow
- [ ] Test opBNB flow
- [ ] Test chain switching
- [ ] Test concurrent operations

**Test Files** (TDD - Write First):
- `tests/integration/test_e2e_multichain.rs`
  - test_complete_base_sepolia_flow()
  - test_complete_opbnb_flow()
  - test_chain_switching_flow()
  - test_concurrent_chain_operations()
  - test_cross_chain_scenarios()

**Success Criteria**:
- Full flows work on both chains
- Chain switching seamless
- Concurrent ops successful

### Sub-phase 10.2: Load Testing
**Goal**: Test system under load

**Tasks**:
- [ ] Create load testing framework
- [ ] Test high session volume
- [ ] Test settlement throughput
- [ ] Test RPC rate limits
- [ ] Test failover under load

**Test Files** (TDD - Write First):
- `tests/integration/test_load.rs`
  - test_high_session_volume()
  - test_settlement_throughput()
  - test_rpc_rate_limits()
  - test_failover_under_load()
  - test_resource_utilization()

**Success Criteria**:
- System stable under load
- Throughput meets targets
- Failover works under stress

### Sub-phase 10.3: Security Testing
**Goal**: Validate security measures

**Tasks**:
- [ ] Test chain ID validation
- [ ] Test replay protection
- [ ] Test signature verification
- [ ] Test access controls
- [ ] Test input validation

**Test Files** (TDD - Write First):
- `tests/integration/test_security.rs`
  - test_chain_id_spoofing()
  - test_replay_attack_prevention()
  - test_signature_verification()
  - test_unauthorized_access()
  - test_malicious_inputs()

**Success Criteria**:
- Security measures effective
- Attacks prevented
- Access controls enforced

## Phase 11: Documentation & Deployment

### Sub-phase 11.1: Documentation
**Goal**: Complete documentation update

**Tasks**:
- [ ] Update README with multi-chain info
- [ ] Create configuration guide
- [ ] Write deployment guide
- [ ] Create troubleshooting guide
- [ ] Update API documentation

**Deliverables**:
- `docs/MULTI_CHAIN_GUIDE.md`
- `docs/DEPLOYMENT_MULTI_CHAIN.md`
- `docs/TROUBLESHOOTING_CHAINS.md`
- Updated `README.md`
- Updated `API.md`

**Success Criteria**:
- Documentation complete
- Examples work correctly
- Guides comprehensive

### Sub-phase 11.2: Migration Tools
**Goal**: Create migration utilities

**Tasks**:
- [ ] Create migration script
- [ ] Implement data migration
- [ ] Add rollback capability
- [ ] Create verification tools
- [ ] Document migration process

**Test Files** (TDD - Write First):
- `tests/migration/test_migration.rs`
  - test_migration_script()
  - test_data_migration()
  - test_rollback_capability()
  - test_migration_verification()
  - test_zero_downtime_migration()

**Success Criteria**:
- Migration works smoothly
- Rollback functions correctly
- Zero data loss

### Sub-phase 11.3: Production Deployment
**Goal**: Deploy to production

**Tasks**:
- [ ] Create deployment scripts
- [ ] Configure production environment
- [ ] Deploy to Base Sepolia mainnet
- [ ] Deploy to opBNB mainnet
- [ ] Verify all systems operational

**Deliverables**:
- Deployment scripts
- Production configuration
- Monitoring dashboards
- Operational runbook

**Success Criteria**:
- Deployment successful
- All chains operational
- Monitoring active

## Implementation Timeline

**Phase 1**: 1 week - Chain Configuration Foundation
**Phase 2**: 1 week - Session Chain Tracking
**Phase 3**: 1 week - Multi-Chain Settlement
**Phase 4**: 1 week - WebSocket Protocol Updates
**Phase 5**: 1 week - Multi-Chain Registration
**Phase 6**: 1 week - API Enhancements
**Phase 7**: 1 week - Gas Management
**Phase 8**: 1 week - Error Handling & Recovery
**Phase 9**: 1 week - Monitoring & Metrics
**Phase 10**: 1 week - Integration Testing
**Phase 11**: 1 week - Documentation & Deployment

**Total Timeline**: 11 weeks

## Critical Path

1. **Phase 1.1-1.3**: Foundation must be solid
2. **Phase 2.1-2.2**: Session tracking essential
3. **Phase 3.1-3.2**: Settlement core functionality
4. **Phase 4.1**: Protocol compatibility
5. **Phase 10.1**: End-to-end validation

## Risk Mitigation

1. **RPC Reliability**: Multiple endpoints per chain
2. **Gas Spikes**: Dynamic gas adjustment
3. **Chain Congestion**: Transaction retry logic
4. **Breaking Changes**: Version compatibility checks
5. **Data Loss**: Comprehensive backup strategy

## Success Metrics

- **Functional**: All tests passing (100%)
- **Performance**: Settlement < 30s on both chains
- **Reliability**: 99.9% uptime per chain
- **Cost**: Gas costs optimized (< 10% overhead)
- **Security**: No vulnerabilities in audit

## Notes

- Each sub-phase should be completed before moving to the next
- Write tests FIRST (TDD approach)
- Keep backward compatibility throughout
- Document all breaking changes
- Maintain single-chain functionality as fallback