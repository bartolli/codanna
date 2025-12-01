# Language Support

Parser architecture and supported languages.

## Supported Languages

| Language | Parser |
|----------|--------|
| Rust | tree-sitter-rust |
| Python | tree-sitter-python |
| TypeScript | tree-sitter-typescript |
| JavaScript | tree-sitter-javascript |
| Java | tree-sitter-java |
| Kotlin | tree-sitter-kotlin-codanna |
| Go | tree-sitter-go |
| PHP | tree-sitter-php |
| C | tree-sitter-c |
| C++ | tree-sitter-cpp |
| C# | tree-sitter-c-sharp |
| Swift | tree-sitter-swift |
| GDScript | tree-sitter-gdscript |

## Parser Technology

Codanna uses tree-sitter for AST parsing - the same technology used by GitHub's code navigator.

### Why tree-sitter?

- Language-agnostic
- Fast incremental parsing
- Error-tolerant
- Battle-tested
- Active ecosystem

## What Gets Extracted

From each supported language:

- Functions and methods
- Classes, structs, traits
- Type definitions
- Imports and includes
- Call relationships
- Type relationships
- Documentation comments

## Performance

See [Performance Documentation](../advanced/performance.md) for current benchmarks.

## Adding New Languages

For detailed guidance on adding language support, see the contributing documentation in the repository.

## See Also

- [How It Works](how-it-works.md) - Overall architecture
- [Performance](../advanced/performance.md) - Parser benchmarks
- [Contributing](../contributing/) - Development guidelines