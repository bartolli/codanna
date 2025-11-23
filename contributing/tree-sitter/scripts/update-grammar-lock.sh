#!/usr/bin/env bash
set -e

# Determine paths
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
GRAMMARS_DIR="$SCRIPT_DIR/../grammars"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
LOCKFILE="$PROJECT_ROOT/contributing/parsers/grammar-versions.lock"

# Function to get repo URL for a language
get_repo_url() {
    case "$1" in
        c) echo "https://github.com/tree-sitter/tree-sitter-c" ;;
        cpp) echo "https://github.com/tree-sitter/tree-sitter-cpp" ;;
        csharp) echo "https://github.com/tree-sitter/tree-sitter-c-sharp" ;;
        gdscript) echo "https://github.com/PrestonKnopp/tree-sitter-gdscript" ;;
        go) echo "https://github.com/tree-sitter/tree-sitter-go" ;;
        java) echo "https://github.com/tree-sitter/tree-sitter-java" ;;
        javascript) echo "https://github.com/tree-sitter/tree-sitter-javascript" ;;
        kotlin) echo "https://github.com/bartolli/tree-sitter-kotlin" ;;
        php) echo "https://github.com/tree-sitter/tree-sitter-php" ;;
        python) echo "https://github.com/tree-sitter/tree-sitter-python" ;;
        rust) echo "https://github.com/tree-sitter/tree-sitter-rust" ;;
        typescript) echo "https://github.com/tree-sitter/tree-sitter-typescript" ;;
    esac
}

# Supported languages
LANGUAGES="c cpp csharp gdscript go java javascript kotlin php python rust typescript"

echo "🔍 Checking grammar versions..."
echo ""

# Check if lockfile exists
if [ ! -f "$LOCKFILE" ]; then
    echo "📝 Creating new lockfile at $LOCKFILE"
    cat > "$LOCKFILE" << 'EOF'
{
  "version": "1.0",
  "description": "Tracks tree-sitter grammar versions to detect updates",
  "grammars": {}
}
EOF
fi

# Temporary file for building new lockfile
TEMP_LOCKFILE=$(mktemp)
cat > "$TEMP_LOCKFILE" << 'EOF'
{
  "version": "1.0",
  "description": "Tracks tree-sitter grammar versions to detect updates",
  "grammars": {
EOF

first_entry=true

# Process each language
for lang in $LANGUAGES; do
    GRAMMAR_DIR="$GRAMMARS_DIR/tree-sitter-$lang"
    REPO_URL=$(get_repo_url "$lang")

    if [ ! -d "$GRAMMAR_DIR" ]; then
        echo "⚠️  $lang: Not installed (run ./setup.sh $lang)"
        continue
    fi

    # Get current commit hash
    cd "$GRAMMAR_DIR"
    COMMIT=$(git rev-parse HEAD)
    COMMIT_SHORT=$(git rev-parse --short HEAD)
    COMMIT_DATE=$(git log -1 --format=%cI)

    # Try to detect ABI version from Cargo.toml or package.json
    ABI_VERSION="unknown"
    if [ -f "Cargo.toml" ]; then
        # Look for tree-sitter version in Cargo.toml
        TS_VERSION=$(grep -E 'tree-sitter\s*=.*"[0-9]' Cargo.toml | head -1 | grep -oE '[0-9]+\.[0-9]+' | head -1 || echo "")
        if [[ "$TS_VERSION" =~ ^0\.2[2-9]|0\.[3-9][0-9] ]]; then
            ABI_VERSION="15"
        elif [[ "$TS_VERSION" =~ ^0\.2[0-1] ]]; then
            ABI_VERSION="14"
        fi
    fi

    # Check if commit changed
    OLD_COMMIT=$(jq -r ".grammars.\"$lang\".commit // \"\"" "$LOCKFILE" 2>/dev/null || echo "")

    if [ "$COMMIT" != "$OLD_COMMIT" ]; then
        if [ -n "$OLD_COMMIT" ]; then
            echo "🔄 $lang: Updated from ${OLD_COMMIT:0:7} to $COMMIT_SHORT"
        else
            echo "✨ $lang: New entry $COMMIT_SHORT"
        fi
    else
        echo "✓ $lang: Up to date ($COMMIT_SHORT)"
    fi

    # Add comma between entries
    if [ "$first_entry" = false ]; then
        echo "," >> "$TEMP_LOCKFILE"
    fi
    first_entry=false

    # Write entry
    cat >> "$TEMP_LOCKFILE" << EOF
    "$lang": {
      "repo": "$REPO_URL",
      "commit": "$COMMIT",
      "updated": "$COMMIT_DATE",
      "abi_version": "$ABI_VERSION"
    }
EOF
done

# Close JSON
cat >> "$TEMP_LOCKFILE" << 'EOF'
  }
}
EOF

# Format JSON properly
if command -v jq &> /dev/null; then
    jq . "$TEMP_LOCKFILE" > "$LOCKFILE"
else
    # If jq not available, use the temp file as-is
    mv "$TEMP_LOCKFILE" "$LOCKFILE"
fi

rm -f "$TEMP_LOCKFILE"

echo ""
echo "✅ Lockfile updated: $LOCKFILE"
echo ""
echo "💡 Tip: Run this script after pulling grammar updates to track changes"
