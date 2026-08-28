# Upstream: vault-graph

The page -- `shell.html`, `page.html`, `page.css`, `page.js` -- plus `lib/vendor.mjs`
and `vendor/{sigma.min.js,graphology.umd.min.js,NOTICE.md}` are taken from
https://github.com/luke321/vault-graph (MIT, Lukas Proprentner) at commit `a49b2e0`
(release 1.8.0), through the fork `bartolli/vault-graph`. The renderer -- plan,
layout, cascade, sigma reducers, the `__vg` probe API -- is unchanged. `graph.mjs`
assembles the standalone document exactly as upstream's `build-graph.mjs` does
(shell markers `<!--CSS-->`, `<!--MARKUP-->`, `<!--SCRIPT-->`, `<!--LIBS-->`,
`<!--ASSETS-->`, `<!--DATA-->`; `page.js`'s `export` line stripped so the result
stays a classic script).

Upgrading = copy the new `src/shell.html`, `src/page.html`, `src/page.css`,
`src/page.js`, `src/vendor.mjs` (to `lib/`) and `vendor/*` over, then re-apply the
hunks below -- each is tagged `codanna:` in the files, so a diff against upstream
shows exactly this list.

| Hunk | What |
|---|---|
| vocabulary | shell + page.html titles, search placeholder, heatmap label and compact-axis tooltip, Refresh tooltip, settings-panel labels (Module colours), tooltip/detail/heatnote/heatkey/day-tooltip/year-chip nouns (note -> symbol, link -> edge, words -> lines) |
| vscope | scope line under the title: `#vg-vscope` div (page.html), its CSS, filled by `buildStats` with root/relations/group |
| detail panel | the `obsidian://` open link becomes the symbol's `file:start-end` chip; the signature renders as `<pre class="sig">` through `sigHtml()` (highlight.js when the build inlined the language's grammar, escaped plain otherwise); the neighbour list is grouped by relation and direction from the adjacency histogram. Pin-to-hub stays |
| graph data | node attrs carry `sig` and `lang`; each `adj` entry carries `r` -- the relation/direction histogram normalised to that node's perspective (`r[Relation] = [outgoing, incoming]`). On the adjacency, not on edge attributes, because budgeted vaults never materialise trimmed edges |
| buildStats | title = project, `#vg-vscope` = scope line, `DOC.title`, footer reads `DATA.codanna` (relations, kinds, scope, index totals, date source) |
| PNG name | `codanna-graph.png` |
| hover ramp | `hoverAmount()` pinned at 1 while the hovered note is the selected one -- otherwise the leave-tween ramps the active note's size, the web's alpha and the dim down and snaps them back when it releases `state.hovered`: a flick on every mouse-out. Upstream ships the ramp at 1.8.0; offered upstream as luke321/vault-graph#38, branch `0004-hover-pin-on-the-selected-note` |
| edge pixel clamp | 1.7.0's identity `zoomToSizeRatioFunction` (dots track the lattice) makes edge strokes grow linearly with zoom: 8px at 5x, 16px at 10x, a hub fan merging into a ribbon wider than its discs. The edge reducer clamps size to `EDGE_MAX_PX * ratio` (drawn px = size / ratio, so strokes never pass 4px); the camera hook re-runs reducers, rAF-throttled, only while the clamp can bind (ratio < 0.4). Dots keep the identity law -- `measureSizeScale` documents why a pixel cap on dots is wrong. Offered upstream as luke321/vault-graph#39, branch `0005-edge-strokes-clamped-in-pixels` |
| hop trail | the relationship lists walk the graph with no way back: hops through `data-go` (and only hops) accumulate `navTrail`, rendered as a back arrow plus clickable crumbs at the top of the card (first, ellipsis, last two). A crumb click truncates the trail to that point; a fresh selection or close starts over. Keyboard: Alt+ArrowLeft / Backspace step back (preventDefault -- the browser answers both with history navigation, which on a file:// page leaves the graph), Escape blurs the input / closes the settings panel / closes the card, `/` focuses search; keys inside inputs stay the input's, and with two views in one document only the focused one acts. Offered upstream as luke321/vault-graph#40, branch `0006-detail-panel-hop-trail` -- the offered branch adds a self-unbinding keydown listener this standalone copy does not carry |
| hljs | `vendor/hljs/` (BSD-3-Clause, `BUILD.md` there has the esbuild recipe): core + one grammar per language present in the dump, inlined by `graph.mjs` after sigma/graphology, each gated on `findNetworkPrimitives()` finding nothing. `page.css` maps the `hljs-*` classes onto the page's own palette slots |

Dropped from the previous vendoring (baseline `1f0aba5`), superseded upstream:
camera/`#zoomctl` (1.7.0's corner camera cluster + `enableCameraPanning` default-on),
focus web (`drawFocusWeb` adopted, with a `checkFocusWeb` diagnostic), the unlinked
group colour fix (adopted), the generated-palette seam and `lib/tokens.mjs` (upstream's
twelve cycling slots + per-folder picker replace both; its role tokens are the values
our tokens module canonised, and its 12-slot palette absorbed the slot-5/6
transposition and the slot-6/10 de-pastel with measurement).

Data contract the page reads (`window.VAULT_DATA`):
`nodes[{id,label,folder,dirs,sub,type,tags,created,touched,words,deg,sig,lang}]`,
`edges[{s,t,w,r}]` (`r[Relation] = [count s->t, count t->s]`, `s < t`),
`stats{nodes,edges,orphans,unresolved,files,templatesExcluded,ghostsIncluded}`,
`vault`, `generated`, plus the codanna-only `codanna{relations,kinds,root,group,
dates,unlinked,symbolsTotal,relationshipsTotal,builderCommit,emissionVersion}`.
`lib/adapter.mjs` is the only producer.

Page settings (colour picks, per-module visibility defaults, pan mode, compact axis,
pinned hub symbols) persist in the browser's localStorage under
`vault-graph:settings:<vault>` -- scoped by the index name, wrapped in try/catch, so
a blocked store costs only persistence.
