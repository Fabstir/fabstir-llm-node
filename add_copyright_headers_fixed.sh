#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1

# Script to add copyright headers ONLY to project source files (not dependencies)

# Define the copyright header for different comment styles
RUST_HEADER="// ---
// Copyright (c) 2025 Fabstir
// SPDX-License-Identifier: BUSL-1.1
// ---
"

HASH_HEADER="# ---
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1
# ---
"

# Function to add header to a file if it doesn't already have it
add_header() {
    local file="$1"
    local header="$2"

    # Check if file already has the copyright header
    if grep -q "Copyright (c) 2025 Fabstir" "$file"; then
        return
    fi

    # Create temporary file with header + original content
    {
        echo -n "$header"
        cat "$file"
    } > "$file.tmp"

    # Replace original file
    mv "$file.tmp" "$file"
    echo "✓ $file"
}

# Function to add header preserving shebang
add_header_with_shebang() {
    local file="$1"
    local header="$2"

    # Check if file already has the copyright header
    if grep -q "Copyright (c) 2025 Fabstir" "$file"; then
        return
    fi

    if head -n 1 "$file" | grep -q "^#!"; then
        # Extract shebang and rest of file
        shebang=$(head -n 1 "$file")
        tail -n +2 "$file" > "$file.body"
        # Reconstruct: shebang + header + body
        {
            echo "$shebang"
            echo "$HASH_HEADER"
            cat "$file.body"
        } > "$file.tmp"
        rm "$file.body"
        mv "$file.tmp" "$file"
        echo "✓ $file (shebang preserved)"
    else
        add_header "$file" "$header"
    fi
}

count=0

echo "Processing Rust files..."
while IFS= read -r file; do
    add_header "$file" "$RUST_HEADER"
    ((count++))
done < <(find /workspace/{src,tests,examples,benches} -type f -name "*.rs" 2>/dev/null)

echo "Processing Shell scripts..."
while IFS= read -r file; do
    add_header_with_shebang "$file" "$HASH_HEADER"
    ((count++))
done < <(find /workspace/{scripts,src,tests,.devcontainer} -type f \( -name "*.sh" -o -name "*.bash" \) 2>/dev/null)

# Root level shell scripts
for file in /workspace/*.sh; do
    if [ -f "$file" ]; then
        add_header_with_shebang "$file" "$HASH_HEADER"
        ((count++))
    fi
done

echo "Processing Python files..."
while IFS= read -r file; do
    add_header_with_shebang "$file" "$HASH_HEADER"
    ((count++))
done < <(find /workspace/{src,tests,scripts,examples} -type f -name "*.py" 2>/dev/null)

echo "Processing configuration files..."
# Root level TOML
for file in /workspace/*.toml; do
    if [ -f "$file" ] && [ "$file" != "/workspace/Cargo.lock" ]; then
        add_header "$file" "$HASH_HEADER"
        ((count++))
    fi
done

# YAML files (excluding docs/openapi.yaml which was already processed)
while IFS= read -r file; do
    add_header "$file" "$HASH_HEADER"
    ((count++))
done < <(find /workspace/{.github,.devcontainer} -type f \( -name "*.yml" -o -name "*.yaml" \) 2>/dev/null)

# Docker compose files in root
for file in /workspace/docker-compose*.yml; do
    if [ -f "$file" ]; then
        add_header "$file" "$HASH_HEADER"
        ((count++))
    fi
done

echo "Processing Dockerfiles..."
for file in /workspace/Dockerfile* /workspace/.devcontainer/Dockerfile; do
    if [ -f "$file" ]; then
        add_header "$file" "$HASH_HEADER"
        ((count++))
    fi
done

echo ""
echo "✅ Done! Processed $count project files"
echo ""
echo "⚠️  NOTE: Third-party files in .cargo/registry/ were incorrectly modified."
echo "   Run: git checkout .cargo/ to revert those changes"
