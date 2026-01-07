# Quick Start

Get Codanna running in 5 minutes.

## Install

```bash
cargo install codanna --all-features
```

## Setup

Initialize Codanna in your project:

```bash
codanna init
```

This creates `.codanna/settings.toml` with default configuration.

## Index Your Code

Build a searchable index from your codebase:

```bash
# See what would be indexed (dry run, optional)
codanna index src --dry-run

# Index your code
codanna index src 
```

## Ask Real Questions

```bash
# Semantic search - finds functions with auth-related doc comments
codanna mcp semantic_search_docs query:"where do we resolve symbol references" limit:3
```

## How Accurate and Fast is Codanna?

Try it yourself:

```bash
# Run with `time` like this
time codanna mcp semantic_search_docs query:"where do we resolve symbol references" limit:3
```

Output 3 results in **0.16s**

```text
Found 3 semantically similar result(s) for 'where do we resolve symbol references':

1. resolve_symbol (Method) - Similarity: 0.592
   File: src/parsing/language_behavior.rs:252
   Doc: Resolve a symbol using language-specific resolution rules  Default implementation delegates to the resolution context.
   Signature: fn resolve_symbol(
        &self,
        name: &str,
        context: &dyn ResolutionScope,
        _document_index: &DocumentIndex,
    ) -> Option<SymbolId>

2. resolve_symbol (Method) - Similarity: 0.577
   File: src/indexing/resolver.rs:107
   Doc: Resolve a symbol reference to its actual definition  Given a symbol name used in a file, this tries to resolve it to the actual...
   Signature: pub fn resolve_symbol<F>(
        &self,
        name: &str,
        from_file: FileId,
        document_index: &DocumentIndex,
        get_behavior: F,
    ) -> Option<SymbolId>
    where
        F: Fn(LanguageId) -> Box<dyn crate::parsing::LanguageBehavior>,

3. is_resolvable_symbol (Method) - Similarity: 0.532
   File: src/parsing/language_behavior.rs:412
   Doc: Check if a symbol should be resolvable (added to resolution context)  Languages override this to filter which symbols are available for resolution....
   Signature: fn is_resolvable_symbol(&self, symbol: &Symbol) -> bool

codanna mcp semantic_search_docs query:"where do we resolve symbol references  0.16s user 0.05s system 177% cpu 0.120 total
```

## Next Steps

- Set up [integrations](../integrations/) with your AI assistant
- Learn more [CLI commands](../user-guide/cli-reference.md)
- Configure [settings](../user-guide/configuration.md) for your project