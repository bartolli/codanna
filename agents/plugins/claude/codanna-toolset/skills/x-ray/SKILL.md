---
name: x-ray
description: Deep codebase exploration using semantic search and relationship mapping. Use when you need to understand the current codebase.
allowed-tools: Bash(codanna:*), Bash(sed:*), Bash(rg:*), Bash(node:*), Read, Grep, Glob
---

## Reframe

LITERAL: "$ARGUMENTS"

### Definitions
- LITERAL — what the user typed
- INTENT — what they mean, given SESSION_CONTEXT
- SESSION_CONTEXT — recent work; held by you, not by the index
- BRIDGE — your job: LITERAL + SESSION_CONTEXT → INTENT → query

### Rules
- Search INTENT, not LITERAL.
- Context disambiguates → NARROW.
- Context insufficient → BROAD. Vague beats confidently-wrong-narrow.

### Transforms (LITERAL → INTENT)
- VAGUE          → specify        ("that parsing thing" → "language parser implementation")
- QUESTION       → keywords       ("how does parsing work?" → "parsing implementation process")
- CONVERSATIONAL → technical      ("stuff that handles languages" → "language handler processor")
- BROAD          → contextualize  ("errors" → "error handling exception management")
- CONTEXTUAL     → reconstruct    ("the logging" → "prompt eval logging", when SESSION_CONTEXT = prompt eval work)

OptimizedQuery: _{written against INTENT}_

---

## Loop

### Definitions
- SEARCH        — `codanna mcp semantic_search_with_context query:"<q>" limit:5`
- INSPECT       — read source at a LOCATION (Read tool or `sed -n 'A,Bp' file`)
- TRAVERSE      — `codanna retrieve describe <name|symbol_id:N>` on a RELATIONSHIP
- REFINE        — return to SEARCH with a new OptimizedQuery

- RESULT        — one hit from SEARCH; carries SCORE, signature, doc, LOCATION, RELATIONSHIPS
- SCORE         — relevance ∈ [0,1]; focus on SCORE > 0.6
- LOCATION      — `file_path:start_line-end_line`
- RELATIONSHIPS — calls, called_by, implements, defines

### Default flow
1. SEARCH(OptimizedQuery)
2. For each RESULT with SCORE > 0.6:
   - INSPECT if signature/doc is insufficient
   - TRAVERSE 1–2 RELATIONSHIPS that look load-bearing
3. Picture incomplete → REFINE with what you learned
4. INTENT answered → stop

### INSPECT mechanics
LOCATION `src/io/exit_code.rs:108-120` →
- Read tool: `file_path=<env.cwd>/src/io/exit_code.rs, offset=108, limit=13`
- limit formula: `end_line - start_line + 1`
- sed (Unix only): `sed -n '108,120p' src/io/exit_code.rs`

### TRAVERSE heuristics
- RELATIONSHIPS appearing across multiple RESULTs are load-bearing — follow first
- 1–2 per RESULT is usually enough
- Prefer `symbol_id:N` over name when shown — avoids ambiguity

---

## Mode

This skill is EXPLORE, not ACT.
- EXPLORE: build understanding, surface patterns, identify integration points
- ACT:     modify code, refactor, implement

INTENT answered → present findings → await user direction.
Do not transition to ACT inside this skill.

---

## Graph

EXPLORE-legal: derives a read-only view, does not modify source.

When TRAVERSE reveals dense, multi-directional RELATIONSHIPS, generate an interactive 3D graph:

```bash
# codanna >= 0.14 (has `codanna dump`): one index read, then the neighborhood in memory
node ${CLAUDE_SKILL_DIR}/visualize-dump.js <symbol_id:ID | name> [depth] [--self-contained]
# older binaries: one `retrieve describe` per node
node ${CLAUDE_SKILL_DIR}/visualize-graph.js <symbol_id:ID> [depth]
```

- Default depth: 2; both scripts emit the same graph shape and template
- Output: HTML file at `.codanna/visualizations/graph-{name}-{timestamp}.html` -- tell user to open in browser;
  `--self-contained` inlines the vendored libs so the file opens from anywhere (2.3 MB)
- `visualize-dump.js` carries `visibility` and line ranges for every node and is deterministic
  for a given index (adjacency sorted by symbol id before the per-level caps); `--from graph.jsonl`
  reuses a saved `codanna dump`, `--binary PATH` picks the binary, `--no-open` skips the browser
- Nodes color-coded by symbol kind; edges labeled by relationship type

Two more views from the same dump (pure d3/SVG, self-contained, dark theme, `--light` for white):

```bash
# structure: radial tidy tree, module -> container -> member
node ${CLAUDE_SKILL_DIR}/visualize-tree.js [--root crate::indexing] [--depth 3] [--kinds Function,Method,...]
# dependencies on the structure: hierarchical edge bundling (Calls by default)
node ${CLAUDE_SKILL_DIR}/visualize-bundle.js [--root crate::indexing] [--depth 3] [--relation calls|uses|implements|extends]
```

- Pick by question: "what depends on this symbol" -> `visualize-dump.js` (focus neighborhood, Force or Top-down/Left-right/Radial DAG);
  "how is the codebase organized" -> `visualize-tree.js`; "which modules call which" -> `visualize-bundle.js --depth N`
  (edges aggregate to the collapsed leaves, arc width ~ log count; hover a name: callers blue, callees red)
- `--depth` collapses deeper levels into counted labels; `--root` scopes to a module prefix (edges crossing the root are dropped and counted)
- Symbols without a module path hang off their file path; Field/Variable/Parameter are out by default (`--kinds` to include)
- Shared pieces: `graph/dump.js` (dump reader), `graph/hierarchy.js` (tree builder + symbol-to-leaf map), `graph/tree-render.js`, `graph/bundle-render.js`

### Suggest when
- RESULT has 3+ RELATIONSHIPS in multiple directions
- User asks about connections, dependencies, or topology
- TRAVERSE across RESULTs reveals a tangled web

### Skip when
- 0-2 RELATIONSHIPS (TRAVERSE is sufficient)
- User asks about implementation, not structure

---

## Budget

Approximate per-operation cost:
- SEARCH   — ~500 tokens
- TRAVERSE — ~300 tokens
- INSPECT  — ~100–500 tokens (depends on range)

Prefer fewer high-value operations over many cheap ones. Three SEARCHes with deliberate REFINE beats ten with drift.

---

## Filters

Add `lang:rust` (or `lang:python`, `lang:typescript`, …) to SEARCH to narrow by language in multi-language projects.
