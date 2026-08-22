// codanna dump -> the data shape template.html reads (window.VAULT_DATA).
//
// The disc groups by `folder` (wedge), nests by `dirs` (legend tree / tint slots),
// sizes by `deg`, orders the timeline by `created`, and marks `touched`. Here: wedge =
// first module segment, dirs = the remaining segments plus the containing type, deg =
// degree over the chosen relationship kinds.
import path from "node:path";
import { firstCommitDates, touchedDay, absolute, localDay } from "./dates.mjs";

export const CONTAINERS = new Set(["Struct", "Class", "Trait", "Enum", "Interface"]);
export const DEFAULT_KINDS = ["Function", "Method", "Struct", "Class", "Trait", "Enum", "Interface", "TypeAlias", "Constant", "Macro"];
export const RELATIONS = ["calls", "uses", "implements", "extends", "defines"];
export const WEDGE_AXES = ["module", "language", "kind"];
export const TINT_AXES = ["module", "kind", "visibility"];

/** "module", "module/kind", "language/module", ... -> {wedge, tint}; throws on an unknown axis. */
export function parseGroup(spec) {
  const [wedge, tint = "module"] = String(spec || "module").split("/");
  if (!WEDGE_AXES.includes(wedge)) throw new Error(`--group wedge '${wedge}' is not one of ${WEDGE_AXES.join(", ")}`);
  if (!TINT_AXES.includes(tint)) throw new Error(`--group tint '${tint}' is not one of ${TINT_AXES.join(", ")}`);
  return { wedge, tint };
}

/** Module path -> segments across the languages' conventions; file path fallback. */
export function segmentsOf(sym) {
  if (sym.module) {
    const cleaned = sym.module.replace(/^res:\/\//, "").replace(/^\.\//, "").replace(/^\\+/, "");
    const segs = cleaned.split(/::|\.|\/|\\/).filter(Boolean);
    if (segs[0] === "crate") segs.shift();   // root-level items group as "(root)"
    return segs;
  }
  const p = sym.file.replace(/^\.\//, "");
  const parts = p.split("/");
  const last = parts.pop() || "";
  return [...parts, last.replace(/\.[^.]+$/, "")].filter(Boolean);
}

export function normalizePrefix(prefix) {
  const segs = prefix.replace(/^res:\/\//, "").replace(/^\.\//, "").replace(/^\\+/, "").split(/::|\.|\/|\\/).filter(Boolean);
  if (segs.length > 1 && segs[0] === "crate") segs.shift();
  return segs;
}

export function buildData(dump, opts) {
  const { symbols, relationships, summary } = dump;
  const kinds = new Set(opts.kinds);
  const relations = new Set([...opts.relations].map((r) => r.toLowerCase()));
  const prefix = opts.root ? normalizePrefix(opts.root) : [];
  const group = opts.group || { wedge: "module", tint: "module" };
  const underPrefix = (segs) => prefix.every((p, i) => segs[i] === p);

  // Containing type per member, from Defines rows whose source is a container kind.
  const containerOf = new Map();
  for (const r of relationships) {
    if (r.relation === "Defines" && CONTAINERS.has(r.fromKind)) containerOf.set(r.to, r.from);
  }

  const dates = opts.dates === "git" ? firstCommitDates(opts.workingDir) : null;
  const index = new Map();   // symbol id -> node index
  const nodes = [];
  const files = new Set();
  for (const sym of symbols.values()) {
    if (sym.kind === "Module" || !kinds.has(sym.kind)) continue;
    const segs = segmentsOf(sym);
    if (!underPrefix(segs)) continue;
    const rest = segs.slice(prefix.length);
    const c = containerOf.get(sym.id);
    const container = c !== undefined ? symbols.get(c) : null;
    const containerName = container && container.id !== sym.id ? container.name : null;
    // Two grouping channels: the wedge (angle = share, neighbours adjacent) and the
    // tint chain inside it (legend tree; the three biggest get their own shade).
    const folder = group.wedge === "language" ? (sym.language || "(unknown)")
                 : group.wedge === "kind" ? sym.kind
                 : (rest[0] || "(root)");
    let dirs;
    if (group.tint === "kind") dirs = [sym.kind];
    else if (group.tint === "visibility") dirs = [String(sym.visibility || "(unknown)").toLowerCase()];
    else {
      dirs = group.wedge === "module" ? rest.slice(1) : rest.slice();
      if (containerName) dirs.push(containerName);
    }
    const file = sym.file;
    files.add(file);
    index.set(sym.id, nodes.length);
    nodes.push({
      id: `${file}:${sym.line}-${sym.endLine || sym.line}`,
      label: sym.name,
      folder,
      dirs,
      sub: dirs[0] || "",
      type: sym.kind,
      tags: [sym.kind, sym.visibility, sym.language].filter(Boolean).map((t) => String(t).toLowerCase()),
      created: dates ? (dates.get(file) || "") : "",
      touched: touchedDay(absolute(opts.workingDir, file)),
      words: Math.max(1, (sym.endLine || sym.line) - sym.line + 1),
      sig: sym.signature || "",
    });
  }

  // One undirected pair per edge, plus a relation/direction histogram the detail panel
  // groups by: r[Relation] = [rows from the lower index to the higher, rows the other way].
  const weight = new Map(), rels = new Map();
  for (const r of relationships) {
    if (!relations.has(r.relation.toLowerCase())) continue;
    const i = index.get(r.from), j = index.get(r.to);
    if (i === undefined || j === undefined || i === j) continue;
    const key = i < j ? `${i} ${j}` : `${j} ${i}`;
    weight.set(key, (weight.get(key) || 0) + 1);
    let h = rels.get(key); if (!h) { h = {}; rels.set(key, h); }
    const c = (h[r.relation] ||= [0, 0]); c[i < j ? 0 : 1] += 1;
  }
  const edges = [...weight].map(([k, w]) => { const [s, t] = k.split(" ").map(Number); return { s, t, w, r: rels.get(k) }; });
  let degree = new Array(nodes.length).fill(0);
  for (const e of edges) { degree[e.s]++; degree[e.t]++; }
  nodes.forEach((n, i) => { n.deg = degree[i]; });
  // --unlinked drop: symbols with no edge over the chosen relations leave the data,
  // re-indexing the edge endpoints. (In the page, the legend eye on "(unlinked)" hides
  // them with the cascade instead -- same picture, bigger file.)
  let kept = nodes, keptEdges = edges;
  if (opts.unlinked === "drop") {
    const remap = new Map();
    kept = nodes.filter((n, i) => { if (degree[i] > 0) { remap.set(i, remap.size); return true; } return false; });
    keptEdges = edges.map((e) => ({ s: remap.get(e.s), t: remap.get(e.t), w: e.w, r: e.r }));
    degree = kept.map((n) => n.deg);
  }

  return {
    vault: opts.name || path.basename(opts.workingDir),
    generated: (() => { const d = new Date(); return `${localDay(d)} ${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`; })(),
    nodes: kept,
    edges: keptEdges,
    stats: {
      files: files.size,
      nodes: kept.length,
      edges: keptEdges.length,
      unresolved: 0,
      orphans: degree.filter((d) => d === 0).length,
      unlinkedDropped: opts.unlinked === "drop" ? nodes.length - kept.length : 0,
      templatesExcluded: false,
      ghostsIncluded: false,
    },
    // codanna-only: the template's stats footer reads these when present.
    codanna: {
      relations: [...relations],
      kinds: [...kinds],
      root: opts.root || "",
      group: `${group.wedge}/${group.tint}`,
      dates: dates ? "git" : "none",
      unlinked: opts.unlinked === "drop" ? "drop" : "include",
      symbolsTotal: summary.symbols,
      relationshipsTotal: summary.relationships,
      builderCommit: summary.builder_commit || "",
      emissionVersion: summary.emission_version,
    },
  };
}
