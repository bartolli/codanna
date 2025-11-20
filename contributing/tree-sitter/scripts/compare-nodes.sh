#!/bin/bash
# Compare tree-sitter AST nodes with our parser
# Usage:
#   compare-nodes.sh <language>     - Uses comprehensive.* file, runs audit
#   compare-nodes.sh <file>          - Compares specific file, saves to log

# Check if tree-sitter CLI is installed
if ! command -v tree-sitter &> /dev/null; then
    echo "Error: tree-sitter CLI not found"
    echo "Install with: cargo install tree-sitter-cli --locked"
    exit 1
fi

INPUT=$1

if [ -z "$INPUT" ]; then
    echo "Usage:"
    echo "  $0 <language>       # Compare using comprehensive.* (with audit)"
    echo "  $0 <file>           # Compare specific file (output to .log)"
    echo ""
    echo "Languages: typescript, python, rust, go, php, c, cpp, csharp, gdscript, java, kotlin"
    exit 1
fi

# Check if input is a file or language name
if [ -f "$INPUT" ]; then
    # Mode 1: Specific file comparison
    FILE_PATH="$INPUT"
    FILE_NAME=$(basename "$FILE_PATH")
    FILE_BASE="${FILE_NAME%.*}"
    LOG_FILE="${FILE_BASE}_comparison.log"

    echo "Comparing: $FILE_PATH"
    echo "Output will be saved to: $LOG_FILE"
    echo ""

    # Parse with tree-sitter
    echo "Parsing with tree-sitter..." > "$LOG_FILE"
    echo "File: $FILE_PATH" >> "$LOG_FILE"
    echo "Generated: $(date -u '+%Y-%m-%d %H:%M:%S UTC')" >> "$LOG_FILE"
    echo "========================================" >> "$LOG_FILE"
    echo "" >> "$LOG_FILE"

    # Extract named nodes only (tree-sitter CLI shows only named nodes by default)
    # Filter empty lines that sometimes appear in tree-sitter output
    if ! tree-sitter parse "$FILE_PATH" 2>&1 | \
        grep -o '([a-z_]*' | sed 's/(//' | grep -v '^$' | sort -u > /tmp/cli-nodes.txt; then
        echo "❌ Tree-sitter failed to parse $FILE_PATH" | tee -a "$LOG_FILE"
        echo "Check if the appropriate grammar is installed." | tee -a "$LOG_FILE"
        exit 1
    fi

    # Parse with codanna
    echo ""
    echo "Parsing with codanna..."

    # Capture both output and exit code
    # Note: Using default behavior (named nodes only) to match tree-sitter
    # Add --all-nodes flag if you want to see all nodes including punctuation
    CODANNA_OUTPUT=$(cargo run --quiet -- parse "$FILE_PATH" 2>&1)
    CODANNA_EXIT=$?

    if [ $CODANNA_EXIT -eq 0 ]; then
        # Success - extract nodes
        echo "$CODANNA_OUTPUT" | jq -r .node | sort -u > /tmp/codanna-nodes.txt
        echo "✅ Codanna parsed successfully"
        echo "" >> "$LOG_FILE"
        echo "=== Comparison Results ===" >> "$LOG_FILE"
        echo "" >> "$LOG_FILE"
    else
        # Handle specific error codes
        case $CODANNA_EXIT in
            3)
                echo "⚠️ File not found by codanna (exit code 3)"
                ;;
            4)
                echo "⚠️ Parse error in codanna (exit code 4)"
                ;;
            8)
                echo "⚠️ Language not supported by codanna (exit code 8)"
                ;;
            *)
                echo "❌ Codanna failed with exit code $CODANNA_EXIT"
                ;;
        esac

        # Show the actual error
        echo "Error details: $CODANNA_OUTPUT" | head -2

        echo "Falling back to tree-sitter-only mode."
        echo "" >> "$LOG_FILE"
        echo "NOTE: Codanna parse failed (exit code $CODANNA_EXIT). Showing tree-sitter AST only." >> "$LOG_FILE"
        echo "Error: $CODANNA_OUTPUT" >> "$LOG_FILE"
        echo "" >> "$LOG_FILE"
    fi

    # Check if codanna parse succeeded
    if [ -f /tmp/codanna-nodes.txt ] && [ -s /tmp/codanna-nodes.txt ]; then
        # We have both - do comparison
        echo "=== Nodes in tree-sitter but not in codanna ===" >> "$LOG_FILE"
        comm -23 /tmp/cli-nodes.txt /tmp/codanna-nodes.txt >> "$LOG_FILE"

        echo "" >> "$LOG_FILE"
        echo "=== Nodes in codanna but not in tree-sitter ===" >> "$LOG_FILE"
        comm -13 /tmp/cli-nodes.txt /tmp/codanna-nodes.txt >> "$LOG_FILE"

        echo "" >> "$LOG_FILE"
        echo "=== Tree-sitter nodes ===" >> "$LOG_FILE"
        cat /tmp/cli-nodes.txt >> "$LOG_FILE"

        echo "" >> "$LOG_FILE"
        echo "=== Codanna nodes ===" >> "$LOG_FILE"
        cat /tmp/codanna-nodes.txt >> "$LOG_FILE"

        # Summary with comparison
        TS_COUNT=$(wc -l < /tmp/cli-nodes.txt | tr -d ' ')
        CODANNA_COUNT=$(wc -l < /tmp/codanna-nodes.txt | tr -d ' ')
        COMMON_COUNT=$(comm -12 /tmp/cli-nodes.txt /tmp/codanna-nodes.txt | wc -l | tr -d ' ')

        echo "" >> "$LOG_FILE"
        echo "=== Summary ===" >> "$LOG_FILE"
        echo "Tree-sitter nodes: $TS_COUNT" >> "$LOG_FILE"
        echo "Codanna nodes: $CODANNA_COUNT" >> "$LOG_FILE"
        echo "Common nodes: $COMMON_COUNT" >> "$LOG_FILE"

        echo "✅ Comparison saved to: $LOG_FILE"
        echo ""
        echo "Tree-sitter: $TS_COUNT nodes, Codanna: $CODANNA_COUNT nodes, Common: $COMMON_COUNT"
        echo "Full comparison saved to: $LOG_FILE"
    else
        # Tree-sitter only mode
        echo "=== Tree-sitter AST nodes found ===" >> "$LOG_FILE"
        cat /tmp/cli-nodes.txt >> "$LOG_FILE"

        # Summary
        TS_COUNT=$(wc -l < /tmp/cli-nodes.txt | tr -d ' ')

        echo "" >> "$LOG_FILE"
        echo "=== Summary ===" >> "$LOG_FILE"
        echo "Total unique nodes: $TS_COUNT" >> "$LOG_FILE"

        echo "✅ AST exploration saved to: $LOG_FILE"
        echo ""
        echo "Found $TS_COUNT unique node types in the AST."
        echo "Full AST structure saved to: $LOG_FILE"
    fi

else
    # Mode 2: Language comprehensive comparison (existing logic)
    LANG="$INPUT"

    # Map language to expected file extension
    case "$LANG" in
        typescript) EXT="ts" ;;
        python) EXT="py" ;;
        rust) EXT="rs" ;;
        go) EXT="go" ;;
        php) EXT="php" ;;
        c) EXT="c" ;;
        cpp) EXT="cpp" ;;
        csharp) EXT="cs" ;;
        gdscript) EXT="gd" ;;
        java) EXT="java" ;;
        kotlin) EXT="kt" ;;
        *)
            echo "❌ Unsupported language: $LANG"
            echo "Supported: typescript, python, rust, go, php, c, cpp, csharp, gdscript, java, kotlin"
            exit 1
            ;;
    esac

    EXAMPLE_FILE="examples/$LANG/comprehensive.$EXT"
    if [ ! -f "$EXAMPLE_FILE" ]; then
        echo "❌ Required file not found: $EXAMPLE_FILE"
        echo "This mode requires comprehensive examples for the audit system."
        exit 1
    fi

    # Parse with tree-sitter and extract node names
    if ! tree-sitter parse "$EXAMPLE_FILE" 2>&1 | \
        grep -o '([a-z_]*' | sort -u | sed 's/(//' > /tmp/cli-nodes.txt; then
        echo "❌ Tree-sitter failed to parse $EXAMPLE_FILE"
        echo ""
        echo "Possible causes:"
        echo "1. Grammar not installed - run: ./contributing/tree-sitter/scripts/setup.sh $LANG"
        echo "2. File has syntax errors"
        echo "3. Tree-sitter CLI not installed - run: cargo install tree-sitter-cli --locked"
        exit 1
    fi

    if [ ! -s /tmp/cli-nodes.txt ]; then
        echo "❌ No nodes found in tree-sitter output"
        echo ""
        echo "Try running directly to see the error:"
        echo "  tree-sitter parse $EXAMPLE_FILE"
        exit 1
    fi

    # Get nodes from our parser (this triggers audit report generation)
    cargo test comprehensive_${LANG}_analysis 2>&1 | \
        grep "✓" | awk '{print $2}' | sort -u > /tmp/codanna-nodes.txt

    echo "=== Nodes in CLI but not in Codanna ==="
    comm -23 /tmp/cli-nodes.txt /tmp/codanna-nodes.txt

    echo ""
    echo "=== Nodes in Codanna but not in CLI ==="
    comm -13 /tmp/cli-nodes.txt /tmp/codanna-nodes.txt
fi