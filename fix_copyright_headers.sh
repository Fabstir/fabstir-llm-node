#!/bin/bash
# Fix copyright headers - remove the --- separator lines

fix_file() {
    local file="$1"

    # Check if file has the old format with ---
    if head -n 1 "$file" | grep -q "^[/#]* ---$"; then
        # Create temp file with fixed header
        {
            # Skip first line (---)
            # Print lines 2-3 (copyright and SPDX)
            sed -n '2,3p' "$file"
            # Skip line 4 (---), print from line 5 onwards
            tail -n +5 "$file"
        } > "$file.tmp"

        mv "$file.tmp" "$file"
        echo "✓ Fixed: $file"
        return 0
    fi
    return 1
}

count=0

echo "Fixing Rust files..."
while IFS= read -r file; do
    if fix_file "$file"; then
        ((count++))
    fi
done < <(find /workspace/{src,tests,examples,benches} -type f -name "*.rs" 2>/dev/null)

echo "Fixing Shell scripts..."
while IFS= read -r file; do
    # Handle shebang files differently
    if head -n 1 "$file" | grep -q "^#!"; then
        # Check if line 2 is ---
        if sed -n '2p' "$file" | grep -q "^# ---$"; then
            {
                # Keep shebang (line 1)
                head -n 1 "$file"
                # Print copyright and SPDX (lines 3-4)
                sed -n '3,4p' "$file"
                # Skip line 5 (---), print from line 6 onwards
                tail -n +6 "$file"
            } > "$file.tmp"
            mv "$file.tmp" "$file"
            echo "✓ Fixed: $file (with shebang)"
            ((count++))
        fi
    else
        if fix_file "$file"; then
            ((count++))
        fi
    fi
done < <(find /workspace/{scripts,src,tests,.devcontainer} -type f \( -name "*.sh" -o -name "*.bash" \) 2>/dev/null)

# Root level shell scripts
for file in /workspace/*.sh; do
    if [ -f "$file" ]; then
        if head -n 1 "$file" | grep -q "^#!"; then
            if sed -n '2p' "$file" | grep -q "^# ---$"; then
                {
                    head -n 1 "$file"
                    sed -n '3,4p' "$file"
                    tail -n +6 "$file"
                } > "$file.tmp"
                mv "$file.tmp" "$file"
                echo "✓ Fixed: $file (with shebang)"
                ((count++))
            fi
        else
            if fix_file "$file"; then
                ((count++))
            fi
        fi
    fi
done

echo "Fixing Python files..."
while IFS= read -r file; do
    if head -n 1 "$file" | grep -q "^#!"; then
        if sed -n '2p' "$file" | grep -q "^# ---$"; then
            {
                head -n 1 "$file"
                sed -n '3,4p' "$file"
                tail -n +6 "$file"
            } > "$file.tmp"
            mv "$file.tmp" "$file"
            echo "✓ Fixed: $file (with shebang)"
            ((count++))
        fi
    else
        if fix_file "$file"; then
            ((count++))
        fi
    fi
done < <(find /workspace/{src,tests,scripts,examples} -type f -name "*.py" 2>/dev/null)

echo "Fixing TOML files..."
for file in /workspace/*.toml; do
    if [ -f "$file" ] && [ "$file" != "/workspace/Cargo.lock" ]; then
        if fix_file "$file"; then
            ((count++))
        fi
    fi
done

# Guest Cargo.toml
if [ -f "/workspace/methods/guest/Cargo.toml" ]; then
    if fix_file "/workspace/methods/guest/Cargo.toml"; then
        ((count++))
    fi
fi

echo "Fixing YAML files..."
while IFS= read -r file; do
    if fix_file "$file"; then
        ((count++))
    fi
done < <(find /workspace/{.github,.devcontainer,deployment,docs} -type f \( -name "*.yml" -o -name "*.yaml" \) 2>/dev/null)

echo "Fixing Dockerfiles..."
for file in /workspace/Dockerfile* /workspace/.devcontainer/Dockerfile; do
    if [ -f "$file" ]; then
        if fix_file "$file"; then
            ((count++))
        fi
    fi
done

echo ""
echo "✅ Done! Fixed $count files"
