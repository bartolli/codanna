#!/bin/bash
set -e

# Parse command line arguments
LANG=$1

GRAMMARS_DIR="$(cd "$(dirname "$0")/../grammars" && pwd)"
mkdir -p "$GRAMMARS_DIR"

# Ensure tree-sitter config points to our grammars directory
TS_CONFIG_DIR="$HOME/.config/tree-sitter"
mkdir -p "$TS_CONFIG_DIR"

cat > "$TS_CONFIG_DIR/config.json" << EOF
{
  "parser-directories": [
    "$GRAMMARS_DIR"
  ]
}
EOF

echo "✅ Tree-sitter configured to use: $GRAMMARS_DIR"

# If specific language requested, only clone that one
if [ -n "$LANG" ]; then
    case "$LANG" in
        typescript) REPO="https://github.com/tree-sitter/tree-sitter-typescript" ;;
        javascript) REPO="https://github.com/tree-sitter/tree-sitter-javascript" ;;
        python) REPO="https://github.com/tree-sitter/tree-sitter-python" ;;
        rust) REPO="https://github.com/tree-sitter/tree-sitter-rust" ;;
        go) REPO="https://github.com/tree-sitter/tree-sitter-go" ;;
        php) REPO="https://github.com/tree-sitter/tree-sitter-php" ;;
        c) REPO="https://github.com/tree-sitter/tree-sitter-c" ;;
        cpp) REPO="https://github.com/tree-sitter/tree-sitter-cpp" ;;
        csharp) REPO="https://github.com/tree-sitter/tree-sitter-c-sharp" ;;
        gdscript) REPO="https://github.com/PrestonKnopp/tree-sitter-gdscript" ;;
        kotlin) REPO="https://github.com/bartolli/tree-sitter-kotlin" ;;
        java) REPO="https://github.com/tree-sitter/tree-sitter-java" ;;
        lua) REPO="https://github.com/tree-sitter-grammars/tree-sitter-lua" ;;
        swift) REPO="https://github.com/alex-pinkus/tree-sitter-swift" ;;
        *)
            echo "❌ Unknown language: $LANG"
            echo "Supported: typescript, javascript, python, rust, go, php, c, cpp, csharp, gdscript, kotlin, java, lua, swift"
            exit 1
            ;;
    esac

    GRAMMAR_NAME="tree-sitter-$LANG"
    dir="$GRAMMARS_DIR/$GRAMMAR_NAME"

    # Determine project root (3 levels up from scripts/)
    PROJECT_ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
    PARSERS_DIR="$PROJECT_ROOT/contributing/parsers"

    if [ -d "$dir" ]; then
        echo "✓ $GRAMMAR_NAME already installed"
    else
        echo "→ Installing $GRAMMAR_NAME..."
        git clone --depth 1 "$REPO" "$dir"
        echo "✅ $GRAMMAR_NAME installed"
    fi

    # Copy node-types.json to parsers directory
    SOURCE_FILE="$dir/src/node-types.json"
    DEST_DIR="$PARSERS_DIR/$LANG"
    DEST_FILE="$DEST_DIR/node-types.json"

    if [ -f "$SOURCE_FILE" ]; then
        mkdir -p "$DEST_DIR"
        cp "$SOURCE_FILE" "$DEST_FILE"
        echo "✅ Copied node-types.json to $DEST_FILE"
    else
        echo "⚠️  Warning: node-types.json not found at $SOURCE_FILE"
        echo "   You may need to generate it with: cd $dir && tree-sitter generate"
    fi

    # Update grammar version lockfile
    echo ""
    LOCK_SCRIPT="$(dirname "$0")/update-grammar-lock.sh"
    if [ -f "$LOCK_SCRIPT" ]; then
        "$LOCK_SCRIPT"
    fi
else
    # List available grammars
    echo ""
    echo "Usage: $0 [language]"
    echo ""
    echo "Examples:"
    echo "  $0 typescript    # Install TypeScript grammar"
    echo "  $0 python        # Install Python grammar"
    echo ""
    echo "Installed grammars:"
    for dir in "$GRAMMARS_DIR"/tree-sitter-*; do
        if [ -d "$dir" ]; then
            basename "$dir"
        fi
    done
    echo ""
    echo "To parse files: tree-sitter parse <file>"
fi