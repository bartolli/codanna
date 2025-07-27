---
allowed-tools: Task
description: Generate comprehensive documentation for a code file using the code-documenter agent
argument-hint: <file-path>
---

# 📚 Document Code File

Use the specialized code-documenter agent to generate comprehensive documentation for the specified file.

## Usage
`/doc-file src/main.rs`

## Task
Please use the code-documenter agent to generate rich documentation for the file: $ARGUMENTS

The documentation should be optimized for code intelligence indexing and include:
- Comprehensive file header
- All public functions/methods
- Structs, enums, and traits
- Examples and cross-references
- Performance characteristics

The agent will analyze the file and add appropriate documentation while preserving the existing code.