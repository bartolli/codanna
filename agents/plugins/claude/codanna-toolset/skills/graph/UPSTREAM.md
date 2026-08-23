# Upstream: vault-graph

`template.html` and `vendor/` are taken from https://github.com/luke321/vault-graph
(MIT, Lukas Proprentner) at commit `1f0aba5`. The renderer -- plan, layout, cascade,
sigma reducers, the `__vg` probe API and everything `.ai-context/invariants.md` measures --
is unchanged. Upgrading = copy the new `src/template.html` and `vendor/*` over, then
re-apply the hunks below (each is tagged `codanna:` in the file, so
`diff upstream/src/template.html template.html` shows exactly this list).

| Hunk | What |
|---|---|
| vocabulary | title, search placeholder, timeline/today/refresh titles, heatmap label and day copy, tooltip and detail-panel nouns (note -> symbol, link -> edge, words -> lines) |
| detail panel | the `obsidian://` open link becomes the symbol's `file:start-end` chip |
| `buildStats` | footer reads `DATA.codanna` (relations, kinds, scope, index totals, date source) |
| PNG name | `codanna-graph.png` |
| camera | `enableCameraPanning: true`, the centre lock reduced to angle-only, `#zoomctl` (+ / - / fit) wired to `animatedZoom` / `animatedUnzoom` / `fit()` -- upstream pins the camera to the disc centre by design; a 10k-symbol disc needs its rim reachable |
| palette seam | `injectedPalette()` in `readTheme` + `THEME.slots.length` in `buildColors`: `window.VAULT_PALETTE = {dark, light}` from the CLI replaces the ten documented slots when the index has more groups (`lib/palette.mjs`, golden-angle OKLCH hues at the documented median L/C) |
| focus web | `drawFocusWeb`: the lit edges stroked once more on the hovers canvas, then the focus neighbours' discs re-drawn over them, under the label pill -- dim discs < web < lit discs < pill. (Not by marking the neighbours `highlighted`: that lifts them above the pill too.) Also offered upstream (not a codanna-specific change). |

Data contract the template reads (`window.VAULT_DATA`): `nodes[{id,label,folder,dirs,sub,type,tags,created,touched,words,deg}]`,
`edges[{s,t,w}]`, `stats{nodes,edges,orphans,unresolved,files,templatesExcluded,ghostsIncluded}`, `vault`, `generated`,
plus the codanna-only `codanna{relations,kinds,root,dates,symbolsTotal,relationshipsTotal,builderCommit,emissionVersion}`.
`lib/adapter.mjs` is the only producer.
