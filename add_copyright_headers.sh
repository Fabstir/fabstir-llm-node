#!/bin/bash
# Copyright (c) 2025 Fabstir
# SPDX-License-Identifier: BUSL-1.1

# Script to add copyright headers to all project source files

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
        echo "Skipping $file (already has header)"
        return
    fi

    # Create temporary file with header + original content
    {
        echo -n "$header"
        cat "$file"
    } > "$file.tmp"

    # Replace original file
    mv "$file.tmp" "$file"
    echo "Added header to: $file"
}

# Counter for files processed
count=0

# Process Rust files (.rs)
echo "Processing Rust files..."
while IFS= read -r -d '' file; do
    add_header "$file" "$RUST_HEADER"
    ((count++))
done < <(find /workspace -type f -name "*.rs" \
    ! -path "*/target/*" \
    ! -path "*/.git/*" \
    ! -path "*/methods/guest/*" \
    ! -path "*/.cargo/*" \
    -print0)

# Process Shell scripts (.sh, .bash)
echo "Processing shell scripts..."
while IFS= read -r -d '' file; do
    # Skip if it's a shebang file - need to preserve shebang at top
    if head -n 1 "$file" | grep -q "^#!"; then
        # Extract shebang
        shebang=$(head -n 1 "$file")
        # Get rest of file
        tail -n +2 "$file" > "$file.body"
        # Reconstruct: shebang + header + body
        {
            echo "$shebang"
            echo "$HASH_HEADER"
            cat "$file.body"
        } > "$file.tmp"
        rm "$file.body"
        mv "$file.tmp" "$file"
        echo "Added header to: $file (preserved shebang)"
    else
        add_header "$file" "$HASH_HEADER"
    fi
    ((count++))
done < <(find /workspace -type f \( -name "*.sh" -o -name "*.bash" \) \
    ! -path "*/node_modules/*" \
    ! -path "*/target/*" \
    ! -path "*/.git/*" \
    -print0)

# Process Python files (.py)
echo "Processing Python files..."
while IFS= read -r -d '' file; do
    # Check for shebang
    if head -n 1 "$file" | grep -q "^#!"; then
        shebang=$(head -n 1 "$file")
        tail -n +2 "$file" > "$file.body"
        {
            echo "$shebang"
            echo "$HASH_HEADER"
            cat "$file.body"
        } > "$file.tmp"
        rm "$file.body"
        mv "$file.tmp" "$file"
        echo "Added header to: $file (preserved shebang)"
    else
        add_header "$file" "$HASH_HEADER"
    fi
    ((count++))
done < <(find /workspace -type f -name "*.py" \
    ! -path "*/node_modules/*" \
    ! -path "*/target/*" \
    ! -path "*/.git/*" \
    ! -path "*/venv/*" \
    ! -path "*/__pycache__/*" \
    -print0)

# Process TOML files (.toml)
echo "Processing TOML files..."
while IFS= read -r -d '' file; do
    add_header "$file" "$HASH_HEADER"
    ((count++))
done < <(find /workspace -type f -name "*.toml" \
    ! -path "*/target/*" \
    ! -path "*/.git/*" \
    ! -path "*/.cargo/*" \
    -print0)

# Process YAML files (.yml, .yaml)
echo "Processing YAML files..."
while IFS= read -r -d '' file; do
    add_header "$file" "$HASH_HEADER"
    ((count++))
done < <(find /workspace -type f \( -name "*.yml" -o -name "*.yaml" \) \
    ! -path "*/node_modules/*" \
    ! -path "*/target/*" \
    ! -path "*/.git/*" \
    -print0)

# Process Dockerfiles
echo "Processing Dockerfiles..."
while IFS= read -r -d '' file; do
    add_header "$file" "$HASH_HEADER"
    ((count++))
done < <(find /workspace -type f -name "Dockerfile*" \
    ! -path "*/node_modules/*" \
    ! -path "*/target/*" \
    ! -path "*/.git/*" \
    -print0)

# Process Makefiles
echo "Processing Makefiles..."
while IFS= read -r -d '' file; do
    add_header "$file" "$HASH_HEADER"
    ((count++))
done < <(find /workspace -type f -name "Makefile*" \
    ! -path "*/node_modules/*" \
    ! -path "*/target/*" \
    ! -path "*/.git/*" \
    -print0)

echo ""
echo "✅ Done! Processed $count files"
