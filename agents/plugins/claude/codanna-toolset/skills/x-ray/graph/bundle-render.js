// Hierarchical edge bundling page (after https://observablehq.com/@d3/hierarchical-edge-bundling,
// ISC): radial cluster of the code hierarchy, dependency edges as bundled
// arcs between leaves. Pure d3/SVG, self-contained, pan/zoom, dark theme.
const fs = require('fs');
const path = require('path');
const { THEMES, KIND_COLORS } = require('./tree-render');

const D3 = path.join(__dirname, 'vendor', 'd3.min.js');

/**
 * hierarchy: flare-shaped tree whose nodes carry `key` (and leaves `kind`, `file`, `line`, `count`).
 * edges: [{ source: leafKey, target: leafKey, count }] between LEAF keys.
 */
function generateBundleHTML(hierarchy, edges, { title, subtitle = '', workingDir = '', leaves = 0, relation = 'Calls', legendKinds = [], theme = 'dark' } = {}) {
  const d3src = fs.readFileSync(D3, 'utf8');
  const T = THEMES[theme] || THEMES.dark;
  const radius = Math.max(520, Math.round(leaves * 11 / (2 * Math.PI)));
  const width = 2 * radius;
  const legend = legendKinds.map(k => `<span class="legend-item"><span class="dot" style="background:${KIND_COLORS[k] || '#999'}"></span>${k}</span>`).join('');
  const colorIn = theme === 'dark' ? '#4cc2ff' : '#0044ff';
  const colorOut = theme === 'dark' ? '#ff6b6b' : '#e00000';
  const colorNone = theme === 'dark' ? 'rgba(138,148,166,0.28)' : 'rgba(160,160,160,0.45)';
  const blend = theme === 'dark' ? 'screen' : 'multiply';
  const inLabel = relation === 'Calls' ? 'callers (incoming)' : 'incoming';
  const outLabel = relation === 'Calls' ? 'callees (outgoing)' : 'outgoing';
  const rel = relation.toLowerCase();
  return `<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>${title}</title>
  <style>
    html, body { margin: 0; height: 100%; background: ${T.bg}; font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; color: ${T.text}; }
    #chart { position: absolute; inset: 0; overflow: hidden; }
    #chart svg { width: 100%; height: 100%; cursor: grab; }
    #info { position: absolute; top: 10px; left: 10px; background: ${T.panelBg}; border: 1px solid ${T.panelBorder}; border-radius: 8px; padding: 12px 14px; font-size: 13px; max-width: 400px; box-shadow: 0 2px 8px rgba(0,0,0,0.25); }
    #info h3 { margin: 0 0 8px; font-size: 14px; }
    #info .stat { margin: 3px 0; color: ${T.muted}; }
    #info .legend { margin-top: 8px; line-height: 1.8; }
    #info .legend-item { display: inline-block; margin-right: 10px; white-space: nowrap; }
    #info .dot { display: inline-block; width: 9px; height: 9px; border-radius: 50%; margin-right: 4px; vertical-align: middle; }
    #info .swatch { display: inline-block; width: 18px; height: 3px; margin-right: 4px; vertical-align: middle; }
    #info button { margin-top: 8px; padding: 4px 10px; background: ${T.button}; color: ${T.buttonText}; border: 1px solid ${T.panelBorder}; border-radius: 4px; cursor: pointer; font-size: 12px; }
    #hint { position: absolute; bottom: 8px; left: 50%; transform: translateX(-50%); font-size: 11px; color: ${T.hint}; }
    text { cursor: pointer; }
  </style>
</head>
<body>
  <div id="chart"></div>
  <div id="info">
    <h3>${title}</h3>
    ${subtitle ? `<div class="stat">${subtitle}</div>` : ''}
    <div class="stat" id="stats"></div>
    <div class="legend"><span class="legend-item"><span class="swatch" style="background:${colorIn}"></span>${inLabel}</span><span class="legend-item"><span class="swatch" style="background:${colorOut}"></span>${outLabel}</span></div>
    <div class="legend">${legend}</div>
    <button id="reset-btn">Reset zoom</button>
  </div>
  <div id="hint">Scroll: zoom, Drag: pan, Hover a name: its ${rel} in and out</div>
  <script>${d3src}</script>
  <script>
    const data = ${JSON.stringify(hierarchy)};
    const edgeList = ${JSON.stringify(edges)};
    const kindColors = ${JSON.stringify(KIND_COLORS)};
    const theme = ${JSON.stringify(T)};
    const colorin = ${JSON.stringify(colorIn)}, colorout = ${JSON.stringify(colorOut)}, colornone = ${JSON.stringify(colorNone)};
    const blend = ${JSON.stringify(blend)};
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
    const textColor = d => kindColors[d.data.kind] ? theme.text : theme.muted;
    const sum = (pairs) => pairs.reduce((n, p) => n + p.count, 0);
    const NL = String.fromCharCode(10);

    const svg = d3.create('svg')
        .attr('viewBox', [-width / 2, -width / 2, width, width])
        .attr('font-family', 'sans-serif')
        .attr('font-size', 10);
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
        .attr('fill', textColor)
        .text(d => d.data.count ? d.data.name + ' (' + d.data.count + ')' : d.data.name)
        .each(function(d) { d.text = this; })
        .on('mouseover', overed)
        .on('mouseout', outed)
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
        .attr('fill', d => kindColors[d.data.kind] || '#999');

    const link = wrapper.append('g')
        .attr('stroke', colornone)
        .attr('fill', 'none')
      .selectAll()
      .data(root.leaves().flatMap(leaf => leaf.outgoing))
      .join('path')
        .style('mix-blend-mode', blend)
        .attr('stroke-width', d => Math.min(4, 0.6 + Math.log2(d.count)))
        .attr('d', ([i, o]) => line(i.path(o)))
        .each(function(d) { d.path = this; });

    function overed(event, d) {
      link.style('mix-blend-mode', null);
      d3.select(this).attr('font-weight', 'bold');
      d3.selectAll(d.incoming.map(d => d.path)).attr('stroke', colorin).raise();
      d3.selectAll(d.incoming.map(([d]) => d.text)).attr('fill', colorin).attr('font-weight', 'bold');
      d3.selectAll(d.outgoing.map(d => d.path)).attr('stroke', colorout).raise();
      d3.selectAll(d.outgoing.map(([, d]) => d.text)).attr('fill', colorout).attr('font-weight', 'bold');
    }
    function outed(event, d) {
      link.style('mix-blend-mode', blend);
      d3.select(this).attr('font-weight', null);
      d3.selectAll(d.incoming.map(d => d.path)).attr('stroke', null);
      d3.selectAll(d.incoming.map(([d]) => d.text)).attr('fill', textColor).attr('font-weight', null);
      d3.selectAll(d.outgoing.map(d => d.path)).attr('stroke', null);
      d3.selectAll(d.outgoing.map(([, d]) => d.text)).attr('fill', textColor).attr('font-weight', null);
    }

    const zoom = d3.zoom().scaleExtent([0.05, 12]).on('zoom', (e) => wrapper.attr('transform', e.transform));
    svg.call(zoom);
    document.getElementById('chart').appendChild(svg.node());
    document.getElementById('reset-btn').addEventListener('click', () => svg.transition().duration(500).call(zoom.transform, d3.zoomIdentity));
    document.getElementById('stats').textContent = 'Leaves: ' + root.leaves().length + '  Arcs: ' + drawn + '  (' + edgeList.reduce((n, e) => n + e.count, 0) + ' ' + relLabel + ' edges)';
  </script>
</body>
</html>`;
}

module.exports = { generateBundleHTML };
