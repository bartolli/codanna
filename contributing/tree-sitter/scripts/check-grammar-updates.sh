#!/usr/bin/env bash
set -e

# Determine paths
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
GRAMMARS_DIR="$SCRIPT_DIR/../grammars"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
LOCKFILE="$PROJECT_ROOT/contributing/parsers/grammar-versions.lock"

# Supported languages
LANGUAGES="c cpp csharp gdscript go java javascript kotlin php python rust swift typescript"

echo "🔍 Checking for grammar updates from remote..."
echo ""

if [ ! -f "$LOCKFILE" ]; then
    echo "❌ No lockfile found. Run ./update-grammar-lock.sh first"
    exit 1
fi

updates_available=false

for lang in $LANGUAGES; do
    GRAMMAR_DIR="$GRAMMARS_DIR/tree-sitter-$lang"

    if [ ! -d "$GRAMMAR_DIR" ]; then
        continue
    fi

    # Get local commit from lockfile
    LOCAL_COMMIT=$(jq -r ".grammars.\"$lang\".commit" "$LOCKFILE" 2>/dev/null || echo "")

    if [ -z "$LOCAL_COMMIT" ]; then
        echo "⚠️  $lang: Not in lockfile"
        continue
    fi

    # Fetch latest from remote (without pulling)
    cd "$GRAMMAR_DIR"
    git fetch origin --quiet 2>/dev/null || {
        echo "⚠️  $lang: Failed to fetch from remote"
        continue
    }

    # Get remote commit
    REMOTE_COMMIT=$(git rev-parse origin/HEAD 2>/dev/null || git rev-parse origin/main 2>/dev/null || git rev-parse origin/master 2>/dev/null || echo "")

    if [ -z "$REMOTE_COMMIT" ]; then
        echo "⚠️  $lang: Could not determine remote commit"
        continue
    fi

    LOCAL_SHORT=${LOCAL_COMMIT:0:7}
    REMOTE_SHORT=${REMOTE_COMMIT:0:7}

    if [ "$LOCAL_COMMIT" != "$REMOTE_COMMIT" ]; then
        echo "🔄 $lang: Update available ($LOCAL_SHORT → $REMOTE_SHORT)"

        # Show commit count difference
        BEHIND_COUNT=$(git rev-list --count ${LOCAL_COMMIT}..${REMOTE_COMMIT} 2>/dev/null || echo "?")
        echo "   └─ $BEHIND_COUNT commits behind"

        updates_available=true
    else
        echo "✓ $lang: Up to date ($LOCAL_SHORT)"
    fi
done

echo ""

if [ "$updates_available" = true ]; then
    echo "📦 Updates available! Run these commands to update:"
    echo ""
    echo "  cd contributing/tree-sitter/grammars/tree-sitter-{language}"
    echo "  git pull"
    echo "  cd -"
    echo "  ./contributing/tree-sitter/scripts/update-grammar-lock.sh"
    echo ""
    echo "Or update all at once:"
    echo "  for dir in contributing/tree-sitter/grammars/tree-sitter-*; do"
    echo "    (cd \$dir && git pull)"
    echo "  done"
    echo "  ./contributing/tree-sitter/scripts/update-grammar-lock.sh"
    exit 1
else
    echo "✅ All grammars are up to date!"
    exit 0
fi
