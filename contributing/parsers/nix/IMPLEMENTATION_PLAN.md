# Nix Language Parser — Implementation Plan

> Status: **planning** · Branch: `feature/parser-for-nix-lang` · Target grammar: `tree-sitter-nix 0.3.0`
>
> This document is the execution plan for adding Nix expression-language support to
> codanna. It follows the conventions in
> [`contributing/development/language-support.md`](../../development/language-support.md)
> and uses **Lua** as the closest reference parser (dynamic, no traits, attrset/scope-centric).

---

## 1. Compatibility verdict

| Item | Value |
|---|---|
| Grammar crate | `tree-sitter-nix = "0.3.0"` (nix-community, Jul 2025) |
| Binding style | modern `LANGUAGE: tree_sitter_language::LanguageFn` (ABI-14/15) |
| Core dep | `tree-sitter-language = "0.1.0"` (no direct `tree-sitter` dep) |
| codanna core | `tree-sitter 0.26.9` — **compatible** |
| Wiring | identical to Lua: `tree_sitter_nix::LANGUAGE.into()` → `parser.set_language(&lang)` |

No version conflict: the grammar exposes the same `LanguageFn` constant codanna already
consumes for Lua/Clojure/etc.

---

## 2. Module + wiring map

Six **new** files in `src/parsing/nix/` plus a small set of **existing** files to edit.
`config.rs` is intentionally NOT edited — `generate_language_defaults()` auto-populates from
the registry.

```mermaid
flowchart LR
    subgraph NEW["NEW — src/parsing/nix/ (6 files)"]
        direction TB
        DEF["definition.rs<br/>LanguageDefinition"]
        PAR["parser.rs<br/>LanguageParser + NodeTracker"]
        BEH["behavior.rs<br/>LanguageBehavior"]
        RES["resolution.rs<br/>ResolutionScope"]
        AUD["audit.rs<br/>ABI-15 coverage"]
        MOD["mod.rs<br/>re-exports + register"]
    end

    subgraph EDIT["EDIT — existing files"]
        direction TB
        CARGO["Cargo.toml<br/>+ tree-sitter-nix = 0.3.0"]
        PMOD["parsing/mod.rs<br/>pub mod nix; pub use ..."]
        REG["parsing/registry.rs<br/>initialize_registry()<br/>+ Deserialize match arm"]
        LANG["parsing/language.rs<br/>enum Language::Nix<br/>+ 6 match arms"]
        TESTS["tests/parsers_tests.rs<br/>gateway #path entries"]
        LOCK["parsers/grammar-versions.lock<br/>nix entry (optional)"]
    end

    subgraph SKIP["NO EDIT NEEDED"]
        CFG["config.rs<br/>auto from registry.iter_all()"]
    end

    MOD --> PMOD
    DEF --> REG
    PAR --> LANG
    DEF -. "tree_sitter_nix::LANGUAGE" .-> CARGO
    AUD --> TESTS
    REG -. registers .-> MOD
    CFG -. reads .-> REG
```

> `language.rs` is **required**, not optional: `Language::from_extension` calls
> `from_language_id("nix")`; without the `Nix` arm it returns `None` and `.nix` files are
> never detected.

---

## 3. Trait architecture

The four traits each new file implements, and the shared types they touch.

```mermaid
classDiagram
    class LanguageDefinition {
        <<trait>>
        +id() LanguageId
        +name() str
        +extensions() slice
        +create_parser(settings) LanguageParser
        +create_behavior() LanguageBehavior
        +default_enabled() bool
    }
    class LanguageParser {
        <<trait>>
        +parse(code, file_id, counter) Vec~Symbol~
        +find_calls(code) Vec
        +find_imports(code, file_id) Vec~Import~
        +extract_doc_comment(node, code) Option
        +language() Language
        +as_any() Any
    }
    class LanguageBehavior {
        <<trait>>
        +module_separator() str
        +parse_visibility(sig) Visibility
        +supports_traits() bool
        +get_language() TsLanguage
        +create_resolution_context(file_id) ResolutionScope
    }
    class ResolutionScope {
        <<trait>>
        +resolve(name) Option~SymbolId~
        +add_symbol(name, id, level)
        +enter_scope(kind)
        +exit_scope()
    }

    class NixLanguage
    class NixParser
    class NixBehavior
    class NixResolutionContext
    class GenericInheritanceResolver

    NixLanguage ..|> LanguageDefinition
    NixParser ..|> LanguageParser
    NixBehavior ..|> LanguageBehavior
    NixResolutionContext ..|> ResolutionScope

    NixLanguage --> NixParser : creates
    NixLanguage --> NixBehavior : creates
    NixBehavior --> NixResolutionContext : creates
    NixBehavior --> GenericInheritanceResolver : no traits, reuse no-op
```

---

## 4. Nix → codanna symbol mapping

Node names use the `_expression` suffix convention of tree-sitter-nix and **must be confirmed
in Phase 0** (AST discovery).

| Nix construct | tree-sitter-nix node (confirm) | SymbolKind | Notes |
|---|---|---|---|
| `.nix` file | root (`source_code`) | Module | file-based path, separator `.` |
| binding whose value is a lambda | `binding` + `function_expression` | **Function** | key heuristic |
| binding with non-lambda value | `binding` | Variable / Constant | literal RHS → Constant |
| returned attrset keys | `attrset_expression` → `binding` | Field | Public |
| `rec { ... }` attrs | `rec_attrset_expression` | Field | self-referential scope |
| lambda params `{ a, b ? d, ... }:` | `formals` / `formal` | Parameter | `@`-pattern binds whole set |
| `inherit a;` / `inherit (src) a;` | `inherit` / `inherit_from` | Variable (+ref to src) | one node → many bindings |
| `import ./x.nix`, `<nixpkgs>` | `apply_expression` + `path_expression` / `spath_expression` | Import | dynamic interpolation = best-effort |
| `f x`, `callPackage ./p.nix {}` | `apply_expression` | call relationship | resolve `function` field |
| `a.b.c` | `select_expression` + `attrpath` | reference | def-vs-ref by position |

**Visibility:** no keywords → `let`/`formals` = `Private`; returned attrset attrs = `Public`.
**Traits/inheritance:** none → `supports_traits() = false`, reuse `GenericInheritanceResolver`.

---

## 5. Parser traversal logic

`extract_symbols_from_node` decision flow (every visited node is registered with `NodeTracker`
to drive the audit report).

```mermaid
flowchart TD
    START(["visit node"]) --> REG["register node in NodeTracker"]
    REG --> K{"node kind?"}
    K -->|binding| B{"value is function_expression?"}
    B -->|yes| FN["emit Function symbol"]
    B -->|no| VAR["emit Variable or Constant"]
    K -->|attrset or rec_attrset| ATTR["enter attrset scope, emit Field per binding"]
    K -->|let_expression| LET["enter let scope, bindings are Private"]
    K -->|function_expression| LAM["enter lambda scope, emit Parameter per formal"]
    K -->|inherit or inherit_from| INH["emit one Variable per name, record ref to source"]
    K -->|apply_expression| APP{"function is import?"}
    APP -->|yes| IMP["record Import"]
    APP -->|no| CALL["record call relationship"]
    K -->|select_expression| SEL["record attrpath reference"]
    K -->|ERROR| ERR["do not skip, recurse into children"]
    K -->|other| OTH["pass through"]
    FN --> CH["recurse children"]
    VAR --> CH
    ATTR --> CH
    LET --> CH
    LAM --> CH
    INH --> CH
    IMP --> CH
    CALL --> CH
    SEL --> CH
    ERR --> CH
    OTH --> CH
    CH --> EXIT["exit scope if entered"]
    EXIT --> DONE(["return"])
```

---

## 6. Scope resolution order

```mermaid
flowchart TD
    Q(["resolve identifier"]) --> L{"in let or formals local scope?"}
    L -->|yes| HIT(["resolved"])
    L -->|no| R{"in enclosing rec attrset?"}
    R -->|yes| HIT
    R -->|no| W{"in a with namespace?"}
    W -->|maybe| WUN["mark UNRESOLVED, with is context-sensitive"]
    W -->|no| T{"file top-level binding?"}
    T -->|yes| HIT
    T -->|no| I{"imported symbol?"}
    I -->|yes| HIT
    I -->|no| MISS(["unresolved"])
    WUN --> MISS
```

> `with expr;` cannot be resolved statically (its bindings depend on a runtime value, and it
> never shadows other bindings). Treat such references as unresolved — a documented limitation,
> same class codanna already tolerates for dynamic dispatch.

---

## 7. Execution phases

```mermaid
flowchart LR
    P0["Phase 0<br/>AST discovery<br/>comprehensive.nix<br/>+ explore test"]
    P1["Phase 1<br/>scaffold + dep<br/>6 files from Lua"]
    P2["Phase 2<br/>parser.rs<br/>symbol extraction"]
    P3["Phase 3<br/>behavior +<br/>resolution"]
    P4["Phase 4<br/>register<br/>mod/registry/language"]
    P5["Phase 5<br/>tests + audit<br/>>70% coverage"]
    P6["Phase 6<br/>e2e verify<br/>clippy/fmt/docs"]
    P0 --> P1 --> P2 --> P3 --> P4 --> P5 --> P6
```

All commands run in the flake (`nix develop -c ...`).

### Phase 0 — AST node discovery (do first; do not guess node names)
- Write `examples/nix/comprehensive.nix` covering: lambdas, `let`, `rec`, `inherit` /
  `inherit (x)`, `with`, `if`, `assert`, attrsets, lists, `import`, `<nixpkgs>`, string
  interpolation, paths, `a.b.c`, `@`-patterns, defaults, `...`.
- Add a throwaway `explore_nix_abi15` test that loads `tree_sitter_nix::LANGUAGE`, parses it,
  and prints `node.kind()` + `kind_id()` (reuse `discover_nodes` from `lua/audit.rs`).
  `nix develop -c cargo test explore_nix_abi15 -- --nocapture`
- Record findings → `contributing/parsers/nix/NODE_MAPPING.md` + `node-types.json`.
- Alternative: add `pkgs.tree-sitter` + `pkgs.nodejs` to the flake devShell and use
  `contributing/tree-sitter/scripts/`.

### Phase 1 — Scaffold + dependency
- `Cargo.toml` += `tree-sitter-nix = "0.3.0"` (after the `tree-sitter-clojure-orchard` line).
- `mkdir src/parsing/nix`, copy the six `lua/*.rs` files as skeletons, rename
  `Lua`→`Nix`, `tree_sitter_lua`→`tree_sitter_nix`.
- Implement order: `definition.rs` → `parser.rs` → `behavior.rs` → `resolution.rs` →
  `audit.rs` → `mod.rs`.

### Phase 2 — parser.rs
- `extract_symbols_from_node` per the traversal diagram. Minimum methods: `parse`,
  `find_calls`, `find_imports`, `extract_doc_comment`, `as_any`, `language()` → `Language::Nix`.
- Register every node in `NodeTracker`. Handle `ERROR` by recursing. Zero-copy slices.

### Phase 3 — behavior.rs + resolution.rs
- behavior: `module_separator() = "."`, file-based `module_path_from_file`,
  `parse_visibility` (Public default), `supports_traits() = false`,
  `get_language()` = `tree_sitter_nix::LANGUAGE.into()`.
- resolution: `NixResolutionContext` with the scope order above; `with` → unresolved.

### Phase 4 — registration
- `src/parsing/mod.rs`: `pub mod nix;` + `pub use nix::{NixBehavior, NixParser};`
- `src/parsing/registry.rs`: `super::nix::register(registry);` in `initialize_registry()`,
  and `"nix" => "nix",` in the `Deserialize` match.
- `src/parsing/language.rs`: `Nix` variant + arms in `to_language_id`, `from_language_id`,
  `from_extension` fallback, `extensions`, `config_key`, `name` (extension `"nix"`).
- Decide `default_enabled()` — **recommend `false` during dev, flip to `true` at release**.

### Phase 5 — tests + audit
- `audit.rs` (copy Lua, swap key-node list).
- `tests/parsers/nix/` + gateway `#[path = "parsers/nix/..."]` in `tests/parsers_tests.rs`.
- `nix develop -c cargo test nix`
- `nix develop -c cargo test audit_nix -- --nocapture`  (target >70% key-node coverage)

### Phase 6 — end-to-end verify + polish
```
nix develop -c cargo build
nix develop -c cargo clippy --fix
nix develop -c cargo fmt
nix develop -c bash -c 'cargo run -- init && cargo run -- index . && cargo run -- retrieve search "mkDerivation"'
```
- Update supported-list in `language-support.md` and add the `nix` entry to
  `grammar-versions.lock`.

---

## 8. Risks & open decisions

| Item | Disposition |
|---|---|
| `with expr;` scoping | statically unresolvable → documented limitation, do not block |
| string/path interpolation `${...}` | extract inner identifier as ref; dynamic target best-effort |
| `attrpath` def vs ref | disambiguate by binding (LHS) vs expression (RHS) position |
| flake lacks `tree-sitter` CLI | use Rust exploration test (Phase 0), or add `tree-sitter`+`nodejs` to flake |
| `default_enabled` true/false at merge | **open** — recommend `false` until audit coverage is solid |

---

## 9. Touch-point checklist

- [ ] `Cargo.toml` — `tree-sitter-nix = "0.3.0"`
- [ ] `src/parsing/nix/{mod,definition,parser,behavior,resolution,audit}.rs`
- [ ] `src/parsing/mod.rs` — module + re-export
- [ ] `src/parsing/registry.rs` — `initialize_registry()` + `Deserialize` arm
- [ ] `src/parsing/language.rs` — `Language::Nix` + 6 match arms
- [ ] `examples/nix/comprehensive.nix` (+ `main.nix`)
- [ ] `tests/parsers/nix/` + `tests/parsers_tests.rs` gateway
- [ ] `contributing/parsers/nix/NODE_MAPPING.md` + `node-types.json`
- [ ] `contributing/parsers/grammar-versions.lock` — nix entry
- [ ] `config.rs` — **no edit** (auto from registry)
