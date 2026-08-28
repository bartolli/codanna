// Per-symbol detail specs for the shared panel (theme.js detailScript):
// signature, span, edge count, and relation groups from the dump's out/inc
// adjacency. `nodeSids` bounds both which symbols get a spec and which related
// rows carry a go-ref; relations to symbols outside the set render as plain
// rows in the panel.
const { qualify, labelOf } = require('./qualify');

const REL_LABEL = { Calls: ['Calls', 'Called by'], Uses: ['Uses', 'Used by'], Implements: ['Implements', 'Implemented by'], Extends: ['Extends', 'Extended by'], Defines: ['Defines', 'Defined in'] };

function buildDetails(g, nodeSids) {
  // Same-name disambiguation over every symbol a panel can show (the tree's
  // own nodes plus every related row), one pass, same rule as the DAG canvas.
  const involved = new Map();
  for (const sid of nodeSids) {
    const sym = g.symbols.get(sid);
    if (!sym) continue;
    involved.set(sid, sym);
    for (const table of [g.out, g.inc]) {
      const byRel = table.get(sid);
      if (!byRel) continue;
      for (const ids of Object.values(byRel)) {
        for (const oid of ids) {
          const o = g.symbols.get(oid);
          if (o) involved.set(oid, o);
        }
      }
    }
  }
  qualify([...involved.values()]);

  const details = {};
  for (const sid of nodeSids) {
    const sym = g.symbols.get(sid);
    if (!sym) continue;
    const rels = {};
    let edges = 0;
    for (const [table, dir] of [[g.out, 0], [g.inc, 1]]) {
      const byRel = table.get(sid);
      if (!byRel) continue;
      for (const [rel, ids] of Object.entries(byRel)) {
        const lab = (REL_LABEL[rel] || [rel, rel + ' (in)'])[dir];
        const counts = new Map();
        for (const oid of ids) counts.set(oid, (counts.get(oid) || 0) + 1);
        for (const [oid, k] of counts) {
          const o = g.symbols.get(oid);
          if (!o) continue;
          (rels[lab] ||= []).push({ name: labelOf(o), k, ...(nodeSids.has(oid) ? { ref: oid } : {}) });
          edges += k;
        }
      }
    }
    details[sid] = { label: labelOf(sym), sig: sym.signature || '', vis: sym.visibility || '', lang: sym.language || '',
                     line: sym.line + 1, endLine: sym.endLine + 1, edges, rels };
  }
  return details;
}

/** Symbol ids that own a node in a flare-shaped hierarchy. */
function sidsInTree(root) {
  const out = new Set();
  (function walk(n) { if (n.sid !== undefined) out.add(n.sid); (n.children || []).forEach(walk); })(root);
  return out;
}

/** Symbol ids of LEAF nodes only -- the ref vocabulary for pages that can
 *  navigate solely to rim leaves (the bundle): a ref to an internal node
 *  would render a go-button that goes nowhere. */
function leafSidsInTree(root) {
  const out = new Set();
  (function walk(n) {
    if (n.children && n.children.length) n.children.forEach(walk);
    else if (n.sid !== undefined) out.add(n.sid);
  })(root);
  return out;
}

module.exports = { buildDetails, sidsInTree, leafSidsInTree, REL_LABEL };
