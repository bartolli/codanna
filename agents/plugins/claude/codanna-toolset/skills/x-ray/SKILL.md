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

When TRAVERSE reveals dense, multi-directional RELATIONSHIPS, visualize. All
pages share the design tokens with the sibling `graph` skill: dark by default,
light/dark toggle top-right, `--light` opens light.

```bash
# codanna >= 0.14 (has `codanna dump`): one index read, then the neighborhood in memory
node ${CLAUDE_SKILL_DIR}/visualize-dump.js <symbol_id:ID | name> [depth]
# older binaries: one `retrieve describe` per node (3D force view only)
node ${CLAUDE_SKILL_DIR}/visualize-graph.js <symbol_id:ID> [depth]
```

- Default view: a 2D layered call DAG -- callers above, callees below, labels at
  rest, deterministic; hover a name to light incoming (accent) / outgoing (red)
  edges; dashed edges are cycle edges; click a node to open its file
- `--3d` keeps the 3D force view (the 5-second gestalt of a tangled
  neighbourhood; `--layout force|td|lr|radialout` implies it); `--all` renders
  the whole index and is always 3D (`--bake` / `--no-bake` control the
  server-side pinned layout, on by default above 2000 nodes)
- Flags: depth default 2 (positional), `--cap N` per-level cap (default 5, 0 =
  none), `--from graph.jsonl` reuses a saved dump, `--binary PATH`, `--no-open`,
  `--relation calls|defines|uses|implements|extends` filters `--all`,
  `--self-contained` inlines the 3D vendor libs (the DAG page always is)
- Output: `.codanna/visualizations/graph-{name}-{timestamp}.html`

Structure and flow over the same dump:

```bash
# structure: collapsible left-to-right tree (drill-down); --radial for the poster IMAGE
node ${CLAUDE_SKILL_DIR}/visualize-tree.js [--root crate::indexing] [--depth N] [--kinds Function,Method,...] [--radial]
# dependencies on the structure: hierarchical edge bundling (Calls by default)
node ${CLAUDE_SKILL_DIR}/visualize-bundle.js [--root crate::indexing] [--depth N] [--relation calls|uses|implements|extends]
```

- Pick by question: "what depends on this symbol / what breaks if I change it"
  -> `visualize-dump.js` (call DAG); "how is this module organized / let me
  explore" -> `visualize-tree.js` (expand/collapse like a file explorer -- the
  NAVIGATION surface for structure); "which modules call which, inside one
  scope, on a printable page" -> `visualize-bundle.js --depth N` (arcs
  aggregate to collapsed leaves, width ~ log count; hover: callers blue,
  callees red)
- `--radial` is a POSTER, not a navigation surface: one all-at-once image of a
  scope's containment shape for docs, slides, and side-by-side scope
  comparison. Without an explicit `--depth` it auto-collapses deep levels into
  counted ancestors (<= ~1200 rendered leaves). Never reach for it when the
  user wants to explore -- that is the collapsible tree's job
- Every 2D view opens the same detail panel on click (signature, kind chips,
  file:span, relation groups with go-navigation where the view supports it);
  opening the source is the explicit Open-file action inside the panel, never
  the click itself -- file:// opens in the browser, not the editor
- "what is the shape of the WHOLE codebase -- module shares, hubs, which
  modules talk, history over time" -> the sibling `graph` skill
  (`node ${CLAUDE_SKILL_DIR}/../graph/graph.mjs`): the disc is the living
  whole-map these views zoom into. The division: the disc owns
  whole-codebase RELATIONSHIP shape (edge webs, hubs, git-date timeline);
  x-ray owns scoped CONTAINMENT structure and single-symbol neighbourhoods.
  When both could answer, prefer the disc for whole-codebase questions and
  x-ray for scoped ones -- never render both for one question
- `--depth` collapses deeper levels into counted labels; `--root` scopes to a
  module prefix (edges crossing the root are dropped and counted); symbols
  without a module path hang off their file path; Field/Variable/Parameter out
  by default (`--kinds` to include)
- Shared pieces: `graph/dump.js` (dump reader), `graph/hierarchy.js` (tree
  builder + symbol-to-leaf map), the renderers under `graph/`, and
  `graph/theme.js` + `graph/tokens.mjs` (design tokens; node >= 22.12)

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
