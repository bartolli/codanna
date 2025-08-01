# MCP Tools Guide

This guide outlines how to effectively use the Code Intelligence MCP (Model Context Protocol) tools with Gemini to understand, navigate, and analyze a codebase. These tools provide a powerful way to query the code index and build a comprehensive understanding of the software's structure and relationships.

## Core Philosophy

The tools are designed to be used in a conversational workflow. You start with a broad query and progressively narrow down your focus, using the output of one tool to inform the input for the next. This allows for a natural and efficient exploration of the code.

## Available Tools

Here is a summary of the primary tools available:

*   `get_index_info`: Provides a high-level statistical overview of the indexed codebase (number of files, symbols, relationships, etc.).
*   `search_symbols`: Performs a fuzzy, full-text search for symbols. Ideal for initial exploration when you don't know the exact name of a symbol.
*   `find_symbol`: Locates a specific symbol by its exact name. Use this when you know what you're looking for.
*   `get_calls`: Shows all functions and methods that a specific function calls. Helps understand a function's dependencies.
*   `find_callers`: Shows all functions that call a specific function. Essential for understanding who uses a particular piece of code.
*   `analyze_impact`: Analyzes the "blast radius" of changing a symbol by showing all other symbols that would be directly or indirectly affected.

## Common Workflows

Below are common workflows demonstrating how to combine these tools to solve software engineering tasks.

### Workflow 1: Assessing the Impact of a Change

Imagine you need to modify a function named `handle_request`. Before you start, you want to understand the potential impact of your changes.

1.  **Find the function:** Start by locating the exact symbol to ensure you're analyzing the right one.
    ```bash
    codanna mcp find_symbol --args '{"name": "handle_request"}'
    ```
    *Output will show the file, line number, and signature, confirming you have the correct function.*

2.  **Understand its dependencies:** See what other functions `handle_request` relies on. This helps you understand its internal logic.
    ```bash
    codanna mcp get_calls --args '{"function_name": "handle_request"}'
    ```
    *Output will list all functions called by `handle_request`.*

3.  **Identify its users:** Find out where this function is being used. This is crucial for impact analysis.
    ```bash
    codanna mcp find_callers --args '{"function_name": "handle_request"}'
    ```
    *Output will list all functions that call `handle_request`.*

4.  **Analyze the full impact radius:** Get a comprehensive view of what your change might break downstream. This tool traverses the call graph to find not just direct callers, but callers of callers, and so on.
    ```bash
    codanna mcp analyze_impact --args '{"symbol_name": "handle_request", "max_depth": 3}'
    ```
    *Output will show a list of all symbols that could be affected by a change to `handle_request`, grouped by their kind (Function, Struct, etc.).*

### Workflow 2: Exploring a Feature Area

Let's say you're new to the project and want to understand how "parsing" works.

1.  **Broad search:** Start with a fuzzy search to find relevant symbols.
    ```bash
    codanna mcp search_symbols --args '{"query": "parse", "limit": 10}'
    ```
    *This will return a list of symbols related to parsing, like `JsonParser`, `parse_file`, `parse_error`, etc., along with their file paths and documentation snippets.*

2.  **Drill down:** From the search results, pick an interesting symbol, for example, `parse_file`, and get more details on it.
    ```bash
    codanna mcp find_symbol --args '{"name": "parse_file"}'
    ```

3.  **Navigate the call graph:** Now, explore the code around `parse_file`.
    *   What does it do?
        ```bash
        codanna mcp get_calls --args '{"function_name": "parse_file"}'
        ```
    *   Who uses it?
        ```bash
        codanna mcp find_callers --args '{"function_name": "parse_file"}'
        ```
    *You can repeat this process, using the output of `get_calls` or `find_callers` as the input for your next query to traverse the codebase's dependency graph.*

### Workflow 3: Getting a Project Overview

When you first approach a project, it's helpful to get a sense of its scale and composition.

1.  **Get index statistics:** Use `get_index_info` to see a summary of the codebase.
    ```bash
    codanna mcp get_index_info
    ```
    *This command provides a quick snapshot of the number of files, total symbols, and a breakdown of symbol kinds (functions, structs, traits), giving you an immediate sense of the project's size and complexity.*

## Quick Tool Reference

*   **`get_index_info`**
    *   `args`: None
*   **`search_symbols`**
    *   `args`: `{"query": "search_term", "limit": 10, "kind": "Function", "module": "my_module::utils"}` (limit, kind, module are optional)
*   **`find_symbol`**
    *   `args`: `{"name": "exact_symbol_name"}`
*   **`get_calls`**
    *   `args`: `{"function_name": "exact_function_name"}`
*   **`find_callers`**
    *   `args`: `{"function_name": "exact_function_name"}`
*   **`analyze_impact`**
    *   `args`: `{"symbol_name": "exact_symbol_name", "max_depth": 3}` (max_depth is optional, defaults to 3)
