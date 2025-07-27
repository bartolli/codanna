---
allowed-tools: Bash(git status), Bash(git ls-files), Bash(ls), Bash(pwd), Read, Write, Glob, Grep, LS
description: Generate or update SRC_FILE_MAP.md with concise file descriptions
---

# 📄 Update Source File Map

Generate or update @SRC_FILE_MAP.md with intelligent file descriptions for quick project understanding.

## 📋 Task Workflow

You will analyze the project and create a comprehensive source file map. Follow these steps:

### 1. Check Project Context
- Current directory: !`pwd`
- Git repository check: !`git status`

### 2. List Configuration Files
!`ls *.toml *.json *.yaml *.yml *.lock 2>/dev/null || echo "No config files found"`

### 3. Get Tracked Files (respects .gitignore)
!`git ls-files`

### 4. Analyze and Generate Map

Based on the information above, create or update `SRC_FILE_MAP.md` with:

#### Structure Template:
```markdown
# Source File Map

**Generated**: [current date]
**Project Type**: [auto-detected from config files]
**Primary Language**: [detected from file extensions]

## 📁 Project Structure

### Configuration
- `Cargo.toml` - Rust project manifest with dependencies
- `clippy.toml` - Linting configuration

### Core Source (`src/`)
- `main.rs` - CLI entry point and command routing
- `lib.rs` - Public API exports and library interface
- `error.rs` - Error types and handling
- `config.rs` - Configuration structures and parsing

### Modules
- `types/` - Core data structures
  - `symbol.rs` - Code symbol representations
  - `reference.rs` - Symbol reference tracking
- `parser/` - Language parsing implementations
  - `rust.rs` - Rust-specific parser
  - `traits.rs` - Common parser interface
- `services/` - Business logic layer
  - `index.rs` - Symbol indexing service
  - `search.rs` - Code search functionality

### Tests
- `tests/` - Integration test suite
- `benches/` - Performance benchmarks

## 🔗 Key Relationships

1. **Parser Flow**: `main.rs` → `parser/traits.rs` → `parser/rust.rs` → `types/symbol.rs`
2. **Storage Layer**: `services/index.rs` → `storage/sqlite.rs` → database
3. **API Surface**: External code → `lib.rs` → internal modules

## 📊 Architecture Notes

- Pattern: Modular service architecture
- Entry Points: `main.rs` (CLI), `lib.rs` (library)
- Data Flow: Parse → Extract → Index → Query
```

### 5. Key Analysis Points

When examining files:
- Read file headers for module documentation
- Note main functions and their purpose
- Identify imports to understand dependencies
- Look for trait implementations and key types
- Check for test modules to understand usage

### 6. Output Requirements

The generated map should:
- Be concise but informative (10-15 words per description)
- Show clear hierarchy and relationships
- Help new developers navigate quickly
- Highlight architectural patterns
- Include only tracked files (git ls-files handles .gitignore)

### Arguments
${ARGUMENTS:+Focus on: $ARGUMENTS}

Now analyze the project structure and create/update the @SRC_FILE_MAP.md file.