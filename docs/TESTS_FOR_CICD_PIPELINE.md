# CI/CD Test Pipeline Documentation

## Overview

This document defines the test suite that should run in the CI/CD pipeline for the Fabstir LLM Node project. Tests are categorized by priority to ensure critical functionality is validated before deployment while maintaining reasonable CI execution times.

## Test Categories

### 🔴 Critical Tests (Must Pass for Deployment)

These tests validate core functionality and must pass before any deployment to production.

#### 1. Unit Tests
```bash
cargo test --lib
```
- **Purpose**: Tests individual components and functions in isolation
- **Timeout**: 5 minutes
- **Dependencies**: None
- **Coverage**: Core business logic, utility functions, data structures

#### 2. Integration Tests
```bash
cargo test --test integration_tests
cargo test --test test_host_management
cargo test --test test_job_assignment
```
- **Purpose**: Validates interactions between components
- **Timeout**: 10 minutes
- **Dependencies**: May require mock services
- **Coverage**: Job assignment, host registration, session management

#### 3. Contract Tests
```bash
cargo test --test contracts_tests
```
- **Purpose**: Validates smart contract interactions
- **Timeout**: 5 minutes
- **Dependencies**: Requires contract ABIs
- **Coverage**: Payment verification, checkpoint submission, escrow management

#### 4. API Tests
```bash
cargo test --test api_tests
```
- **Purpose**: Ensures HTTP and inference endpoints work correctly
- **Timeout**: 5 minutes
- **Dependencies**: None (uses mocks)
- **Coverage**: REST API endpoints, request/response handling

#### 5. WebSocket Tests
```bash
cargo test --test websocket_tests
```
- **Purpose**: Validates WebSocket communication for SDK integration
- **Timeout**: 5 minutes
- **Dependencies**: None (uses mocks)
- **Coverage**: Session management, streaming, message protocols

### 🟡 Secondary Tests (Should Pass, Non-blocking)

These tests are important but failures shouldn't block deployment if critical tests pass.

#### 6. Storage Tests (Mock)
```bash
cargo test --test storage_tests -- --skip real
```
- **Purpose**: Tests storage abstraction with mock S5 backend
- **Timeout**: 3 minutes
- **Dependencies**: None (uses mocks)

#### 7. Model Management Tests
```bash
cargo test --test models_tests -- --skip download
```
- **Purpose**: Tests model validation and caching logic
- **Timeout**: 3 minutes
- **Dependencies**: None (skips actual downloads)

#### 8. Payment Tests
```bash
cargo test --test payments_tests
```
- **Purpose**: Tests payment calculation and verification logic
- **Timeout**: 3 minutes
- **Dependencies**: None (uses mocks)

### 🟢 Optional Tests (Run Nightly/Weekly)

These tests are resource-intensive or require special setup.

#### 9. Performance Tests
```bash
cargo test --test performance_tests
```
- **Purpose**: Benchmarks and performance regression testing
- **Timeout**: 30 minutes
- **Dependencies**: May require GPU
- **When to run**: Nightly builds only

#### 10. Real Storage Tests
```bash
cargo test --test storage_tests -- real
```
- **Purpose**: Tests against actual S5 network
- **Timeout**: 15 minutes
- **Dependencies**: S5 network access, API keys
- **When to run**: Pre-release validation

#### 11. EZKL Proof Tests
```bash
cargo test --test ezkl_tests
```
- **Purpose**: Tests cryptographic proof generation
- **Timeout**: 20 minutes
- **Dependencies**: EZKL setup, proving keys
- **When to run**: Weekly or pre-release

## GitHub Actions Workflow

### Basic CI Pipeline (.github/workflows/ci.yml)

```yaml
name: CI Pipeline

on:
  push:
    branches: [ main, develop ]
  pull_request:
    branches: [ main ]

env:
  CARGO_TERM_COLOR: always
  RUST_BACKTRACE: 1

jobs:
  test:
    name: Test Suite
    runs-on: ubuntu-latest
    timeout-minutes: 20

    steps:
    - uses: actions/checkout@v4

    - name: Setup Rust
      uses: actions-rust-lang/setup-rust-toolchain@v1
      with:
        toolchain: stable
        components: rustfmt, clippy

    - name: Cache dependencies
      uses: actions/cache@v3
      with:
        path: |
          ~/.cargo/bin/
          ~/.cargo/registry/index/
          ~/.cargo/registry/cache/
          ~/.cargo/git/db/
          target/
        key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}
        restore-keys: |
          ${{ runner.os }}-cargo-

    # Linting and formatting checks
    - name: Check formatting
      run: cargo fmt -- --check

    - name: Run clippy
      run: cargo clippy -- -D warnings

    # Critical tests
    - name: Run unit tests
      run: cargo test --lib

    - name: Run integration tests
      run: |
        cargo test --test integration_tests
        cargo test --test test_host_management
        cargo test --test test_job_assignment

    - name: Run contract tests
      run: cargo test --test contracts_tests

    - name: Run API tests
      run: cargo test --test api_tests

    - name: Run WebSocket tests
      run: cargo test --test websocket_tests

    # Secondary tests (continue on error)
    - name: Run storage tests
      continue-on-error: true
      run: cargo test --test storage_tests -- --skip real

    - name: Run model tests
      continue-on-error: true
      run: cargo test --test models_tests -- --skip download

    - name: Run payment tests
      continue-on-error: true
      run: cargo test --test payments_tests

  build:
    name: Build Check
    runs-on: ubuntu-latest
    needs: test

    steps:
    - uses: actions/checkout@v4

    - name: Setup Rust
      uses: actions-rust-lang/setup-rust-toolchain@v1
      with:
        toolchain: stable

    - name: Build release binary
      run: cargo build --release

    - name: Check binary size
      run: |
        size=$(stat -c%s target/release/fabstir-llm-node)
        echo "Binary size: $size bytes"
        if [ $size -gt 104857600 ]; then
          echo "Warning: Binary larger than 100MB"
        fi
```

### Nightly Test Pipeline (.github/workflows/nightly.yml)

```yaml
name: Nightly Tests

on:
  schedule:
    - cron: '0 2 * * *'  # Run at 2 AM UTC daily
  workflow_dispatch:  # Allow manual trigger

jobs:
  comprehensive-tests:
    name: Full Test Suite
    runs-on: ubuntu-latest
    timeout-minutes: 60

    steps:
    - uses: actions/checkout@v4

    - name: Setup Rust
      uses: actions-rust-lang/setup-rust-toolchain@v1
      with:
        toolchain: stable

    - name: Run all tests including heavy ones
      run: |
        cargo test --all-features
        cargo test --test performance_tests || true
        cargo test --test ezkl_tests || true

    - name: Generate test report
      if: always()
      run: |
        cargo test --all-features -- --format json > test-results.json || true
        echo "Test results saved to test-results.json"
```

## Local Testing Commands

### Quick CI Validation (Before Push)
```bash
# Run this before pushing to catch CI failures early
./scripts/ci-local.sh
```

Create `scripts/ci-local.sh`:
```bash
#!/bin/bash
set -e

echo "🔍 Running CI validation locally..."

echo "📝 Checking formatting..."
cargo fmt -- --check

echo "📎 Running clippy..."
cargo clippy -- -D warnings

echo "🧪 Running critical tests..."
cargo test --lib
cargo test --test integration_tests
cargo test --test test_job_assignment
cargo test --test contracts_tests
cargo test --test api_tests
cargo test --test websocket_tests

echo "✅ All CI checks passed!"
```

### Full Test Suite
```bash
# Run comprehensive tests (takes longer)
cargo test --all-features
```

### Test Specific Component
```bash
# Test only contract-related functionality
cargo test contracts::

# Test only WebSocket functionality
cargo test websocket::

# Test with output for debugging
cargo test -- --nocapture
```

## Test Dependencies and Requirements

### Environment Variables for Tests

Create `.env.test` file:
```bash
# Test configuration
TEST_MODE=true
RUST_LOG=debug
RUST_BACKTRACE=1

# Mock service endpoints
MOCK_S5_URL=http://localhost:5522
MOCK_VECTOR_DB_URL=http://localhost:8081

# Test accounts (Base Sepolia)
TEST_RPC_URL=https://sepolia.base.org
TEST_USER_ADDRESS=0x...
TEST_HOST_ADDRESS=0x...

# Contract addresses for testing
CONTRACT_JOB_MARKETPLACE=0x1273E6358aa52Bb5B160c34Bf2e617B745e4A944
CONTRACT_HOST_EARNINGS=0x908962e8c6CE72610021586f85ebDE09aAc97776
CONTRACT_NODE_REGISTRY=0x2AA37Bb6E9f0a5d0F3b2836f3a5F656755906218

# Payment percentages
TREASURY_FEE_PERCENTAGE=10
HOST_EARNINGS_PERCENTAGE=90
```

### Required System Dependencies

```yaml
# In CI environment, install these:
- build-essential
- pkg-config
- libssl-dev
- protobuf-compiler
```

## Common Test Failures and Solutions

### 1. Contract Test Failures
```
Error: Failed to connect to RPC endpoint
```
**Solution**: Ensure TEST_RPC_URL is accessible and valid

### 2. Compilation Errors
```
Error: could not compile `fabstir-llm-node`
```
**Solution**: Clear cargo cache and rebuild
```bash
cargo clean
cargo build --tests
```

### 3. Timeout Issues
```
Error: test timed out after 60 seconds
```
**Solution**: Increase test timeout or skip heavy tests in CI
```rust
#[test]
#[ignore] // Skip in normal test runs
fn heavy_test() {
    // ...
}
```

### 4. Missing Dependencies
```
Error: failed to run custom build command for openssl-sys
```
**Solution**: Install system dependencies
```bash
sudo apt-get install pkg-config libssl-dev
```

## Test Coverage

### Generating Coverage Report (Local)
```bash
# Install tarpaulin
cargo install cargo-tarpaulin

# Generate coverage
cargo tarpaulin --out Html --output-dir ./coverage
```

### Coverage Thresholds
- **Critical components**: Minimum 80% coverage
- **Core business logic**: Minimum 70% coverage
- **Utilities**: Minimum 60% coverage

## Performance Benchmarks

### Running Benchmarks
```bash
cargo bench
```

### Key Metrics to Monitor
- Inference latency: < 100ms for small prompts
- WebSocket message throughput: > 1000 msg/s
- Memory usage: < 2GB under normal load
- Checkpoint submission: < 5s per submission

## Deployment Gates

### Pre-deployment Checklist
- [ ] All critical tests pass
- [ ] No clippy warnings
- [ ] Code formatted with rustfmt
- [ ] Binary size < 100MB
- [ ] Documentation updated
- [ ] Version bumped in Cargo.toml
- [ ] CHANGELOG.md updated

### Smoke Test After Deployment
```bash
# Quick validation that deployed node works
curl http://localhost:8080/health
curl http://localhost:8080/v1/models
```

## Maintenance

### Adding New Tests
1. Add test file to appropriate directory under `/tests/`
2. Update this document with new test category if needed
3. Add to CI pipeline if it's a critical test
4. Document any special requirements

### Removing Obsolete Tests
1. Mark as `#[ignore]` first
2. Remove after one release cycle
3. Update this documentation

## Contact

For questions about tests or CI/CD pipeline:
- Create issue in GitHub repository
- Tag with `testing` or `ci/cd` label
- Include test output and environment details