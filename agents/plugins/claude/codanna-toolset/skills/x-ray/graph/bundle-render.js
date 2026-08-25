// Hierarchical edge bundling page (after https://observablehq.com/@d3/hierarchical-edge-bundling,
// ISC): radial cluster of the code hierarchy, dependency edges as bundled
// arcs between leaves. Pure d3/SVG, self-contained, pan/zoom, dark theme.
const fs = require('fs');
const path = require('path');
const { themeStyle, themeScript, detailScript, kindVars } = require('./theme');

const D3 = path.join(__dirname, 'vendor', 'd3.min.js');

/**
 * hierarchy: flare-shaped tree whose nodes carry `key` (and leaves `kind`, `file`, `line`, `count`).
 * edges: [{ source: leafKey, target: leafKey, count }] between LEAF keys.
 */
function generateBundleHTML(hierarchy, edges, { title, subtitle = '', workingDir = '', leaves = 0, relation = 'Calls', legendKinds = [], theme = 'dark', details = {} } = {}) {
  const d3src = fs.readFileSync(D3, 'utf8');
  const radius = Math.max(520, Math.round(leaves * 11 / (2 * Math.PI)));
  const width = 2 * radius;
  const legend = legendKinds.map(k => `<span class="legend-item"><span class="dot" style="background:${kindVars[k] || 'var(--n3)'}"></span>${k}</span>`).join('');
  const inLabel = relation === 'Calls' ? 'callers (incoming)' : 'incoming';
  const outLabel = relation === 'Calls' ? 'callees (outgoing)' : 'outgoing';
  const rel = relation.toLowerCase();
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
    text { cursor: pointer; }
  </style>
</head>
<body>
  <div id="chart"></div>
  ${themeScript(theme)}
  ${detailScript()}
  <div id="detail"></div>
  <div id="info">
    <h3>${title}</h3>
    ${subtitle ? `<div class="stat">${subtitle}</div>` : ''}
    <div class="stat" id="stats"></div>
    <div class="legend"><span class="legend-item"><span class="swatch" style="background:var(--accent)"></span>${inLabel}</span><span class="legend-item"><span class="swatch" style="background:var(--g9)"></span>${outLabel}</span></div>
    <div class="legend">${legend}</div>
    <button id="reset-btn">Reset zoom</button>
  </div>
  <div id="hint">Scroll: zoom, Drag: pan. Hover a name: its ${rel} in and out. Click: details</div>
  <script>${d3src}</script>
  <script>
    const data = ${JSON.stringify(hierarchy)};
    const edgeList = ${JSON.stringify(edges)};
    const KVAR = ${JSON.stringify(kindVars)};
    const DETAILS = ${JSON.stringify(details)};
    const workingDir = ${JSON.stringify(workingDir)};
    // Incoming rides the accent, outgoing the red slot; the resting arc colour
    // mixes a neutral into transparency so both themes derive it from tokens.
    const colorin = 'var(--accent)', colorout = 'var(--g9)';
    const colornone = 'color-mix(in srgb, var(--n1) 30%, transparent)';
    const blend = () => document.documentElement.getAttribute('data-theme') === 'light' ? 'multiply' : 'screen';
    const relLabel = ${JSON.stringify(rel)};
    const width = ${width};
    const radius = width / 2;

    // Layout: radial cluster, leaves on the rim (after the Observable example).
    const tree = d3.cluster().size([2 * Math.PI, radius - 160]);
    const root = tree(d3.hierarchy(data)
        .sort((a, b) => d3.ascending(a.height, b.height) || d3.ascending(a.data.name, b.data.name)));

    // bilink by leaf key.
    const byKey = new Map(root.leaves().map(d => [d.data.key, d]));
    for (const d of root.leaves()) { d.incoming = []; d.outgoing = []; }
    let drawn = 0;
    for (const e of edgeList) {
      const s = byKey.get(e.source), t = byKey.get(e.target);
      if (!s || !t || s === t) continue;
      const pair = [s, t]; pair.count = e.count;
      s.outgoing.push(pair); t.incoming.push(pair); drawn += 1;
    }
    const line = d3.lineRadial().curve(d3.curveBundle.beta(0.85)).radius(d => d.y).angle(d => d.x);
    const id = (node) => (node.parent ? id(node.parent) + '.' : '') + node.data.name;
    const textColor = d => KVAR[d.data.kind] ? 'var(--text-1)' : 'var(--text-3)';
    const sum = (pairs) => pairs.reduce((n, p) => n + p.count, 0);
    const NL = String.fromCharCode(10);

    // 1:1 viewBox: screen-pixel zoom math stays exact (a content-sized
    // viewBox letterboxes and sends programmatic pans off the canvas).
    const vw = window.innerWidth, vh = window.innerHeight;
    const svg = d3.create('svg')
        .attr('viewBox', [0, 0, vw, vh])
        .attr('font-size', 10)
        .style('font-family', 'var(--font-ui)');
    const wrapper = svg.append('g');

    const node = wrapper.append('g')
      .selectAll()
      .data(root.leaves())
      .join('g')
        .attr('transform', d => 'rotate(' + (d.x * 180 / Math.PI - 90) + ') translate(' + d.y + ',0)')
      .append('text')
        .attr('dy', '0.31em')
        .attr('x', d => d.x < Math.PI ? 6 : -6)
        .attr('text-anchor', d => d.x < Math.PI ? 'start' : 'end')
        .attr('transform', d => d.x >= Math.PI ? 'rotate(180)' : null)
        .style('fill', textColor)
        .text(d => d.data.count ? d.data.name + ' (' + d.data.count + ')' : d.data.name)
        .each(function(d) { d.text = this; })
        .on('mouseover', overed)
        .on('mouseout', outed)
        .on('click', (ev, d) => showDetail(d))
        .call(text => text.append('title').text(d => id(d)
          + (d.data.kind ? NL + d.data.kind + (d.data.file ? '  ' + d.data.file + ':' + d.data.line : '') : '')
          + NL + sum(d.outgoing) + ' outgoing, ' + sum(d.incoming) + ' incoming'));

    // Kind dots at the rim, just inside the label.
    wrapper.append('g')
      .selectAll()
      .data(root.leaves().filter(d => d.data.kind))
      .join('circle')
        .attr('transform', d => 'rotate(' + (d.x * 180 / Math.PI - 90) + ') translate(' + d.y + ',0)')
        .attr('r', 2.2)
        .style('fill', d => KVAR[d.data.kind] || 'var(--n3)');

    const link = wrapper.append('g')
        .style('stroke', colornone)
        .attr('fill', 'none')
      .selectAll()
      .data(root.leaves().flatMap(leaf => leaf.outgoing))
      .join('path')
        .style('mix-blend-mode', blend())
        .attr('stroke-width', d => Math.min(4, 0.6 + Math.log2(d.count)))
        .attr('d', ([i, o]) => line(i.path(o)))
        .each(function(d) { d.path = this; });

    function overed(event, d) {
      link.style('mix-blend-mode', null);
      d3.select(this).attr('font-weight', 'bold');
      d3.selectAll(d.incoming.map(d => d.path)).style('stroke', colorin).raise();
      d3.selectAll(d.incoming.map(([d]) => d.text)).style('fill', colorin).attr('font-weight', 'bold');
      d3.selectAll(d.outgoing.map(d => d.path)).style('stroke', colorout).raise();
      d3.selectAll(d.outgoing.map(([, d]) => d.text)).style('fill', colorout).attr('font-weight', 'bold');
    }
    function outed(event, d) {
      link.style('mix-blend-mode', blend());
      d3.select(this).attr('font-weight', null);
      d3.selectAll(d.incoming.map(d => d.path)).style('stroke', null);
      d3.selectAll(d.incoming.map(([d]) => d.text)).style('fill', textColor).attr('font-weight', null);
      d3.selectAll(d.outgoing.map(d => d.path)).style('stroke', null);
      d3.selectAll(d.outgoing.map(([, d]) => d.text)).style('fill', textColor).attr('font-weight', null);
    }
    // Blend mode is the one colour decision CSS vars cannot carry.
    document.addEventListener('themechange', () => link.style('mix-blend-mode', blend()));

    // The shared panel; go-buttons pan to the related leaf on the rim.
    const bySid = new Map(root.leaves().filter(d => d.data.sid != null).map(d => [d.data.sid, d]));
    const cartesian = (d) => { const a = d.x - Math.PI / 2; return [d.y * Math.cos(a), d.y * Math.sin(a)]; };
    function showDetail(d) {
      const det = d.data.sid != null ? DETAILS[d.data.sid] : null;
      const spec = {
        name: d.data.name, dotted: id(d), kind: d.data.kind,
        visibility: det && det.vis, language: det && det.lang,
        edges: det ? det.edges : undefined,
        lines: det ? det.endLine - det.line + 1 : undefined,
        path: d.data.file ? d.data.file + ':' + (det ? det.line + '-' + det.endLine : d.data.line) : '',
        signature: det && det.sig, rels: det ? Object.assign({}, det.rels) : {},
        href: d.data.file && workingDir ? 'file://' + workingDir + '/' + d.data.file : '',
      };
      if (d.data.count) spec.rels['Collapsed inside'] = [{ name: d.data.count + ' symbols', k: 1 }];
      window.__detail.show(spec, (ref) => {
        const t = bySid.get(+ref);
        if (!t) return;
        const k = d3.zoomTransform(svg.node()).k;
        const [cx, cy] = cartesian(t);
        svg.transition().duration(400).call(zoom.transform,
          d3.zoomIdentity.translate(vw / 2 - k * cx, vh / 2 - k * cy).scale(k));
        showDetail(t);
      });
    }

    const zoom = d3.zoom().scaleExtent([0.05, 12]).on('zoom', (e) => wrapper.attr('transform', e.transform));
    const fit = Math.min(vw, vh) / (width + 80);
    const home = d3.zoomIdentity.translate(vw / 2, vh / 2).scale(fit);
    document.getElementById('chart').appendChild(svg.node());
    svg.call(zoom).call(zoom.transform, home);
    document.getElementById('reset-btn').addEventListener('click', () => svg.transition().duration(500).call(zoom.transform, home));
    document.getElementById('stats').textContent = 'Leaves: ' + root.leaves().length + '  Arcs: ' + drawn + '  (' + edgeList.reduce((n, e) => n + e.count, 0) + ' ' + relLabel + ' edges)';
  </script>
</body>
</html>`;
}

module.exports = { generateBundleHTML };
