#!/usr/bin/env node
/**
 * 3D force graph of a symbol's neighborhood from ONE `codanna dump` read.
 *
 * Same output shape and template as visualize-graph.js, which walks the
 * per-symbol surface (one `retrieve describe` per node); this reads the whole
 * resolved graph once (symbol rows + relationship rows, see `codanna dump`),
 * then derives the focus neighborhood in memory.
 *
 * Usage:
 *   node visualize-dump.js <symbol_id:ID | name> [depth] [--cap N] [--from graph.jsonl]
 *                          [--binary PATH] [--self-contained] [--no-open] [--light]
 *                          [--3d] (3D force view; default is the 2D layered DAG)
 *   node visualize-dump.js --all [--relation KIND] [...]
 *   depth default 2. --cap N caps neighbors per relation kind beyond the root
 *   (default 5, 0 = uncapped). --all skips the focus walk and renders the whole
 *   graph, optionally one relation kind (Calls, Defines, Uses, Implements,
 *   Extends). --from reads a saved dump instead of running the binary.
 *   --self-contained inlines the vendored libs (the HTML opens from anywhere).
 *   --layout force|td|lr|radialout sets the initial layout (default force);
 *   every page carries all four as buttons.
 *   Graphs above 2000 nodes or 3000 links get a BAKED 2D force layout
 *   (vendored d3 run here, positions pinned as fx/fy/fz): the page starts at
 *   its post-layout frame rate instead of running the layout in the browser.
 *   --bake / --no-bake override the size rule.
 */

const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');
const { generateHTML, inlineVendor } = require('./graph/render');
const { generateDAGHTML } = require('./graph/dag-render');
const { readDump } = require('./graph/dump');
const { saveArtifact, serveAndOpen, openFile } = require('./graph/publish');

const workingDir = process.env.CLAUDE_PROJECT_DIR || process.cwd();

function parseArgs() {
  const args = process.argv.slice(2);
  const flags = { from: null, binary: process.env.CODANNA_BIN || 'codanna', selfContained: false, open: true, cap: 5, all: false, relation: null, layout: 'force', bake: null };
  const positional = [];
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    if (a === '--from') flags.from = args[++i];
    else if (a === '--binary') flags.binary = args[++i];
    else if (a === '--self-contained') flags.selfContained = true;
    else if (a === '--no-open') flags.open = false;
    else if (a === '--light') flags.theme = 'light';
    else if (a === '--cap') flags.cap = parseInt(args[++i], 10);
    else if (a === '--all') flags.all = true;
    else if (a === '--relation') flags.relation = args[++i];
    else if (a === '--layout') { flags.layout = args[++i]; flags.threeD = true; }
    else if (a === '--3d') flags.threeD = true;
    else if (a === '--bake') flags.bake = true;
    else if (a === '--no-bake') flags.bake = false;
    else positional.push(a);
  }
  if (!flags.all && positional.length < 1) {
    console.error('Usage: node visualize-dump.js <symbol_id:ID | name> [depth] [--cap N] [--from graph.jsonl] [--binary PATH] [--self-contained] [--no-open]');
    console.error('       node visualize-dump.js --all [--relation KIND] [...]');
    process.exit(1);
  }
  if (!['force', 'td', 'lr', 'radialout'].includes(flags.layout)) {
    console.error(`unknown layout '${flags.layout}'; accepted: force, td, lr, radialout`); process.exit(1);
  }
  if (flags.relation) {
    const canon = { calls: 'Calls', defines: 'Defines', uses: 'Uses', implements: 'Implements', extends: 'Extends' }[flags.relation.toLowerCase()];
    if (!canon) { console.error(`unknown relation '${flags.relation}'; accepted: calls, defines, uses, implements, extends`); process.exit(1); }
    flags.relation = canon;
  }
  return { symbol: positional[0] || null, depth: parseInt(positional[1] || '2', 10), ...flags };
}

function getGroup(kind) {
  switch ((kind || '').toLowerCase()) {
    case 'function': return 1;
    case 'method': return 2;
    case 'class': return 3;
    case 'struct': return 4;
    case 'constant': return 5;
    case 'variable': return 6;
    case 'trait': return 7;
    case 'interface': return 8;
    case 'enum': return 9;
    default: return 0;
  }
}

/** Same neighborhood walk and per-relation caps as visualize-graph.js. */
const OUT = [['Calls', 'calls', 20], ['Defines', 'defines', 30], ['Implements', 'implements', 10], ['Extends', 'extends', 5], ['Uses', 'uses', 10]];
const IN = [['Calls', 'calledBy', 20], ['Implements', 'implementedBy', 20], ['Extends', 'extendedBy', 10], ['Uses', 'usedBy', 10]];

function degree(graph, id) {
  const sum = (m) => Object.values(m.get(id) || {}).reduce((n, v) => n + v.length, 0);
  return sum(graph.out) + sum(graph.inc);
}

function resolveRoot(graph, symbol) {
  if (symbol.startsWith('symbol_id:')) {
    const id = parseInt(symbol.slice('symbol_id:'.length), 10);
    if (!graph.symbols.has(id)) { console.error(`symbol_id ${id} is not in the dump`); process.exit(1); }
    return id;
  }
  const matches = [...graph.symbols.values()].filter(s => s.name === symbol);
  if (matches.length === 0) { console.error(`Symbol not found: ${symbol}`); process.exit(1); }
  matches.sort((a, b) => degree(graph, b.id) - degree(graph, a.id));
  if (matches.length > 1) {
    console.error(`${matches.length} symbols named '${symbol}'; using the most connected:`);
    for (const m of matches.slice(0, 8)) console.error(`  symbol_id:${m.id}  ${m.kind}  ${m.file}:${m.line + 1}  (degree ${degree(graph, m.id)})`);
  }
  return matches[0].id;
}

/** Run the 2D force layout here (vendored d3) and pin every node: the page
 *  then renders positions instead of computing them. Same force shape as
 *  the browser default (link distance 30, charge -30), z = 0. */
function bakeLayout(graphData, ticks = 300) {
  const d3 = require(path.join(__dirname, 'graph', 'vendor', 'd3.min.js'));
  const nodes = graphData.nodes.map(n => ({ id: n.id }));
  const links = graphData.links.map(l => ({ source: l.source, target: l.target }));
  const t0 = Date.now();
  const sim = d3.forceSimulation(nodes)
    .force('link', d3.forceLink(links).id(d => d.id).distance(30))
    .force('charge', d3.forceManyBody().strength(-30).theta(0.9).distanceMax(600))
    .force('center', d3.forceCenter(0, 0))
    .force('collide', d3.forceCollide(5))
    .stop();
  for (let i = 0; i < ticks; i++) sim.tick();
  nodes.forEach((n, i) => { const g = graphData.nodes[i]; g.fx = n.x; g.fy = n.y; g.fz = 0; });
  return Date.now() - t0;
}

/** Whole graph: every symbol row, every relationship row (optionally one kind). */
function buildWholeGraph(graph, relation) {
  const typeOf = { Calls: 'calls', Defines: 'defines', Implements: 'implements', Extends: 'extends', Uses: 'uses' };
  const links = [];
  const seen = new Set();
  for (const [from, byRel] of graph.out) {
    for (const [rel, targets] of Object.entries(byRel)) {
      if (relation && rel !== relation) continue;
      for (const to of targets) {
        const key = `${from}->${to}`;
        if (seen.has(key) || !graph.symbols.has(from) || !graph.symbols.has(to)) continue;
        seen.add(key);
        links.push({ source: from, target: to, type: typeOf[rel] || rel.toLowerCase() });
      }
    }
  }
  const connected = new Set(links.flatMap(l => [l.source, l.target]));
  const nodes = [...graph.symbols.values()]
    .filter(s => !relation || connected.has(s.id))
    .map(s => ({ ...s, group: getGroup(s.kind), level: 1 }));
  return { nodes, links };
}

function buildGraph(graph, rootId, depth, cap) {
  const nodes = new Map();
  const links = [];
  const visited = new Set();
  const addNode = (id, level) => {
    const s = graph.symbols.get(id);
    if (!s) return false;
    if (!nodes.has(id)) nodes.set(id, { ...s, group: getGroup(s.kind), level });
    return true;
  };
  function explore(id, level) {
    if (level > depth || visited.has(id)) return;
    visited.add(id);
    if (!addNode(id, level)) return;
    const walk = (table, spec, direction) => {
      const byRel = table.get(id) || {};
      for (const [relation, type, maxItems] of spec) {
        const limit = cap === 0 ? Infinity : (level === 0 ? maxItems : Math.min(maxItems, cap));
        for (const other of (byRel[relation] || []).slice(0, limit)) {
          if (!addNode(other, level + 1)) continue;
          links.push({ source: direction === 'out' ? id : other, target: direction === 'out' ? other : id, type });
          if (level + 1 <= depth) explore(other, level + 1);
        }
      }
    };
    walk(graph.out, OUT, 'out');
    walk(graph.inc, IN, 'in');
  }
  explore(rootId, 0);
  const seen = new Set();
  const uniqueLinks = links.filter(l => { const k = `${l.source}->${l.target}`; if (seen.has(k)) return false; seen.add(k); return true; });
  return { nodes: [...nodes.values()], links: uniqueLinks };
}

// Main
const opts = parseArgs();
const t0 = Date.now();
const graph = readDump({ ...opts, workingDir });
console.log(`dump: ${graph.summary.symbols} symbols, ${graph.summary.relationships} relationships (${Date.now() - t0} ms)`);
let graphData;
let safeName;
if (opts.all) {
  graphData = buildWholeGraph(graph, opts.relation);
  safeName = opts.relation ? `all-${opts.relation.toLowerCase()}` : 'all';
  console.log(`Whole graph${opts.relation ? ` (${opts.relation})` : ''}: ${graphData.nodes.length} nodes and ${graphData.links.length} links`);
} else {
  const rootId = resolveRoot(graph, opts.symbol);
  graphData = buildGraph(graph, rootId, opts.depth, opts.cap);
  safeName = opts.symbol.replace(/[^a-zA-Z0-9_-]/g, '_');
  console.log(`Found ${graphData.nodes.length} nodes and ${graphData.links.length} links (root symbol_id:${rootId}, depth ${opts.depth}, cap ${opts.cap === 0 ? 'none' : opts.cap})`);
}
const large = graphData.nodes.length > 2000 || graphData.links.length > 3000;
if (opts.bake === true || (opts.bake === null && large)) {
  const ms = bakeLayout(graphData);
  console.log(`Baked 2D layout in ${ms} ms (300 ticks); the page renders pinned positions`);
}
// Default neighbourhood view: the 2D layered DAG -- direction and labels at
// rest. The 3D force view stays behind --3d (or an explicit --layout) and is
// always the shape of --all (thousands of nodes want WebGL, not SVG).
const use3d = opts.all || opts.threeD;
let html;
if (use3d) {
  html = generateHTML(graphData, { layout: opts.layout, theme: opts.theme || 'dark' });
  if (opts.selfContained) html = inlineVendor(html);
} else {
  const rootNode = graphData.nodes.find(n => n.level === 0);
  html = generateDAGHTML(graphData, {
    title: (rootNode ? rootNode.name : opts.symbol) + ' - Call DAG',
    subtitle: `depth ${opts.depth}` + (opts.cap ? `, cap ${opts.cap}` : ''),
    workingDir, theme: opts.theme || 'dark',
  });
}
const artifactFile = saveArtifact(html, safeName, workingDir);
console.log(`\nVisualization saved to: ${artifactFile}`);
if (use3d) serveAndOpen(html, __dirname, { open: opts.open });
else if (opts.open) openFile(artifactFile);
