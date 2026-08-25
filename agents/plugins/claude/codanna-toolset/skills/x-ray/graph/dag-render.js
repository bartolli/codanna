// 2D layered DAG of a symbol's neighbourhood: call direction as the y-axis
// (callers above, callees below), longest-path layering with DFS cycle
// breaking, barycenter ordering, label-width-aware placement. Labels at rest,
// deterministic for a given graph; design tokens + the runtime theme toggle.
const fs = require('fs');
const path = require('path');
const { themeStyle, themeScript, detailScript, kindVars } = require('./theme');

const D3 = path.join(__dirname, 'vendor', 'd3.min.js');

/**
 * graphData: nodes {id,name,kind,file,line,module,signature,level}, links
 * {source,target,type} with source -> target = caller -> callee.
 */
function generateDAGHTML(graphData, { title, subtitle = '', workingDir = '', theme = 'dark' } = {}) {
  const d3src = fs.readFileSync(D3, 'utf8');
  const kinds = [...new Set(graphData.nodes.map(n => n.kind).filter(Boolean))];
  const legend = kinds.map(k => `<span class="legend-item"><span class="dot" style="background:${kindVars[k] || 'var(--n3)'}"></span>${k}</span>`).join('');
  return `<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>${title}</title>
  ${themeStyle()}
  <style>
    html, body { margin: 0; height: 100%; }
    #chart { position: absolute; inset: 0; overflow: hidden; }
    #chart svg { width: 100%; height: 100%; cursor: grab; }
    #chart text { cursor: pointer; }
  </style>
</head>
<body>
  <div id="chart"></div>
  ${themeScript(theme)}
  ${detailScript()}
  <div id="info">
    <h3>${title}</h3>
    ${subtitle ? `<div class="stat">${subtitle}</div>` : ''}
    <div class="stat" id="stats"></div>
    <div class="legend"><span class="legend-item"><span class="swatch" style="background:var(--accent)"></span>incoming (callers)</span><span class="legend-item"><span class="swatch" style="background:var(--g9)"></span>outgoing (callees)</span></div>
    <div class="legend">${legend}</div>
    <button id="reset-btn">Reset zoom</button>
  </div>
  <div id="detail"></div>
  <div id="hint">Rows are call distance from the focus: callers above, callees below. Hover: edges. Click: details. Dashed: lateral or cycle edge</div>
  <script>${d3src}</script>
  <script>
    const data = ${JSON.stringify(graphData)};
    const workingDir = ${JSON.stringify(workingDir)};
    const KVAR = ${JSON.stringify(kindVars)};

    const idx = new Map(data.nodes.map((n, i) => [n.id, i]));
    const N = data.nodes.length;
    // One edge per (s, t) pair for layout; multiplicity and relations in the title.
    const pair = new Map();
    for (const l of data.links) {
      const s = idx.get(l.source), t = idx.get(l.target);
      if (s === undefined || t === undefined || s === t) continue;
      const k = s + ' ' + t;
      if (!pair.has(k)) pair.set(k, { s, t, n: 0, types: new Set(), rev: false });
      const e = pair.get(k); e.n += 1; e.types.add(l.type || 'Calls');
    }
    const edges = [...pair.values()];

    // Ego layering: signed BFS distance from the focus symbol. Direct callers
    // sit exactly one row above the focus, their callers above them, callees
    // mirrored below -- the IDE call-hierarchy shape. (Longest-path layering
    // over the whole neighbourhood scattered direct callers across arbitrary
    // rows whenever caller-of-caller chains existed.) BFS order wins ties;
    // a node reached through an out-edge lands one row below its finder,
    // through an in-edge one row above.
    const outAdj = Array.from({ length: N }, () => []), inAdj = Array.from({ length: N }, () => []);
    edges.forEach(e => { outAdj[e.s].push(e.t); inAdj[e.t].push(e.s); });
    const focus = data.nodes.findIndex(n => n.level === 0);
    const dist = new Array(N).fill(null);
    const q = [];
    if (focus >= 0) { dist[focus] = 0; q.push(focus); }
    while (q.length) {
      const u = q.shift();
      for (const v of outAdj[u]) if (dist[v] === null) { dist[v] = dist[u] + 1; q.push(v); }
      for (const v of inAdj[u]) if (dist[v] === null) { dist[v] = dist[u] - 1; q.push(v); }
    }
    let minD = 0, maxD = 0;
    dist.forEach(d => { if (d !== null) { minD = Math.min(minD, d); maxD = Math.max(maxD, d); } });
    const layer = dist.map(d => (d === null ? maxD - minD + 1 : d - minD));
    const L = Math.max(...layer, 0) + 1;
    const layers = Array.from({ length: L }, () => []);
    for (let i = 0; i < N; i++) layers[layer[i]].push(i);
    // Lateral and upward edges (same row, or against the flow: cycles and
    // sibling links) draw dashed; everything else points downward.
    edges.forEach(e => { e.rev = layer[e.s] >= layer[e.t]; });
    const cyc = edges.filter(e => e.rev).length;

    // Barycenter ordering, four alternating sweeps.
    const pos = new Array(N).fill(0);
    const setPos = () => layers.forEach(l => l.forEach((n, i) => pos[n] = i));
    const up = Array.from({ length: N }, () => []), down = Array.from({ length: N }, () => []);
    edges.forEach(e => {
      if (layer[e.s] === layer[e.t]) return;
      const [a, b] = layer[e.s] < layer[e.t] ? [e.s, e.t] : [e.t, e.s];
      down[a].push(b); up[b].push(a);
    });
    setPos();
    for (let sweep = 0; sweep < 4; sweep++) {
      const dirs = sweep % 2 === 0 ? up : down;
      const order = sweep % 2 === 0 ? layers : [...layers].reverse();
      for (const l of order) {
        l.sort((a, b) => {
          const ba = dirs[a].length ? dirs[a].reduce((s, n) => s + pos[n], 0) / dirs[a].length : pos[a];
          const bb = dirs[b].length ? dirs[b].reduce((s, n) => s + pos[n], 0) / dirs[b].length : pos[b];
          return ba - bb || a - b;
        });
        setPos();
      }
    }

    // Placement: x from cumulative label widths (labels never overlap at rest).
    // A wide layer wraps into rows of at most MAXW, so one 100-node layer does
    // not stretch the whole picture into a ribbon; rows of a layer sit in one
    // band, tighter than the gap between layers.
    const bodyFont = getComputedStyle(document.body).fontFamily;
    const meas = document.createElement('canvas').getContext('2d');
    meas.font = '11px ' + bodyFont;
    const labelOf = n => n.name;
    const X = new Array(N).fill(0), Y = new Array(N).fill(0);
    const GAP = 34, ROWP = 24, BANDGAP = 72, R = 4.5, MAXW = 1500;
    let maxW = 0, yBase = 0, totalH = 0;
    layers.forEach((l) => {
      const rows = [[]];
      let x = 0;
      for (const n of l) {
        const w = 14 + meas.measureText(labelOf(data.nodes[n])).width;
        if (x > 0 && x + w > MAXW) { rows.push([]); x = 0; }
        rows[rows.length - 1].push({ n, x, w }); x += w + GAP;
      }
      rows.forEach((row, ri) => {
        const rw = row.length ? row[row.length - 1].x + row[row.length - 1].w : 0;
        maxW = Math.max(maxW, rw);
        const off = -rw / 2;
        row.forEach(s => { X[s.n] = s.x + off; Y[s.n] = yBase + ri * ROWP; });
      });
      totalH = yBase + (rows.length - 1) * ROWP;
      yBase = totalH + BANDGAP;
    });

    // The viewBox stays 1:1 with the viewport, so screen-pixel zoom math (the
    // go-button pan) is exact; the content-sized viewBox letterboxed and sent
    // every pan off the canvas.
    const svg = d3.create('svg')
        .attr('viewBox', [0, 0, window.innerWidth, window.innerHeight])
        .attr('font-size', 11)
        .style('font-family', 'var(--font-ui)');
    const wrapper = svg.append('g');

    const epath = (e) => {
      const x1 = X[e.s], y1 = Y[e.s] + R, x2 = X[e.t], y2 = Y[e.t] - R;
      // Same-row edges bow below their row instead of cutting through it.
      const my = (y1 + y2) / 2 + (Math.abs(y1 - y2) < 1 ? 30 : 0);
      return 'M' + x1 + ',' + y1 + 'C' + x1 + ',' + my + ' ' + x2 + ',' + my + ' ' + x2 + ',' + y2;
    };
    const linkSel = wrapper.append('g')
        .attr('fill', 'none')
        .style('stroke', 'color-mix(in srgb, var(--n1) 45%, transparent)')
      .selectAll('path')
      .data(edges)
      .join('path')
        .attr('stroke-width', e => Math.min(4, 0.8 + Math.log2(e.n)))
        .attr('stroke-dasharray', e => e.rev ? '4 3' : null)
        .attr('d', epath)
        .each(function(e) { e.el = this; })
        .call(p => p.append('title').text(e => [...e.types].join(', ') + (e.n > 1 ? ' x' + e.n : '')));

    const inEdges = Array.from({ length: N }, () => []), outEdges = Array.from({ length: N }, () => []);
    edges.forEach(e => { outEdges[e.s].push(e); inEdges[e.t].push(e); });

    const node = wrapper.append('g')
      .selectAll('g')
      .data(data.nodes.map((n, i) => ({ n, i })))
      .join('g')
        .attr('transform', d => 'translate(' + X[d.i] + ',' + Y[d.i] + ')')
        .on('mouseover', overed)
        .on('mouseout', outed)
        .on('click', (ev, d) => showDetail(d.i));

    node.append('circle')
        .attr('r', d => d.n.level === 0 ? 6.5 : R)
        .style('fill', d => d.n.level === 0 ? 'var(--text-1)' : (KVAR[d.n.kind] || 'var(--n3)'))
        .style('stroke', d => d.n.level === 0 ? 'var(--accent)' : null)
        .attr('stroke-width', d => d.n.level === 0 ? 2 : null);

    node.append('text')
        .attr('x', 9)
        .attr('dy', '0.32em')
        .text(d => labelOf(d.n))
        .attr('font-weight', d => d.n.level === 0 ? 700 : null)
        .style('fill', 'var(--text-1)')
        .style('stroke', 'var(--surface-0)')
        .attr('stroke-width', 3)
        .attr('paint-order', 'stroke')
        .each(function(d) { d.text = this; });

    node.append('title')
        .text(d => d.n.name + (d.n.kind ? String.fromCharCode(10) + d.n.kind + (d.n.file ? '  ' + d.n.file + ':' + d.n.line : '') : '')
          + (d.n.signature ? String.fromCharCode(10) + d.n.signature : ''));

    function overed(ev, d) {
      d3.selectAll(inEdges[d.i].map(e => e.el)).style('stroke', 'var(--accent)').raise();
      d3.selectAll(outEdges[d.i].map(e => e.el)).style('stroke', 'var(--g9)').raise();
      d3.select(this).select('text').attr('font-weight', 700);
    }
    function outed(ev, d) {
      d3.selectAll(inEdges[d.i].map(e => e.el)).style('stroke', null);
      d3.selectAll(outEdges[d.i].map(e => e.el)).style('stroke', null);
      d3.select(this).select('text').attr('font-weight', d.n.level === 0 ? 700 : null);
    }

    // The disc sidebar: relation groups from this page's own edges; go-buttons
    // pan to the related node and reopen the panel there.
    const REL_LABEL = { Calls: ['Calls', 'Called by'], Uses: ['Uses', 'Used by'], Implements: ['Implements', 'Implemented by'], Extends: ['Extends', 'Extended by'], Defines: ['Defines', 'Defined in'] };
    // Link types are walk provenance ('calls', 'calledBy', ...); the edge is
    // already oriented caller -> callee, so provenance folds into the canonical
    // relation and the heading direction comes from which side this node is on.
    const CANON = { calls: 'Calls', calledby: 'Calls', uses: 'Uses', usedby: 'Uses',
                    defines: 'Defines', definedby: 'Defines', implements: 'Implements',
                    implementedby: 'Implements', extends: 'Extends', extendedby: 'Extends' };
    function showDetail(i) {
      const n = data.nodes[i];
      const rels = {};
      let deg = 0;
      const addRows = (list, dir, otherOf) => {
        for (const e of list) {
          deg += e.n;
          const seen = new Set();
          for (const t of e.types) {
            const canon = CANON[String(t).toLowerCase().replace(/[^a-z]/g, '')] || t;
            if (seen.has(canon)) continue;
            seen.add(canon);
            const lab = (REL_LABEL[canon] || [canon, canon + ' (in)'])[dir];
            (rels[lab] ||= []).push({ name: data.nodes[otherOf(e)].name, k: e.n, ref: otherOf(e) });
          }
        }
      };
      addRows(outEdges[i], 0, e => e.t);
      addRows(inEdges[i], 1, e => e.s);
      window.__detail.show({
        name: n.name,
        dotted: n.module || '',
        kind: n.kind, visibility: n.visibility, language: n.language,
        edges: deg,
        lines: n.endLine >= n.line ? n.endLine - n.line + 1 : undefined,
        path: n.file ? n.file + ':' + (n.line + 1) + '-' + (n.endLine + 1) : '',
        signature: n.signature,
        rels,
        href: n.file && workingDir ? 'file://' + workingDir + '/' + n.file : '',
      }, (ref) => {
        const j = +ref;
        const k = d3.zoomTransform(svg.node()).k;
        svg.transition().duration(400).call(zoom.transform,
          d3.zoomIdentity.translate(window.innerWidth / 2 - k * X[j], window.innerHeight / 2 - k * Y[j]).scale(k));
        showDetail(j);
      });
    }

    const zoom = d3.zoom().scaleExtent([0.05, 12]).on('zoom', (e) => wrapper.attr('transform', e.transform));
    const vw = window.innerWidth, vh = window.innerHeight;
    const fit = Math.min((vw - 140) / (maxW + 60), (vh - 160) / (totalH + 60), 1);
    const home = d3.zoomIdentity.translate(vw / 2, (vh - fit * totalH) / 2).scale(fit);
    document.getElementById('chart').appendChild(svg.node());
    svg.call(zoom).call(zoom.transform, home);
    document.getElementById('reset-btn').addEventListener('click', () => svg.transition().duration(500).call(zoom.transform, home));
    document.getElementById('stats').textContent =
      'Nodes: ' + N + '  Edges: ' + data.links.length + '  Rows: ' + L + (cyc ? '  Lateral edges: ' + cyc : '');
  </script>
</body>
</html>`;
}

module.exports = { generateDAGHTML };
