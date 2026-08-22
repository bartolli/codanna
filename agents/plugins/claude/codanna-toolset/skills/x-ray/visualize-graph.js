#!/usr/bin/env node
/**
 * Generate a 3D force graph visualization of symbol relationships from Codanna.
 *
 * Usage:
 *   node visualize-graph.js <symbol_id_or_name> [depth]
 *   node visualize-graph.js symbol_id:6695 2
 *   node visualize-graph.js uniform 3
 *
 * Output: Opens visualization in browser or saves to .codanna/visualizations/
 */

const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const workingDir = process.env.CLAUDE_PROJECT_DIR || process.cwd();
const { generateHTML } = require('./graph/render');
const { saveArtifact, serveAndOpen } = require('./graph/publish');

function runCodanna(subcommand, args) {
  const cmd = `codanna ${subcommand} ${args} --json`;
  try {
    const output = execSync(cmd, {
      encoding: 'utf8',
      stdio: ['pipe', 'pipe', 'pipe'],
      cwd: workingDir
    });
    return JSON.parse(output);
  } catch (error) {
    if ((error.status === 1 || error.status === 3) && error.stdout) {
      return JSON.parse(error.stdout);
    }
    throw error;
  }
}

function parseArgs() {
  const args = process.argv.slice(2);
  if (args.length < 1) {
    console.error('Usage: node visualize-graph.js <symbol_id_or_name> [depth]');
    console.error('  symbol_id:6695  - Use symbol ID directly');
    console.error('  uniform         - Search by name');
    process.exit(1);
  }
  return {
    symbol: args[0],
    depth: parseInt(args[1] || '2', 10)
  };
}

/**
 * Extract symbol data from JSON response (Envelope format)
 */
function extractSymbolData(response) {
  if (response.status !== 'success') return null;

  // Envelope format: data is array or single object
  const data = response.data;
  const item = Array.isArray(data) ? data[0] : data;
  if (!item) return null;

  const { symbol, file_path, relationships } = item;

  // Parse line from file_path (format: path:line or path:start-end)
  let line = symbol.range?.start_line || 0;
  let endLine = symbol.range?.end_line || line;

  return {
    id: symbol.id,
    name: symbol.name,
    kind: symbol.kind,
    file: file_path || '',
    line,
    endLine,
    module: symbol.module_path || '',
    signature: symbol.signature || '',
    visibility: symbol.visibility || '',
    doc: symbol.doc_comment || '',
    language: symbol.language_id || '',
    // All relationship types
    calls: extractRelationships(relationships?.calls),
    callers: extractRelationships(relationships?.called_by),
    defines: extractRelationships(relationships?.defines),
    implements: extractRelationships(relationships?.implements),
    implementedBy: extractRelationships(relationships?.implemented_by),
    extends: extractRelationships(relationships?.extends),
    extendedBy: extractRelationships(relationships?.extended_by),
    uses: extractRelationships(relationships?.uses),
    usedBy: extractRelationships(relationships?.used_by)
  };
}

/**
 * Extract relationship entries from JSON
 */
function extractRelationships(items) {
  if (!items || !Array.isArray(items)) return [];

  return items.map(item => {
    // Handle both [symbol, ref] tuples and plain objects
    const sym = Array.isArray(item) ? item[0] : item;
    if (!sym) return null;

    return {
      id: sym.id,
      name: sym.name,
      kind: sym.kind,
      file: sym.file_path || '',
      line: sym.range?.start_line || 0,
      module: sym.module_path || '',
      signature: sym.signature || '',
      language: sym.language_id || ''
    };
  }).filter(Boolean);
}

/**
 * Get symbol by ID using JSON API
 */
function getSymbolById(symbolId) {
  try {
    const response = runCodanna('retrieve describe', `symbol_id:${symbolId}`);
    return extractSymbolData(response);
  } catch (e) {
    return null;
  }
}

/**
 * Find symbol by name, returns the one with most relationships
 */
function findSymbolByName(name) {
  try {
    const response = runCodanna('retrieve symbol', name);
    if (response.status !== 'success') return null;

    // Envelope format: data is array
    const data = response.data;
    // If multiple matches, try to find one with relationships
    if (Array.isArray(data) && data.length > 1) {
      for (const item of data) {
        const detailed = getSymbolById(item.symbol.id);
        if (detailed && (detailed.calls.length > 0 || detailed.callers.length > 0)) {
          return detailed;
        }
      }
    }

    return extractSymbolData(response);
  } catch (e) {
    return null;
  }
}

function buildGraph(rootSymbol, depth) {
  const nodes = new Map();
  const links = [];
  const visited = new Set();

  function getGroup(kind) {
    switch (kind?.toLowerCase()) {
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

  function addNode(sym, level) {
    if (!sym.id) return;
    const key = sym.id;
    if (!nodes.has(key)) {
      nodes.set(key, {
        id: key,
        name: sym.name,
        kind: sym.kind,
        file: sym.file,
        line: sym.line,
        endLine: sym.endLine || sym.line,
        module: sym.module || '',
        signature: sym.signature || '',
        visibility: sym.visibility || '',
        language: sym.language || '',
        group: getGroup(sym.kind),
        level
      });
    }
    return key;
  }

  function explore(symbolId, level) {
    if (level > depth) return;
    if (visited.has(symbolId)) return;
    visited.add(symbolId);

    const sym = getSymbolById(symbolId);
    if (!sym) return;

    addNode(sym, level);

    // Helper to add relationships with limits
    function addRelationships(items, type, direction, maxItems) {
      const limit = level === 0 ? maxItems : Math.min(maxItems, 5);
      for (const item of items.slice(0, limit)) {
        if (item.id) {
          addNode(item, level + 1);
          links.push({
            source: direction === 'out' ? sym.id : item.id,
            target: direction === 'out' ? item.id : sym.id,
            type
          });
          if (level + 1 <= depth) {
            explore(item.id, level + 1);
          }
        }
      }
    }

    // Outgoing relationships (this symbol → target)
    addRelationships(sym.calls, 'calls', 'out', 20);
    addRelationships(sym.defines, 'defines', 'out', 30);
    addRelationships(sym.implements, 'implements', 'out', 10);
    addRelationships(sym.extends, 'extends', 'out', 5);
    addRelationships(sym.uses, 'uses', 'out', 10);

    // Incoming relationships (source → this symbol)
    addRelationships(sym.callers, 'calledBy', 'in', 20);
    addRelationships(sym.implementedBy, 'implementedBy', 'in', 20);
    addRelationships(sym.extendedBy, 'extendedBy', 'in', 10);
    addRelationships(sym.usedBy, 'usedBy', 'in', 10);
  }

  // Start exploration from root
  let rootId;
  if (rootSymbol.startsWith('symbol_id:')) {
    rootId = parseInt(rootSymbol.replace('symbol_id:', ''), 10);
  } else {
    const sym = findSymbolByName(rootSymbol);
    if (!sym) {
      console.error(`Symbol not found: ${rootSymbol}`);
      process.exit(1);
    }
    rootId = sym.id;
  }

  explore(rootId, 0);

  // Deduplicate links
  const uniqueLinks = [];
  const linkSet = new Set();
  for (const link of links) {
    const key = `${link.source}->${link.target}`;
    if (!linkSet.has(key)) {
      linkSet.add(key);
      uniqueLinks.push(link);
    }
  }

  return {
    nodes: Array.from(nodes.values()),
    links: uniqueLinks
  };
}

// Main
const { symbol, depth } = parseArgs();
const open = !process.argv.includes('--no-open');
console.log(`Building relationship graph for: ${symbol} (depth: ${depth})`);

const graphData = buildGraph(symbol, depth);
console.log(`Found ${graphData.nodes.length} nodes and ${graphData.links.length} links`);

if (graphData.nodes.length === 0) {
  console.error('No data found. Check if the symbol exists.');
  process.exit(1);
}

const safeName = symbol.replace(/[^a-zA-Z0-9_-]/g, '_');
const html = generateHTML(graphData);
const artifactFile = saveArtifact(html, safeName, workingDir);
console.log(`\nVisualization saved to: ${artifactFile}`);
serveAndOpen(html, __dirname, { open });
