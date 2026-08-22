// Shared hierarchy builder: module path segments -> container (Defines) ->
// member, flare-shaped {name, children}. Used by the radial tree and the
// edge-bundling renderers. Every node gets a stable `key`; `repOf` maps each
// placed symbol id to the key of the LEAF that represents it (its own leaf,
// or the collapsed ancestor under --depth).

const CONTAINERS = new Set(['Struct', 'Class', 'Trait', 'Enum', 'Interface']);
const DEFAULT_KINDS = ['Function', 'Method', 'Struct', 'Class', 'Trait', 'Enum', 'Interface', 'TypeAlias', 'Constant', 'Macro'];

/** Module path -> segments, across the languages' conventions. */
function segmentsOf(sym) {
  const mp = sym.module;
  if (mp) {
    const cleaned = mp.replace(/^res:\/\//, '').replace(/^\.\//, '').replace(/^\\+/, '');
    return cleaned.split(/::|\.|\/|\\/).filter(Boolean);
  }
  const p = sym.file.replace(/^\.\//, '');
  const parts = p.split('/');
  const last = parts.pop() || '';
  return [...parts, last.replace(/\.[^.]+$/, '')].filter(Boolean);
}

function normalizePrefix(prefix) {
  return prefix.replace(/^res:\/\//, '').replace(/^\.\//, '').replace(/^\\+/, '').split(/::|\.|\/|\\/).filter(Boolean);
}

function buildHierarchy(graph, opts) {
  const repOf = new Map();
  const prefix = opts.root ? normalizePrefix(opts.root) : [];
  const rootName = prefix.length ? prefix.join('.') : 'index';
  const root = { name: rootName, children: new Map() };
  const internal = (node, name) => {
    let child = node.children.get(name);
    if (!child) { child = { name, children: new Map() }; node.children.set(name, child); }
    return child;
  };
  const underPrefix = (segs) => prefix.every((p, i) => segs[i] === p);

  // Which members sit in a container: Defines container -> member.
  const containerOf = new Map();
  for (const [from, byRel] of graph.out) {
    for (const to of (byRel.Defines || [])) {
      const c = graph.symbols.get(from);
      if (c && CONTAINERS.has(c.kind)) containerOf.set(to, from);
    }
  }

  const leafOf = new Map(); // symbol id -> tree node (containers are internal nodes with symbol data)
  const placed = [];
  for (const sym of graph.symbols.values()) {
    if (sym.kind === 'Module') continue;
    const segs = segmentsOf(sym);
    if (!underPrefix(segs)) continue;
    placed.push([sym, segs.slice(prefix.length)]);
  }
  // containers first so members find them
  placed.sort((a, b) => (CONTAINERS.has(a[0].kind) ? 0 : 1) - (CONTAINERS.has(b[0].kind) ? 0 : 1));
  const leafData = (sym) => ({ name: sym.name, kind: sym.kind, file: sym.file, line: sym.line + 1, sid: sym.id });
  let leaves = 0;
  for (const [sym, segs] of placed) {
    const isContainer = CONTAINERS.has(sym.kind);
    if (!isContainer && !opts.kinds.has(sym.kind)) continue;
    let parent = root;
    const cid = containerOf.get(sym.id);
    if (cid && leafOf.has(cid)) {
      parent = leafOf.get(cid);
    } else {
      for (const s of segs) parent = internal(parent, s);
    }
    if (isContainer) {
      const node = { ...leafData(sym), children: new Map() };
      parent.children.set(`${sym.kind}:${sym.name}#${sym.id}`, node);
      leafOf.set(sym.id, node);
    } else {
      parent.children.set(`${sym.kind}:${sym.name}#${sym.id}`, leafData(sym));
      leaves += 1;
    }
  }

  // Map -> arrays, sort, collapse beyond depth.
  function finish(node, depth, key) {
    node.key = key;
    if (!node.children) {
      if (node.sid !== undefined) repOf.set(node.sid, key);
      return { node, leaves: 1, sids: node.sid !== undefined ? [node.sid] : [] };
    }
    const kids = [...node.children.entries()];
    node.children = [];
    let below = 0;
    const sids = node.sid !== undefined ? [node.sid] : [];
    for (const [k, child] of kids) {
      const r = finish(child, depth + 1, key + '/' + k);
      below += r.leaves;
      sids.push(...r.sids);
      node.children.push(r.node);
    }
    node.children.sort((a, b) => (a.children ? 0 : 1) - (b.children ? 0 : 1) || a.name.localeCompare(b.name));
    const collapse = depth >= opts.depth || node.children.length === 0;
    if (collapse) {
      delete node.children;
      if (depth >= opts.depth && below > 1) node.count = below;
      for (const s of sids) repOf.set(s, key);
      return { node, leaves: 1, sids };
    }
    if (node.sid !== undefined) repOf.set(node.sid, null); // container with members: internal, no leaf
    return { node, leaves: below, sids };
  }
  const done = finish(root, 0, 'root');
  return { hierarchy: done.node, leaves: done.leaves, containers: leafOf.size, symbolLeaves: leaves, repOf };
}


module.exports = { buildHierarchy, segmentsOf, normalizePrefix, CONTAINERS, DEFAULT_KINDS };
