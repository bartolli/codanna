#!/usr/bin/env node
/**
 * Hierarchical edge bundling of the codebase from ONE `codanna dump` read:
 * the code hierarchy (module -> container -> member) as a radial cluster,
 * dependency edges (Calls by default) as bundled arcs between leaves. With
 * --depth, edges aggregate to the collapsed leaves (module-level "who calls
 * whom"), arc width ~ log(count). Hover a name: incoming blue, outgoing red.
 *
 * Usage:
 *   node visualize-bundle.js [--root <module prefix>] [--depth N] [--kinds a,b]
 *                            [--relation calls|uses|implements|extends]
 *                            [--from graph.jsonl] [--binary PATH] [--no-open] [--light]
 */

const { readDump } = require('./graph/dump');
const { buildHierarchy, DEFAULT_KINDS } = require('./graph/hierarchy');
const { generateBundleHTML } = require('./graph/bundle-render');
const { buildDetails, sidsInTree } = require('./graph/details');
const { saveArtifact, openFile } = require('./graph/publish');

const workingDir = process.env.CLAUDE_PROJECT_DIR || process.cwd();
const RELATIONS = { calls: 'Calls', uses: 'Uses', implements: 'Implements', extends: 'Extends' };

function parseArgs() {
  const args = process.argv.slice(2);
  const o = { root: null, depth: Infinity, kinds: new Set(DEFAULT_KINDS), relation: 'Calls', from: null, binary: process.env.CODANNA_BIN || 'codanna', open: true, theme: 'dark' };
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    if (a === '--root') o.root = args[++i];
    else if (a === '--depth') o.depth = parseInt(args[++i], 10);
    else if (a === '--kinds') o.kinds = new Set(args[++i].split(','));
    else if (a === '--relation') {
      const r = RELATIONS[(args[++i] || '').toLowerCase()];
      if (!r) { console.error('unknown relation; accepted: calls, uses, implements, extends'); process.exit(1); }
      o.relation = r;
    }
    else if (a === '--from') o.from = args[++i];
    else if (a === '--binary') o.binary = args[++i];
    else if (a === '--no-open') o.open = false;
    else if (a === '--light') o.theme = 'light';
    else { console.error(`unknown argument ${a}`); process.exit(1); }
  }
  return o;
}

// Main
const opts = parseArgs();
const t0 = Date.now();
const graph = readDump({ ...opts, workingDir });
console.log(`dump: ${graph.summary.symbols} symbols, ${graph.summary.relationships} relationships (${Date.now() - t0} ms)`);
const { hierarchy, leaves, containers, symbolLeaves, repOf } = buildHierarchy(graph, opts);
console.log(`tree: root '${hierarchy.name}', ${symbolLeaves} symbol leaves, ${containers} containers, rendered leaves ${leaves}${Number.isFinite(opts.depth) ? ` (depth ${opts.depth})` : ''}`);
if (leaves === 0) { console.error('nothing under that root / kinds'); process.exit(1); }

// Edges between leaf representatives, aggregated per (source, target) pair.
const pairs = new Map();
let total = 0, dropped = 0, selfPairs = 0;
for (const [from, byRel] of graph.out) {
  for (const to of (byRel[opts.relation] || [])) {
    total += 1;
    const s = repOf.get(from), t = repOf.get(to);
    if (!s || !t) { dropped += 1; continue; }
    if (s === t) { selfPairs += 1; continue; }
    const k = s + ' ' + t;
    pairs.set(k, (pairs.get(k) || 0) + 1);
  }
}
const edges = [...pairs.entries()].map(([k, count]) => { const [source, target] = k.split(' '); return { source, target, count }; });
const represented = edges.reduce((n, e) => n + e.count, 0);
console.log(`${opts.relation}: ${total} edges in the index; ${edges.length} arcs between ${leaves} leaves (${represented} edges represented, ${selfPairs} inside one leaf, ${dropped} outside the tree)`);

const title = `${hierarchy.name} - ${opts.relation} bundling`;
const subtitle = `${graph.summary.symbols} symbols in the index` + (opts.root ? `, root ${opts.root}` : '') + (Number.isFinite(opts.depth) ? `, depth ${opts.depth}` : '');
const html = generateBundleHTML(hierarchy, edges, { title, subtitle, workingDir, leaves, relation: opts.relation, legendKinds: [...opts.kinds], theme: opts.theme, details: buildDetails(graph, sidsInTree(hierarchy)) });
const safeName = (opts.root || 'index').replace(/[^a-zA-Z0-9_-]/g, '_');
const artifactFile = saveArtifact(html, `bundle-${safeName}`, workingDir);
console.log(`\nBundle saved to: ${artifactFile}`);
if (opts.open) openFile(artifactFile);
