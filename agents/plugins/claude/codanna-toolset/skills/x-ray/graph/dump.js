// Shared reader for `codanna dump` (JSON Lines envelope stream).
// Returns symbols (id -> node record), out/inc adjacency (id -> relation ->
// [ids], sorted by id), and the summary envelope's data.
const fs = require('fs');
const { execSync } = require('child_process');

/** Read the dump stream: one Envelope per line; begin / result* / summary. */
function readDump({ from, binary, workingDir = process.cwd() }) {
  const text = from
    ? fs.readFileSync(from, 'utf8')
    : execSync(`${binary} dump`, { cwd: workingDir, encoding: 'utf8', maxBuffer: 1 << 30, stdio: ['pipe', 'pipe', 'pipe'] });
  const symbols = new Map();       // id -> node record
  const out = new Map();           // id -> { relation -> [to ids] }
  const inc = new Map();           // id -> { relation -> [from ids] }
  let summary = null;
  const push = (map, id, relation, other) => {
    let byRel = map.get(id);
    if (!byRel) { byRel = {}; map.set(id, byRel); }
    (byRel[relation] ||= []).push(other);
  };
  for (const line of text.split('\n')) {
    if (!line) continue;
    const env = JSON.parse(line);
    if (env.type === 'summary') { summary = env.data; continue; }
    if (env.type !== 'result') continue;
    const entity = env.meta && env.meta.entity_type;
    const d = env.data;
    if (entity === 'symbol') {
      symbols.set(d.id, {
        id: d.id,
        name: d.name,
        kind: d.kind,
        file: d.file_path || '',
        line: (d.range && d.range.start_line) || 0,
        endLine: (d.range && d.range.end_line) || 0,
        module: d.module_path || '',
        signature: d.signature || '',
        visibility: d.visibility || '',
        language: d.language_id || '',
        cls: (d.scope_context && d.scope_context.ClassMember && d.scope_context.ClassMember.class_name) || ''
      });
    } else if (entity === 'relationship') {
      push(out, d.from.id, d.relation, d.to.id);
      push(inc, d.to.id, d.relation, d.from.id);
    }
  }
  if (!summary) throw new Error('dump stream has no summary line (truncated?)');
  // The dump promises no row order; sort every adjacency list by symbol id
  // so the capped neighborhood is deterministic for a given index (ids are
  // allocated in indexing order, the closest analogue of storage order).
  for (const table of [out, inc]) {
    for (const byRel of table.values()) {
      for (const list of Object.values(byRel)) list.sort((x, y) => x - y);
    }
  }
  return { symbols, out, inc, summary };
}

module.exports = { readDump };
