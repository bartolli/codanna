// Same-name disambiguation, one implementation for both hosts: node (details.js
// builds sidebar rows at generate time) and the page (theme.js embeds this file
// verbatim into detailScript; window.__qualify). Per node in a same-name group,
// the shortest scope suffix unique within the group, rendered by that suffix's
// head segment. The containing type is the innermost segment, so a class
// distinguishes before the module path does (JSON.Render vs PureJSON.Render);
// identical scopes fall back to the file stem; still-identical labels stay bare
// (tooltips carry file:line).
(function (root) {
  var BS = String.fromCharCode(92);
  // Module separators are per language; language_id keyed, shape-detected
  // fallback for languages the map does not name.
  var MOD_SEP = { rust: '::', c: '::', cpp: '::', php: BS, go: '/', gdscript: '/' };
  // Member access reads differently from module paths (JSON.Render but
  // RawSymbol::new); the class tier joins with the member idiom.
  var MEMBER_SEP = { rust: '::', c: '::', cpp: '::', php: '::' };

  function sepOf(n) {
    var s = MOD_SEP[String(n.language || '').toLowerCase()];
    if (s) return s;
    var m = String(n.module || '');
    if (m.indexOf('::') >= 0) return '::';
    if (m.indexOf(BS) >= 0) return BS;
    if (m.indexOf('/') >= 0) return '/';
    return '.';
  }

  /** Assigns n.qual/n.qsep on duplicate-name nodes; returns the label fn. */
  function qualify(nodes) {
    var byName = Object.create(null);
    nodes.forEach(function (n) { (byName[n.name] || (byName[n.name] = [])).push(n); });
    Object.keys(byName).forEach(function (name) {
      var group = byName[name];
      if (group.length < 2) return;
      var segs = group.map(function (n) {
        n.sep = sepOf(n);
        var s = String(n.module || '').split(n.sep).filter(Boolean);
        if (n.cls) s.push(n.cls);
        return s;
      });
      var suffix = function (g, k) {
        var s = segs[g];
        return s.slice(Math.max(0, s.length - k)).join(group[g].sep);
      };
      group.forEach(function (n, g) {
        var s = segs[g], qual = null, atClass = false;
        for (var k = 1; k <= s.length; k++) {
          var mine = suffix(g, k), unique = true;
          for (var o = 0; o < group.length; o++) {
            if (o !== g && suffix(o, k) === mine) { unique = false; break; }
          }
          if (unique) { qual = s[s.length - k]; atClass = (k === 1 && !!n.cls); break; }
        }
        if (qual) { n.qual = qual; n.qsep = atClass ? (MEMBER_SEP[String(n.language || '').toLowerCase()] || '.') : n.sep; }
        else if (n.file) { n.qual = String(n.file).split('/').pop().replace(/\.[a-z]+$/, ''); n.qsep = n.sep; }
      });
    });
    return labelOf;
  }

  function labelOf(n) {
    return n.qual ? n.qual + (n.qsep || n.sep || '::') + n.name : n.name;
  }

  var api = { qualify: qualify, labelOf: labelOf, sepOf: sepOf };
  if (typeof module !== 'undefined' && module.exports) module.exports = api;
  else root.__qualify = api;
})(typeof window !== 'undefined' ? window : globalThis);
