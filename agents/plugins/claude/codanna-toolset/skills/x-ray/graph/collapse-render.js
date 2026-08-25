// Collapsible horizontal tidy tree (after https://observablehq.com/@d3/collapsible-tree,
// ISC): the code hierarchy as a left-to-right tree, click an internal node to
// expand or collapse it, click a leaf to open its file. Same flare-shaped input
// as the radial tree; design tokens + the runtime theme toggle shared.
const fs = require('fs');
const path = require('path');
const { themeStyle, themeScript, detailScript, kindVars } = require('./theme');

const D3 = path.join(__dirname, 'vendor', 'd3.min.js');

/**
 * hierarchy: flare-shaped {name, children, kind?, file?, line?, count?}.
 * opts: title, subtitle, workingDir (leaf file:// links), legendKinds,
 * openDepth (levels expanded initially), theme (initial; runtime toggle on page).
 */
function generateCollapseHTML(hierarchy, { title, subtitle = '', workingDir = '', legendKinds = [], openDepth = 2, theme = 'dark', details = {} } = {}) {
  const d3src = fs.readFileSync(D3, 'utf8');
  const legend = legendKinds.map(k => `<span class="legend-item"><span class="dot" style="background:${kindVars[k] || 'var(--n3)'}"></span>${k}</span>`).join('');
  return `<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <title>${title}</title>
  ${themeStyle()}
  <style>
    html, body { margin: 0; height: 100%; }
    #chart { position: absolute; inset: 0; overflow: hidden; }
    #chart svg { width: 100%; height: 100%; cursor: grab; font-size: var(--fs-micro); user-select: none; }
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
    <div class="legend">${legend}</div>
    <button id="expand-btn">Expand all</button>
    <button id="collapse-btn">Collapse all</button>
    <button id="reset-btn">Reset view</button>
  </div>
  <div id="detail"></div>
  <div id="hint">Drag: pan, Scroll: zoom. Click a name: expand / collapse. Click a leaf: details</div>
  <script>${d3src}</script>
  <script>
    const data = ${JSON.stringify(hierarchy)};
    const workingDir = ${JSON.stringify(workingDir)};
    const KVAR = ${JSON.stringify(kindVars)};
    const OPEN_DEPTH = ${JSON.stringify(openDepth)};
    const DETAILS = ${JSON.stringify(details)};

    const width = Math.max(928, window.innerWidth - 40);
    const marginTop = 10, marginRight = 220, marginBottom = 10, marginLeft = 60;
    // dx is the row pitch, dy the column width -- the tree reads left to right.
    const root = d3.hierarchy(data);
    const dx = 14;
    const dy = Math.max(130, (width - marginRight - marginLeft) / (1 + root.height));
    const tree = d3.tree().nodeSize([dx, dy]);
    const diagonal = d3.linkHorizontal().x(d => d.y).y(d => d.x);

    const svg = d3.create("svg")
        .attr("viewBox", [0, 0, window.innerWidth, window.innerHeight])
        .style("font-family", "var(--font-ui)");
    const wrapper = svg.append("g");

    const gLink = wrapper.append("g")
        .attr("fill", "none")
        .style("stroke", "var(--text-3)")
        .attr("stroke-opacity", 0.4)
        .attr("stroke-width", 1.5);

    const gNode = wrapper.append("g")
        .attr("cursor", "pointer")
        .attr("pointer-events", "all");

    const label = d => d.data.count ? d.data.name + ' (' + d.data.count + ')' : d.data.name;
    const dotFill = d => (d._children || d.children)
      ? 'var(--text-2)'
      : (KVAR[d.data.kind] || 'var(--n3)');

    function update(event, source) {
      const duration = event && event.altKey ? 2500 : 250;
      const nodes = root.descendants().reverse();
      const links = root.links();
      tree(root);

      // Pan and zoom own the framing (the Observable original resized the svg
      // instead, which parked a collapsed tree under the info panel with no way
      // to travel to it).
      const transition = svg.transition().duration(duration);

      const node = gNode.selectAll("g").data(nodes, d => d.id);

      const nodeEnter = node.enter().append("g")
          .attr("transform", d => "translate(" + source.y0 + "," + source.x0 + ")")
          .attr("fill-opacity", 0)
          .attr("stroke-opacity", 0)
          .on("click", (event, d) => {
            if (d._children) { d.children = d.children ? null : d._children; update(event, d); }
            else showDetail(d);
          });

      nodeEnter.append("circle")
          .attr("r", 3)
          .style("fill", dotFill)
          .attr("stroke-width", 10);

      nodeEnter.append("text")
          .attr("dy", "0.31em")
          .attr("x", d => d._children ? -7 : 7)
          .attr("text-anchor", d => d._children ? "end" : "start")
          .text(label)
          .style("fill", "var(--text-1)")
          .style("stroke", "var(--surface-0)")
          .attr("stroke-linejoin", "round")
          .attr("stroke-width", 3)
          .attr("paint-order", "stroke");

      nodeEnter.append("title")
          .text(d => d.ancestors().reverse().map(a => a.data.name).join('.')
            + (d.data.kind ? String.fromCharCode(10) + d.data.kind + (d.data.file ? '  ' + d.data.file + ':' + d.data.line : '') : ''));

      node.merge(nodeEnter).transition(transition)
          .attr("transform", d => "translate(" + d.y + "," + d.x + ")")
          .attr("fill-opacity", 1)
          .attr("stroke-opacity", 1);

      node.exit().transition(transition).remove()
          .attr("transform", d => "translate(" + source.y + "," + source.x + ")")
          .attr("fill-opacity", 0)
          .attr("stroke-opacity", 0);

      const link = gLink.selectAll("path").data(links, d => d.target.id);
      const linkEnter = link.enter().append("path")
          .attr("d", d => { const o = { x: source.x0, y: source.y0 }; return diagonal({ source: o, target: o }); });
      link.merge(linkEnter).transition(transition).attr("d", diagonal);
      link.exit().transition(transition).remove()
          .attr("d", d => { const o = { x: source.x, y: source.y }; return diagonal({ source: o, target: o }); });

      root.eachBefore(d => { d.x0 = d.x; d.y0 = d.y; });
      document.getElementById('stats').textContent =
        'Visible: ' + root.descendants().length + ' of ' + total + '  Depth: ' + root.height;
    }

    root.x0 = 0;
    root.y0 = 0;
    // The flat list is captured BEFORE collapsing: root.descendants() walks only
    // attached children, so expand-all over it would never reach a collapsed
    // subtree's interior.
    const allNodes = root.descendants();
    const total = allNodes.length;
    allNodes.forEach((d, i) => {
      d.id = i;
      d._children = d.children;
      if (d.depth >= OPEN_DEPTH) d.children = null;
    });

    function setAll(open) {
      allNodes.forEach(d => { if (d._children) d.children = open ? d._children : null; });
      if (!open) root.children = root._children;   // the root row stays visible
      update(null, root);
    }
    document.getElementById('expand-btn').addEventListener('click', () => setAll(true));
    document.getElementById('collapse-btn').addEventListener('click', () => setAll(false));

    // The disc sidebar, verbatim: signature, relation groups, go-buttons that
    // expand the path to a related symbol's node, pan to it, and reopen there.
    function showDetail(d) {
      const det = d.data.sid != null ? DETAILS[d.data.sid] : null;
      const dotted = d.ancestors().reverse().map(a => a.data.name).join('.');
      const spec = {
        name: d.data.name,
        dotted,
        kind: d.data.kind,
        visibility: det && det.vis,
        language: det && det.lang,
        edges: det ? det.edges : undefined,
        lines: det ? det.endLine - det.line + 1 : undefined,
        path: d.data.file ? d.data.file + ':' + (det ? det.line + '-' + det.endLine : d.data.line) : '',
        signature: det && det.sig,
        rels: det ? det.rels : {},
        href: d.data.file && workingDir ? 'file://' + workingDir + '/' + d.data.file : '',
      };
      if (d.data.count) (spec.rels = Object.assign({}, spec.rels))['Collapsed inside'] = [{ name: d.data.count + ' symbols', k: 1 }];
      window.__detail.show(spec, goTo);
    }

    function goTo(ref) {
      const sid = +ref;
      const target = allNodes.find(n => n.data.sid === sid);
      if (!target) return;
      target.ancestors().forEach(a => { if (a._children) a.children = a._children; });
      update(null, target.parent || target);
      const k = d3.zoomTransform(svg.node()).k;
      svg.transition().duration(400).call(zoom.transform,
        d3.zoomIdentity.translate(window.innerWidth / 2 - k * target.y, window.innerHeight / 2 - k * target.x).scale(k));
      showDetail(target);
    }

    const zoom = d3.zoom().scaleExtent([0.05, 12]).on('zoom', (e) => wrapper.attr('transform', e.transform));
    const home = d3.zoomIdentity.translate(70, window.innerHeight / 2);
    document.getElementById('reset-btn').addEventListener('click', () => svg.transition().duration(500).call(zoom.transform, home));

    update(null, root);
    document.getElementById('chart').appendChild(svg.node());
    svg.call(zoom).call(zoom.transform, home);
  </script>
</body>
</html>`;
}

module.exports = { generateCollapseHTML };
