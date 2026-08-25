// Radial tidy tree page: the Observable `Tree` chart (ISC) over a flare-shaped
// hierarchy, pure d3/SVG, self-contained (d3 inlined), pan/zoom added outside
// the chart function so it stays verbatim.
const fs = require('fs');
const path = require('path');
const { themeStyle, themeScript, detailScript, kindVars } = require('./theme');

const D3 = path.join(__dirname, 'vendor', 'd3.min.js');

const TREE_FN = String.raw`
// Copyright 2022-2023 Observable, Inc.
// Released under the ISC license.
// https://observablehq.com/@d3/radial-tree
function Tree(data, { // data is either tabular (array of objects) or hierarchy (nested objects)
  path, // as an alternative to id and parentId, returns an array identifier, imputing internal nodes
  id = Array.isArray(data) ? d => d.id : null, // if tabular data, given a d in data, returns a unique identifier (string)
  parentId = Array.isArray(data) ? d => d.parentId : null, // if tabular data, given a node d, returns its parent's identifier
  children, // if hierarchical data, given a d in data, returns its children
  tree = d3.tree, // layout algorithm (typically d3.tree or d3.cluster)
  separation = tree === d3.tree ? (a, b) => (a.parent == b.parent ? 1 : 2) / a.depth : (a, b) => a.parent == b.parent ? 1 : 2,
  sort, // how to sort nodes prior to layout (e.g., (a, b) => d3.descending(a.height, b.height))
  label, // given a node d, returns the display name
  title, // given a node d, returns its hover text
  link, // given a node d, its link (if any)
  linkTarget = "_blank", // the target attribute for links (if any)
  width = 640, // outer width, in pixels
  height = 400, // outer height, in pixels
  margin = 60, // shorthand for margins
  marginTop = margin, // top margin, in pixels
  marginRight = margin, // right margin, in pixels
  marginBottom = margin, // bottom margin, in pixels
  marginLeft = margin, // left margin, in pixels
  radius = Math.min(width - marginLeft - marginRight, height - marginTop - marginBottom) / 2, // outer radius
  r = 3, // radius of nodes
  padding = 1, // horizontal padding for first and last column
  fill = "#999", // fill for nodes
  fillOpacity, // fill opacity for nodes
  stroke = "#555", // stroke for links
  strokeWidth = 1.5, // stroke width for links
  strokeOpacity = 0.4, // stroke opacity for links
  strokeLinejoin, // stroke line join for links
  strokeLinecap, // stroke line cap for links
  halo = "#fff", // color of label halo
  haloWidth = 3, // padding around the labels
} = {}) {

  // If id and parentId options are specified, or the path option, use d3.stratify
  // to convert tabular data to a hierarchy; otherwise we assume that the data is
  // specified as an object {children} with nested objects (a.k.a. the "flare.json"
  // format), and use d3.hierarchy.
  const root = path != null ? d3.stratify().path(path)(data)
      : id != null || parentId != null ? d3.stratify().id(id).parentId(parentId)(data)
      : d3.hierarchy(data, children);

  // Sort the nodes.
  if (sort != null) root.sort(sort);

  // Compute labels and titles.
  const descendants = root.descendants();
  const L = label == null ? null : descendants.map(d => label(d.data, d));

  // Compute the layout.
  tree().size([2 * Math.PI, radius]).separation(separation)(root);

  const svg = d3.create("svg")
      .attr("viewBox", [-marginLeft - radius, -marginTop - radius, width, height])
      .attr("width", width)
      .attr("height", height)
      .attr("style", "max-width: 100%; height: auto; height: intrinsic;")
      .attr("font-family", "sans-serif")
      .attr("font-size", 10);

  svg.append("g")
      .attr("fill", "none")
      .attr("stroke", stroke)
      .attr("stroke-opacity", strokeOpacity)
      .attr("stroke-linecap", strokeLinecap)
      .attr("stroke-linejoin", strokeLinejoin)
      .attr("stroke-width", strokeWidth)
    .selectAll("path")
    .data(root.links())
    .join("path")
      .attr("d", d3.linkRadial()
          .angle(d => d.x)
          .radius(d => d.y));

  const node = svg.append("g")
    .selectAll("a")
    .data(root.descendants())
    .join("a")
      .attr("xlink:href", link == null ? null : d => link(d.data, d))
      .attr("target", link == null ? null : linkTarget)
      .attr("transform", d => ` + '`rotate(${d.x * 180 / Math.PI - 90}) translate(${d.y},0)`' + String.raw`);

  node.append("circle")
      .attr("fill", d => d.children ? stroke : fill)
      .attr("r", r);

  if (title != null) node.append("title")
      .text(d => title(d.data, d));

  if (L) node.append("text")
      .attr("transform", d => ` + '`rotate(${d.x >= Math.PI ? 180 : 0})`' + String.raw`)
      .attr("dy", "0.32em")
      .attr("x", d => d.x < Math.PI === !d.children ? 6 : -6)
      .attr("text-anchor", d => d.x < Math.PI === !d.children ? "start" : "end")
      .attr("paint-order", "stroke")
      .attr("stroke", halo)
      .attr("stroke-width", haloWidth)
      .text((d, i) => L[i]);

  return svg.node();
}
`;

/**
 * hierarchy: flare-shaped {name, children, kind?, file?, line?, count?}.
 * opts: title, subtitle, workingDir (for file:// leaf links), leaves (count,
 * sizes the radius), theme (INITIAL theme; the page carries a runtime toggle).
 * Colours are design tokens applied as style('...', 'var(--...)'), so the
 * toggle re-themes the SVG through the CSS cascade alone.
 */
function generateTreeHTML(hierarchy, { title, subtitle = '', workingDir = '', leaves = 0, legendKinds = [], theme = 'dark', details = {} } = {}) {
  const d3src = fs.readFileSync(D3, 'utf8');
  const radius = Math.max(480, Math.round(leaves * 11 / (2 * Math.PI)));
  const margin = 140;
  const size = 2 * (radius + margin);
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
    #chart svg { width: 100%; height: 100%; cursor: grab; }
    a { color: inherit; text-decoration: none; }
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
    <div class="legend">${legend}</div>
    <button id="reset-btn">Reset zoom</button>
  </div>
  <div id="hint">Scroll: zoom, Drag: pan, Hover: path, Click a symbol: details</div>
  <script>${d3src}</script>
  <script>
${TREE_FN}
    const data = ${JSON.stringify(hierarchy)};
    const workingDir = ${JSON.stringify(workingDir)};
    const KVAR = ${JSON.stringify(kindVars)};
    const DETAILS = ${JSON.stringify(details)};

    const svgNode = Tree(data, {
      label: d => d.count ? d.name + ' (' + d.count + ')' : d.name,
      title: (d, n) => n.ancestors().reverse().map(a => a.data.name).join('.') + (d.kind ? '\\n' + d.kind + (d.file ? '  ' + d.file + ':' + d.line : '') : ''),
      link: null,
      sort: (a, b) => d3.ascending(a.data.name, b.data.name),
      stroke: '#888',
      halo: '#fff',
      width: ${size},
      height: ${size},
      margin: ${margin}
    });
    const svg = d3.select(svgNode);
    // Colours as token custom properties (style beats the chart function's
    // presentation attributes), so the theme toggle needs no re-render.
    svg.select('g').style('stroke', 'var(--text-3)');
    svg.selectAll('circle').style('fill', d => d.children ? 'var(--text-3)' : (KVAR[d.data.kind] || 'var(--n3)'));
    svg.selectAll('text').style('fill', 'var(--text-1)').style('stroke', 'var(--surface-0)');
    svg.style('font-family', 'var(--font-ui)');
    // Pan/zoom: wrap the chart groups, keep the chart function untouched.
    const wrapper = svg.append('g');
    svg.selectAll(':scope > g').filter(function() { return this !== wrapper.node(); }).each(function() { wrapper.node().appendChild(this); });
    const zoom = d3.zoom().scaleExtent([0.05, 12]).on('zoom', (e) => wrapper.attr('transform', e.transform));
    svg.call(zoom);
    svg.attr('width', null).attr('height', null).attr('style', null);
    document.getElementById('chart').appendChild(svgNode);
    document.getElementById('reset-btn').addEventListener('click', () => svg.transition().duration(500).call(zoom.transform, d3.zoomIdentity));

    // The poster's click opens the info panel (no go-buttons: this is the
    // glance view); Open file stays the explicit action inside the panel.
    svg.selectAll('a').on('click', (ev, d) => {
      if (!d.data || d.data.sid == null) return;
      const det = DETAILS[d.data.sid];
      const dotted = d.ancestors().reverse().map(a => a.data.name).join('.');
      window.__detail.show({
        name: d.data.name, dotted, kind: d.data.kind,
        visibility: det && det.vis, language: det && det.lang,
        edges: det ? det.edges : undefined,
        lines: det ? det.endLine - det.line + 1 : undefined,
        path: d.data.file ? d.data.file + ':' + (det ? det.line + '-' + det.endLine : d.data.line) : '',
        signature: det && det.sig, rels: det ? det.rels : {},
        href: d.data.file && workingDir ? 'file://' + workingDir + '/' + d.data.file : '',
      });
      ev.stopPropagation();
    });

    const root = d3.hierarchy(data);
    document.getElementById('stats').textContent = 'Nodes: ' + root.descendants().length + '  Leaves: ' + root.leaves().length + '  Depth: ' + root.height;
  </script>
</body>
</html>`;
}

module.exports = { generateTreeHTML };
