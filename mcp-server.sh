#!/bin/bash
# MCP Server wrapper for proper argument handling
# This script ensures the MCP inspector can properly launch our server

# Change to the project directory
cd "$(dirname "$0")"

# Run the server
exec cargo run -- serve