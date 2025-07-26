#!/bin/bash
# fabstir-llm-node/.devcontainer/yolo-runner.sh

echo "🚀 Fabstir Node YOLO Mode"
echo "========================="

# Initialize if needed
if [ ! -f "Cargo.toml" ]; then
    cargo init --name fabstir-llm-node
fi

# Start test watcher
exec /usr/local/bin/test-watcher.sh