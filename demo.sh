#!/bin/bash

# Codebase Intelligence Demo Script
# This script demonstrates all the features of our code intelligence POC

echo "==================================="
echo "Codebase Intelligence System Demo"
echo "==================================="
echo

# Build the project first
echo "Building the project..."
cargo build --release 2>/dev/null || {
    echo "Error: Failed to build the project"
    exit 1
}

BINARY="./target/release/codebase-intelligence"

# Initialize configuration if needed
if [ ! -f ".code-intelligence/settings.toml" ]; then
    echo "Initializing configuration..."
    $BINARY init
    echo
fi

# Index the demo file
echo "1. Indexing demo/example.rs..."
echo "   Command: $BINARY index demo/example.rs"
echo
$BINARY index demo/example.rs
echo

# Demonstrate symbol search
echo "2. Finding symbols by name..."
echo "   Command: $BINARY retrieve symbol Calculator"
echo
$BINARY retrieve symbol Calculator
echo

# Show function calls
echo "3. Analyzing function call relationships..."
echo "   Command: $BINARY retrieve calls calculate"
echo
$BINARY retrieve calls calculate
echo

echo "   Command: $BINARY retrieve callers format_result"
echo
$BINARY retrieve callers format_result
echo

# Show trait implementations
echo "4. Finding trait implementations..."
echo "   Command: $BINARY retrieve implementations Operation"
echo
$BINARY retrieve implementations Operation
echo

# Show type usage
echo "5. Analyzing type usage..."
echo "   Command: $BINARY retrieve uses Calculator"
echo
$BINARY retrieve uses Calculator
echo

echo "   Command: $BINARY retrieve uses Report"
echo
$BINARY retrieve uses Report
echo

# Show what methods are defined
echo "6. Finding method definitions..."
echo "   Command: $BINARY retrieve defines Calculator"
echo
$BINARY retrieve defines Calculator
echo

echo "   Command: $BINARY retrieve defines Operation"
echo
$BINARY retrieve defines Operation
echo

# Show impact analysis
echo "7. Impact analysis..."
echo "   Command: $BINARY retrieve impact format_result"
echo
$BINARY retrieve impact format_result
echo

echo "   Command: $BINARY retrieve impact Operation --depth 3"
echo
$BINARY retrieve impact Operation --depth 3
echo

# Show comprehensive dependency analysis
echo "8. Comprehensive dependency analysis..."
echo "   Command: $BINARY retrieve dependencies calculate"
echo
$BINARY retrieve dependencies calculate
echo

echo "   Command: $BINARY retrieve dependencies Calculator"
echo
$BINARY retrieve dependencies Calculator
echo

echo "==================================="
echo "Demo Complete!"
echo "==================================="
echo
echo "This POC demonstrates:"
echo "- Symbol extraction (functions, methods, structs, traits)"
echo "- Call graph analysis (who calls whom)"
echo "- Trait implementation tracking"
echo "- Type usage analysis"
echo "- Method definition tracking"
echo "- Impact radius calculation"
echo "- Comprehensive dependency analysis"
echo
echo "Next steps for a full implementation:"
echo "- Multi-file indexing with directory walking"
echo "- Persistent storage (SQLite/memory-mapped files)"
echo "- Cross-file relationship resolution"
echo "- MCP server implementation"
echo "- Support for multiple languages"
echo "- Performance optimizations (parallel processing)"