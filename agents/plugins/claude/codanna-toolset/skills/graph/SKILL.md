---
name: graph
description: Whole-codebase structural map of the codanna index as one self-contained HTML disc. Wedges per top-level module, concentric rings with hubs at the centre, hover or click a symbol to light its edge web, search, hide or highlight modules, scrub the timeline by file first-commit date, heatmap of symbols added per day. Use when the question is about the shape of the whole codebase or a module family (what is central, which modules talk, where the hubs sit), not one symbol's neighbourhood. Needs codanna >= 0.14 (`codanna dump`).
allowed-tools: Bash(codanna:*), Bash(node:*), Read, Grep, Glob
---

## What it draws

One HTML file, no server: the codanna index rendered as a pie chart of symbols. Each
top-level module owns a wedge sized by its share; inside a wedge symbols fill rings from
the centre outwards, best-connected first, so hubs sit near the middle and leaves on the
rim. Unlinked symbols form their own inner group. Above the disc a heatmap shows symbols
added per day (file first-commit date from git). Hover or click a symbol to see its edge
web and the connected symbols; the legend hides (eye) or highlights (label) modules and
submodules; the timeline replays the codebase growing oldest-first.

## Run

```bash
node ${CLAUDE_SKILL_DIR}/graph.mjs                      # whole index, Calls edges, default kinds
node ${CLAUDE_SKILL_DIR}/graph.mjs --root crate::indexing   # one module family
node ${CLAUDE_SKILL_DIR}/graph.mjs --relation calls,uses,implements,extends
node ${CLAUDE_SKILL_DIR}/graph.mjs --kinds function,method,struct,trait --dates none --light
```

- Runs `codanna dump` in the project (`--from graph.jsonl` reuses a saved dump, `--binary PATH` picks the binary)
- Output: `.codanna/visualizations/graph-disc-<timestamp>.html`, opened in the browser (`--no-open` to skip, `--out FILE` to place it)
- `--group <wedge>[/<tint>]` -- wedge axis `module` (default), `language`, or `kind`; tint axis `module` (default), `kind`, or `visibility`. The wedge is the angle (share, neighbours adjacent); the tint is the legend tree inside it (three biggest get their own shade, the rest pool). `module/kind` keeps the structure and shows what each module is made of; `language/module` for polyglot indexes; `module/visibility` for API surface per module; `kind/module` is a census (edges cross everywhere)
- `--root PREFIX` scopes to a module prefix (`crate::` is dropped, `.`/`::`/`/` all separate segments); applies to the module segments whatever the wedge axis
- `--kinds` defaults to Function, Method, Struct, Class, Trait, Enum, Interface, TypeAlias, Constant, Macro
- `--relation` defaults to `calls`; any of calls, uses, implements, extends, defines, comma-separated
- `--dates git|none` -- git first-commit date per file drives the timeline and heatmap; `none` leaves both empty
- `--unlinked include|drop` -- `drop` removes symbols with no edge over the chosen relations at build time (smaller file, smaller disc); in the page, the legend eye on `(unlinked)` hides the same set with the cascade animation
- `--palette auto|fixed|generated` -- the template has ten documented hue slots and falls back to greys past them; `auto` (default) generates a palette sized to the group count in the same colour family when there are more than ten groups, `fixed` keeps the upstream ten, `generated` forces one
- `--name NAME` sets the title (default: project directory name); `--light` builds the light theme

## Scoping and filters: what to apply when

Start whole-index once to see module shares and hubs, then scope. The disc is smooth to a
few thousand symbols; hover and filters fall to ~10 fps near 10,000 (the CLI says so
above 4,000). Measured on the codanna self index (14,003 symbols):

| Situation | Apply | Effect on self index |
|---|---|---|
| First look, whole codebase | defaults | 9,983 symbols, 8,248 calls edges, 51% unlinked |
| The hub hole dominates (`(unlinked)` is the biggest group) | `--unlinked drop` | 4,890 symbols, same 8,248 edges; or hide it in-page with its legend eye |
| Many constants/aliases, few calls | `--kinds function,method,struct,class,trait,enum,interface` | 8,453 symbols, unlinked 42% |
| Type-level structure matters (impls, inheritance, field types) | `--relation calls,uses,implements,extends` | 11,003 edges, unlinked 41%; both together: 10,305 edges, 32% |
| Work inside one module family | `--root crate::indexing` (or `src/...` for path-derived languages) | 701 symbols, 6 groups, smooth hover |
| "What is each module made of" (trait-heavy vs free functions) | `--group module/kind` | same wedges, tints = kinds |
| Polyglot repo / monorepo, languages first | `--group language/module` | 15 language wedges on self, modules as tints |
| Public surface vs internals per module | `--group module/visibility` | tints = public / private / crate... |
| Census by symbol kind | `--group kind/module` | wedges = kinds; every call crosses the disc, read the counts |
| Grey/white wedges (more than ten top-level groups) | default `--palette auto`; `--palette fixed` to compare with the upstream ten | 31 groups get 31 hues, golden-angle spaced so neighbours differ |
| Heatmap/timeline empty or misleading (shallow clone, vendored code) | `--dates none` | both panels blank, disc unchanged |
| Re-render without re-reading the index | `--from graph.jsonl` (from `codanna dump > graph.jsonl`) | same data, no dump run |

`defines` as a relation is containment; the legend tree already carries it, so add it
only when you want container->member lines on the disc. `--root` matches module
segments, so `crate::parsing::rust`, `parsing.rust` and `parsing/rust` are the same
prefix; a symbol with no `module_path` falls back to its file path segments.

## Reading it

- Wedge angle = module share of the drawn symbols; a wedge reaching the rim with few rings is sparse, many tight rings is dense
- Centre of a wedge = its hubs (highest degree over the chosen relations); the rim = leaves
- Hover: the blue web is the symbol's edges; the detail panel (click) shows the highlighted signature, its `file:start-end`, and the connected symbols grouped by relation and direction (Calls / Called by, Uses / Used by, Implements..., `xN` = several call sites)
- Camera: drag to pan, wheel or pinch to zoom toward the pointer, the `+` / `-` / `fit` cluster bottom-left (Fit recentres the whole disc)
- Legend eye hides a module and the rest regrow into a full circle; legend label pushes the module out and rings it -- highlight and visibility are separate axes
- Heatmap day hover/click rings every symbol whose file entered git that day; `mark today` marks files touched today (mtime)
- 51% of the default-kind symbols on a typical index have no Calls edge (constants, type aliases, trait items): they are the `(unlinked)` group. Narrow `--kinds`, widen `--relation`, or `--unlinked drop` -- see the table above

## Pick by question

- "what is the shape of this codebase / module family" -> this skill
- "what depends on this one symbol" -> the x-ray skill's `visualize-dump.js` (focus neighbourhood)
- "how is the module tree organised" -> x-ray `visualize-tree.js`; "which modules call which" -> x-ray `visualize-bundle.js`

## Files

`graph.mjs` (CLI), `lib/dump.mjs` (dump reader), `lib/adapter.mjs` (dump -> disc data), `lib/dates.mjs` (git dates, mtime), `lib/palette.mjs` (generated wedge colours),
`template.html` + `vendor/` (the vault-graph renderer, MIT -- see `UPSTREAM.md` for the pin and the tagged hunks),
`assets/logo-mask.svg` (the mark in the hub, painted with the wedge colours).
