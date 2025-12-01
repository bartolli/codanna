# Tree-sitter CLI Local Development

This directory provides tools for exploring AST structures using the official tree-sitter CLI, helpful for debugging parser implementations.

## Quick Start

1. **Install a grammar** (one-time setup per language):
   ```bash
   ./contributing/tree-sitter/scripts/setup.sh typescript
   ```

2. **Parse files and explore AST**:
   ```bash
   # Use tree-sitter directly (works from any directory)
   tree-sitter parse examples/typescript/comprehensive.ts

   # Or use our helper script
   ./contributing/tree-sitter/scripts/explore-ast.sh examples/typescript/comprehensive.ts
   ```

3. **Compare with our parser**:
   ```bash
   # From project root
   ./contributing/tree-sitter/scripts/compare-nodes.sh typescript
   ```

## Setup Script

The setup script configures tree-sitter and installs grammars on-demand:

```bash
# Show installed grammars
./contributing/tree-sitter/scripts/setup.sh

# Install specific language grammar
./contributing/tree-sitter/scripts/setup.sh python
./contributing/tree-sitter/scripts/setup.sh rust
./contributing/tree-sitter/scripts/setup.sh go
```

Supported languages: typescript, javascript, python, rust, go, php, c, cpp, csharp, java, kotlin, swift, gdscript

## Available Scripts

All scripts are located in `contributing/tree-sitter/scripts/`:

| Script | Purpose | Input | Example |
|--------|---------|-------|---------|
| `setup.sh` | Configure tree-sitter and install grammars | Language name | `./scripts/setup.sh typescript` |
| `update-grammar-lock.sh` | Generate/update grammar version lockfile | None | `./scripts/update-grammar-lock.sh` |
| `check-grammar-updates.sh` | Check for remote grammar updates | None | `./scripts/check-grammar-updates.sh` |
| `explore-ast.sh` | Parse ANY file and display its AST | File path + mode | `./scripts/explore-ast.sh file.ts both` |
| `compare-nodes.sh` | Compare codanna with tree-sitter | Language or file path | See below |

### explore-ast.sh
Parse files with codanna and/or tree-sitter:
```bash
# Default: Use codanna parse (named nodes only)
./contributing/tree-sitter/scripts/explore-ast.sh examples/rust/main.rs

# Use tree-sitter
./contributing/tree-sitter/scripts/explore-ast.sh examples/rust/main.rs tree-sitter

# Compare both parsers
./contributing/tree-sitter/scripts/explore-ast.sh examples/rust/main.rs both
```

### compare-nodes.sh
**Two modes:**
- **Language mode**: `./contributing/tree-sitter/scripts/compare-nodes.sh typescript`
  - Compares comprehensive.* files with our parser
  - Triggers audit report generation
  - Shows differences between parsers

- **File mode**: `./contributing/tree-sitter/scripts/compare-nodes.sh path/to/file.ts`
  - Compares AST nodes between codanna and tree-sitter
  - Saves detailed output to `{filename}_comparison.log`
  - Shows matching statistics and differences

## How It Works

1. Tree-sitter config is saved to `~/.config/tree-sitter/config.json`
2. Grammars are cloned to `contributing/tree-sitter/grammars/`
3. Tree-sitter automatically finds grammars based on the config
4. File extensions determine which grammar to use (.ts → typescript, .py → python)

## Grammar Version Tracking

The project tracks tree-sitter grammar versions using a lockfile system to detect updates:

### Lockfile System

**Location**: `contributing/parsers/grammar-versions.lock`

This JSON file tracks:
- Commit hash of each grammar
- Last update timestamp
- ABI version (14 or 15)
- Repository URL

### Checking for Updates

```bash
# Check if remote grammars have updates
./contributing/tree-sitter/scripts/check-grammar-updates.sh
```

This fetches from remote and compares without pulling.

Output:
```
✓ c: Up to date (d8d0503)
🔄 python: Update available (293fdc0 → a1b2c3d)
   └─ 5 commits behind
```

### Updating Grammars

When updates are available:

```bash
# Update all grammars
for dir in contributing/tree-sitter/grammars/tree-sitter-*; do
  (cd $dir && git pull)
done

# Update lockfile
./contributing/tree-sitter/scripts/update-grammar-lock.sh
```

The lockfile automatically updates when running `setup.sh` too.

### Why This Matters

- **ABI version changes**: Some grammars may upgrade from ABI-14 to ABI-15
- **Breaking changes**: Grammar updates can change node types
- **node-types.json updates**: New node types may be added
- **Cargo.toml sync**: tree-sitter crate versions need to match

## Notes

- Grammars are cloned with `--depth 1` for speed
- The grammars directory is gitignored
- Each developer's tree-sitter config points to their local grammar directory
- No environment variables or .env files needed - tree-sitter handles it
- Lockfile automatically syncs with grammar commits
