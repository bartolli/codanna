#!/usr/bin/env node
/**
 * Radial tidy tree of the codebase from ONE `codanna dump` read (pure d3, SVG).
 *
 * Hierarchy: module path segments -> container (Struct/Class/Trait/Enum/
 * Interface, via Defines edges) -> member. Symbols without a module path hang
 * off their file path. Module-kind symbols are the segments themselves.
 *
 * Usage:
 *   node visualize-tree.js [--root <module prefix>] [--depth N] [--kinds a,b,c]
 *                          [--from graph.jsonl] [--binary PATH] [--no-open] [--light]
 *                          [--radial] (radial poster view; default is the collapsible tree)
 *   --root   keep symbols under this module prefix (e.g. crate::indexing or
 *            examples.python); default: whole index
 *   --depth  collapse nodes deeper than N into their ancestor with a count
 *   --kinds  leaf kinds to include; default Function,Method,Struct,Class,Trait,
 *            Enum,Interface,TypeAlias,Constant,Macro (Field/Variable/Parameter
 *            are noise for structure)
 *   --light  white background (the notebook look); default is dark
 */

const path = require('path');
const { readDump } = require('./graph/dump');
const { generateTreeHTML } = require('./graph/tree-render');
const { generateCollapseHTML } = require('./graph/collapse-render');
const { buildDetails, sidsInTree } = require('./graph/details');
const { buildHierarchy, DEFAULT_KINDS } = require('./graph/hierarchy');
const { saveArtifact, openFile } = require('./graph/publish');

const workingDir = process.env.CLAUDE_PROJECT_DIR || process.cwd();
function parseArgs() {
  const args = process.argv.slice(2);
  const o = { root: null, depth: Infinity, kinds: new Set(DEFAULT_KINDS), from: null, binary: process.env.CODANNA_BIN || 'codanna', open: true, theme: 'dark' };
  for (let i = 0; i < args.length; i++) {
    const a = args[i];
    if (a === '--root') o.root = args[++i];
    else if (a === '--depth') o.depth = parseInt(args[++i], 10);
    else if (a === '--kinds') o.kinds = new Set(args[++i].split(','));
    else if (a === '--from') o.from = args[++i];
    else if (a === '--binary') o.binary = args[++i];
    else if (a === '--no-open') o.open = false;
    else if (a === '--light') o.theme = 'light';
    else if (a === '--radial') o.view = 'radial';
    else { console.error(`unknown argument ${a}`); process.exit(1); }
  }
  return o;
}

// Main
const opts = parseArgs();
const t0 = Date.now();
const graph = readDump({ ...opts, workingDir });
console.log(`dump: ${graph.summary.symbols} symbols, ${graph.summary.relationships} relationships (${Date.now() - t0} ms)`);
const { hierarchy, leaves, containers, symbolLeaves } = buildHierarchy(graph, opts);
console.log(`tree: root '${hierarchy.name}', ${symbolLeaves} symbol leaves, ${containers} containers, rendered leaves ${leaves}${Number.isFinite(opts.depth) ? ` (depth ${opts.depth})` : ''}`);
if (leaves === 0) { console.error('nothing under that root / kinds'); process.exit(1); }

const title = `${hierarchy.name} - Code tree`;
const subtitle = `${graph.summary.symbols} symbols in the index` + (opts.root ? `, root ${opts.root}` : '') + (Number.isFinite(opts.depth) ? `, depth ${opts.depth}` : '');
// Default: the collapsible left-to-right tree (drill-down, labels at rest);
// --radial keeps the all-at-once radial poster.
const details = buildDetails(graph, sidsInTree(hierarchy));
const html = opts.view === 'radial'
  ? generateTreeHTML(hierarchy, { title, subtitle, workingDir, leaves, legendKinds: [...opts.kinds], theme: opts.theme, details })
  : generateCollapseHTML(hierarchy, { title, subtitle, workingDir, legendKinds: [...opts.kinds], theme: opts.theme, details });
const safeName = (opts.root || 'index').replace(/[^a-zA-Z0-9_-]/g, '_');
const artifactFile = saveArtifact(html, `tree-${safeName}`, workingDir);
console.log(`\nTree saved to: ${artifactFile}`);
if (opts.open) openFile(artifactFile);
