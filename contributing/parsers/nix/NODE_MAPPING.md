# Nix AST Node Mapping

Discovered from `tree-sitter-nix = "0.3.0"` via `explore_nix_abi15` test (Phase 0).
63 total grammar nodes.

## Root

| tree-sitter node | ID | codanna handling |
|---|---|---|
| `source_code` | 62 | root; has single `expression:` field |

## Bindings

| tree-sitter node | ID | field names | codanna symbol |
|---|---|---|---|
| `binding_set` | 91 | `binding:` (multiple) | container; recurse |
| `binding` | 92 | `attrpath:`, `expression:` | Function / Variable / Constant depending on RHS |
| `attrpath` | 95 | `attr:` (identifier) | key of binding |

## Attrsets

| tree-sitter node | ID | codanna handling |
|---|---|---|
| `attrset_expression` | 86 | enter Class scope, recurse |
| `rec_attrset_expression` | 88 | enter Class scope (self-referential), recurse |

## Let

| tree-sitter node | ID | codanna handling |
|---|---|---|
| `let_expression` | 73 | enter Block scope; bindings are Private |

## Functions / Lambdas

| tree-sitter node | ID | field names | codanna symbol |
|---|---|---|---|
| `function_expression` | 68 | `universal:` (simple `x:`) OR `formals:` + `body:` | enter Function scope |
| `formals` | 69 | `formal:` (multiple) | iterate for parameters |
| `formal` | 70 | `name:` (identifier), `default:` (optional expr) | Parameter |

> **`universal`** is the field name for a simple single-identifier lambda parameter (`x: body`).
> **`formals`** is the field name for destructuring pattern (`{ a, b ? 1, ... }:`).
> The `@`-pattern sibling identifier appears at the `function_expression` level as an unnamed child.

## Inherit

| tree-sitter node | ID | codanna symbol |
|---|---|---|
| `inherit` | 50 | Variable per name in `inherited_attrs` |
| `inherit_from` | 94 | Variable per name in `inherited_attrs` (source in parentheses) |
| `inherited_attrs` | 96 | container for the names |

## Control flow / other expressions

| tree-sitter node | ID | codanna handling |
|---|---|---|
| `apply_expression` | 81 | `function:` + `argument:` — recurse; detect `import` calls |
| `select_expression` | 83 | `expression:` + `index:` — recurse |
| `if_expression` | 75 | recurse |
| `assert_expression` | 71 | recurse |
| `with_expression` | 72 | recurse (bindings statically unresolvable) |
| `let_expression` | 73 | enter Block scope |
| `binary_expression` | 79 | recurse |
| `parenthesized_expression` | 85 | recurse |
| `list_expression` | 99 | recurse |

## Literals / leaf nodes

| tree-sitter node | ID | notes |
|---|---|---|
| `identifier` | 2 | bare name (used inside attrpath, formal, etc.) |
| `variable_expression` | 64 | wraps `identifier` in expression position; has `name:` field |
| `integer_expression` | 3 | literal integer |
| `string_expression` | 89 | `"..."` string |
| `indented_string_expression` | 90 | `''...''` multiline string |
| `path_expression` | 65 | `/absolute/path` |
| `path_fragment` | 59 | segment inside a path |
| `spath_expression` | 6 | `<nixpkgs>` angle-bracket path |
| `interpolation` | 98 | `${...}` inside strings |
| `string_fragment` | 56 | plain text inside a string |
| `comment` | 55 | `# ...` comment |
| `ellipses` | 14 | `...` in formals |

## Known limitations

- `with expr;` bindings are statically unresolvable; references inside `with_expression` are left unresolved.
- String interpolation `${...}` paths in imports are treated as best-effort (raw text recorded).
- Complex `attrpath` bindings like `a.b.c = ...` are not emitted as symbols (skipped).
