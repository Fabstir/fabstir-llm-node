#!/bin/bash
# Quick script to verify test files exist and count tests

echo "Verifying Phase 3.4 Monitoring Test Files"
echo "========================================"
echo ""

# Check if test files exist
echo "Checking test files..."
for file in test_metrics test_health_checks test_alerting test_dashboards; do
    if [ -f "tests/monitoring/$file.rs" ]; then
        echo "✓ tests/monitoring/$file.rs exists"
        test_count=$(grep -c "#\[tokio::test\]" "tests/monitoring/$file.rs")
        echo "  → Found $test_count tests"
    else
        echo "✗ tests/monitoring/$file.rs NOT FOUND"
    fi
done

echo ""
echo "Checking test runner..."
if [ -f "tests/monitoring_tests.rs" ]; then
    echo "✓ tests/monitoring_tests.rs exists"
else
    echo "✗ tests/monitoring_tests.rs NOT FOUND"
fi

echo ""
echo "Checking run script..."
if [ -f "run_monitoring_tests.sh" ]; then
    echo "✓ run_monitoring_tests.sh exists"
    chmod +x run_monitoring_tests.sh
    echo "  → Made executable"
else
    echo "✗ run_monitoring_tests.sh NOT FOUND"
fi

echo ""
echo "Total expected tests: 52 (13 per module)"
echo ""
echo "Next steps for Claude Code:"
echo "1. Create src/monitoring/ directory"
echo "2. Implement modules to make tests pass"
echo "3. Run ./run_monitoring_tests.sh to verify"