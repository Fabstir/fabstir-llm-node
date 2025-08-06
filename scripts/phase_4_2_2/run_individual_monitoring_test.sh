#\!/bin/bash
# Save as: run_individual_monitoring_test.sh

if [ -z "$1" ]; then
    echo "Usage: ./run_individual_monitoring_test.sh <test_module>"
    echo ""
    echo "Available test modules:"
    echo "  test_metrics"
    echo "  test_health_checks"
    echo "  test_alerting"
    echo "  test_dashboards"
    exit 1
fi

# Disable caching
unset RUSTC_WRAPPER
export CARGO_INCREMENTAL=0

# Run the specified test module
cargo test --test monitoring_tests monitoring::$1 -- --nocapture 2>&1 | \
    grep -E "(test .* \.\.\.|test result:|passed|failed|FAILED|error\[E[0-9]+\])"
