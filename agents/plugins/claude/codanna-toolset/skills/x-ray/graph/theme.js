// Shared theming for the x-ray pages: token CSS (dark default, light under
// [data-theme="light"]), the runtime theme state + toggle button, and the
// kind -> categorical-slot mapping. Reads the vendored ESM tokens module via
// require(esm): node >= 22.12 (or 20.19) required.
const path = require('path');
const { tokens, cssBlock, jsTheme } = require(path.join(__dirname, 'tokens.mjs'));

// Symbol kinds on the categorical slots, fixed assignment, never cycled;
// structural noise kinds take the neutrals.
const KIND_SLOT = { Function: 0, Method: 1, Struct: 2, Class: 3, Trait: 4, Interface: 5, Enum: 6, TypeAlias: 7, Constant: 8, Macro: 9 };
const KIND_NEUTRAL = { Field: 0, Variable: 1, Parameter: 2, Module: 2 };

function kindColors(theme) {
  const t = tokens(theme); const out = {};
  for (const [k, i] of Object.entries(KIND_SLOT)) out[k] = t.slots[i];
  for (const [k, i] of Object.entries(KIND_NEUTRAL)) out[k] = t.neutrals[i];
  return out;
}

// Kind -> CSS custom property, for SVG consumers that colour via style('fill',
// 'var(--gN)') and get theme switching from the cascade for free.
const kindVars = (() => {
  const out = {};
  for (const [k, i] of Object.entries(KIND_SLOT)) out[k] = `var(--g${i + 1})`;
  for (const [k, i] of Object.entries(KIND_NEUTRAL)) out[k] = `var(--n${i + 1})`;
  return out;
})();

const TOKENS = {
  dark: { ...jsTheme('dark'), kinds: kindColors('dark') },
  light: { ...jsTheme('light'), kinds: kindColors('light') },
};

/** Token custom properties (both themes) + the chrome every page shares:
 *  body ground, info panel, buttons, and the theme toggle cluster (same shape
 *  as the graph disc's zoom cluster: 30px square, 6px radius, surface-2). */
function themeStyle() {
  return `<style>
${cssBlock('dark', ':root')}
${cssBlock('light', ':root[data-theme="light"]')}
html, body { background: var(--surface-0); color: var(--text-1); font-family: var(--font-ui); }
#info { position: absolute; top: 10px; left: 10px; z-index: 8;
  background: var(--surface-1); border: 1px solid var(--border); color: var(--text-1);
  border-radius: 8px; padding: 12px 14px; font-size: var(--fs-base); max-width: 400px;
  box-shadow: 0 2px 8px rgba(0,0,0,0.25); }
#info h3 { margin: 0 0 8px; font-size: var(--fs-title); }
#info .stat { margin: 3px 0; color: var(--text-2); }
#info .legend { margin-top: 8px; line-height: 1.8; }
#info .legend-item { display: inline-block; margin-right: 10px; white-space: nowrap; }
#info .dot { display: inline-block; width: 9px; height: 9px; border-radius: 50%; margin-right: 4px; vertical-align: middle; }
#info .swatch { display: inline-block; width: 18px; height: 3px; margin-right: 4px; vertical-align: middle; }
#info button { margin-top: 8px; padding: 4px 10px; background: var(--surface-2); color: var(--text-2);
  border: 1px solid var(--border); border-radius: 4px; cursor: pointer; font-size: var(--fs-body); }
#info button:hover { color: var(--text-1); border-color: var(--border-strong); }
#hint { position: absolute; bottom: 8px; left: 50%; transform: translateX(-50%); font-size: var(--fs-small); color: var(--text-3); }
#detail { position: absolute; right: 12px; top: 54px; width: 300px; z-index: 8; display: none;
  background: var(--surface-2); border: 1px solid var(--border); border-radius: 9px;
  padding: 12px; box-shadow: 0 8px 28px rgba(0,0,0,.16); font-size: var(--fs-body);
  max-height: calc(100% - 130px); overflow-y: auto; }
#detail h2 { font-size: var(--fs-base); margin: 0 22px 6px 0; line-height: 1.3; }
#detail .meta { font-size: var(--fs-small); color: var(--text-2); margin-bottom: 9px; word-break: break-all; }
#detail .chip { display: inline-block; font-size: var(--fs-micro); padding: 1px 6px; margin: 0 3px 3px 0;
  border: 1px solid var(--border); border-radius: 999px; color: var(--text-2); }
#detail .chip.mono { font-family: var(--font-mono); border-style: dashed; }
#detail .x { position: absolute; right: 8px; top: 6px; background: none; border: none;
  color: var(--text-3); font-size: 16px; cursor: pointer; padding: 2px 6px; }
#detail .x:hover { color: var(--text-1); }
#detail a.open { display: inline-block; font-size: var(--fs-small); color: var(--accent);
  text-decoration: none; border: 1px solid var(--accent); border-radius: 6px;
  padding: 4px 8px; margin-top: 6px; }
#detail a.open:hover { background: color-mix(in srgb, var(--accent) 12%, transparent); }
#detail .meta { font-size: var(--fs-small); color: var(--text-2); margin-bottom: 9px; }
#detail .meta span { display: inline-block; margin-right: 8px; }
#detail .nb { font-size: 10px; text-transform: uppercase; letter-spacing: .07em;
  color: var(--text-3); margin: 10px 0 5px; font-weight: 600; }
#detail ul { list-style: none; margin: 0; padding: 0; }
#detail li button, #detail li .ext {
  background: none; border: 0; padding: 2px 0; font: inherit; font-size: var(--fs-body);
  color: var(--text-2); text-align: left; width: 100%; display: block;
  overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
}
#detail li button { cursor: pointer; }
#detail li button:hover { color: var(--accent); text-decoration: underline; }
#detail li .rk { color: var(--text-2); font-size: 10px; }
#detail pre.sig { font: 11px/1.45 var(--font-mono); white-space: pre-wrap; word-break: break-word;
  margin: 8px 0 4px; padding: 7px 8px; background: var(--surface-1); color: var(--text-1);
  border: 1px solid var(--border); border-radius: 6px; max-height: 180px; overflow: auto; }
#detail pre.sig .tk-kw { color: var(--g7); }
#detail pre.sig .tk-ty { color: var(--g3); }
#detail pre.sig .tk-fn { color: var(--text-1); font-weight: 600; }
#detail pre.sig .tk-str { color: var(--g5); }
#detail pre.sig .tk-num { color: var(--g2); }
#themectl { position: absolute; top: 12px; right: 12px; z-index: 9; }
#themectl button { font-family: var(--font-ui); width: 30px; height: 30px; padding: 0;
  cursor: pointer; background: var(--surface-2); color: var(--text-2);
  border: 1px solid var(--border); border-radius: 6px; display: grid; place-items: center; }
#themectl button:hover { color: var(--text-1); border-color: var(--border-strong); }
#themectl svg { display: block; }
</style>`;
}

/** The toggle (half-filled circle icon) + theme state. Pages read the current
 *  theme object with THEME() at render time and re-colour their canvas/SVG on
 *  the document 'themechange' event; the CSS chrome follows the data-theme
 *  attribute on its own. Initial theme: localStorage override, else `initial`. */
function themeScript(initial = 'dark') {
  return `<div id="themectl"><button id="themebtn" title="Toggle light/dark theme" aria-label="Toggle light/dark theme">
<svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true"><circle cx="7" cy="7" r="5.4" fill="none" stroke="currentColor" stroke-width="1.4"/><path d="M7 1.6 A5.4 5.4 0 0 1 7 12.4 Z" fill="currentColor"/></svg>
</button></div>
<script>
window.TOKENS = ${JSON.stringify(TOKENS)};
(function () {
  var KEY = 'codanna-viz-theme';
  var cur = ${JSON.stringify(initial)};
  try { cur = localStorage.getItem(KEY) || cur; } catch (e) { /* storage blocked: per-load theme only */ }
  window.THEME = function () { return TOKENS[cur] || TOKENS.dark; };
  function apply() {
    if (cur === 'light') document.documentElement.setAttribute('data-theme', 'light');
    else document.documentElement.removeAttribute('data-theme');
    document.dispatchEvent(new CustomEvent('themechange', { detail: { theme: cur } }));
  }
  document.getElementById('themebtn').addEventListener('click', function () {
    cur = cur === 'light' ? 'dark' : 'light';
    try { localStorage.setItem(KEY, cur); } catch (e) { /* fine */ }
    apply();
  });
  if (cur === 'light') apply();
})();
</script>`;
}

/** The disc sidebar's panel engine, shared by the x-ray pages: meta row, tag
 *  chips, dotted path, file:span chip, highlighted signature (the disc's
 *  tokenizer ported verbatim), and relation groups (Calls / Called by, ...)
 *  with x-count badges; rows with a `ref` become go-buttons wired to `onGo`.
 *  spec: {name, dotted, kind, visibility, language, edges, lines, path,
 *  signature, rels: {label: [{name, k, ref?}]}, href}. */
function detailScript() {
  return `<script>
window.__detail = (function () {
  var esc = function (t) { return String(t).replace(/[&<>"]/g, function (c) { return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]; }); };
  var SIG_KW = ('fn pub const static async unsafe impl trait struct enum type where mut self Self let ' +
    'return class interface def func function var val public private protected internal override ' +
    'abstract final void new extends implements import export default readonly declare namespace module ' +
    'package record sealed data object companion init lateinit suspend operator inline throws await dyn ' +
    'ref out in is as typedef template typename virtual constexpr extern friend explicit noexcept ' +
    'defn defmacro defmethod defprotocol defrecord ns signal onready export_var match macro_rules ' +
    'global local elif lambda yield async nonlocal with try except finally raise pass').split(' ');
  var SIG_PRIM = ('int long short char float double bool string str usize isize u8 u16 u32 u64 u128 i8 i16 ' +
    'i32 i64 i128 f32 f64 byte boolean number any unknown never void null nil none None undefined ' +
    'auto size_t uint int32 int64 float32 float64 uint8 uint16 uint32 uint64 rune error Array Dictionary ' +
    'Variant').split(' ');
  var sigKw = Object.create(null), sigPrim = Object.create(null);
  SIG_KW.forEach(function (w) { sigKw[w] = true; });
  SIG_PRIM.forEach(function (w) { sigPrim[w] = true; });
  function highlightSig(sig) {
    var re = /("(?:[^"\\\\]|\\\\.)*"|'(?:[^'\\\\]|\\\\.)*')|(\\d[\\w.]*)|([A-Za-z_$][\\w$]*)|(\\s+)|([^\\s\\w"']+)/g;
    var out = '', m, src = String(sig), prevWord = '';
    var DECL = { fn: 1, def: 1, func: 1, 'function': 1, fun: 1, proc: 1, defn: 1, defmacro: 1, sub: 1 };
    while ((m = re.exec(src))) {
      var tok = m[0], cls = '';
      if (m[1]) cls = 'tk-str';
      else if (m[2]) cls = 'tk-num';
      else if (m[3]) {
        var rest = src.slice(re.lastIndex).replace(/^\\s+/, '');
        var callable = rest.charAt(0) === '(' || (rest.charAt(0) === '<' && /^<[^>]*>\\s*\\(/.test(rest));
        if (callable && DECL[prevWord]) cls = 'tk-fn';
        else if (sigKw[tok]) cls = 'tk-kw';
        else if (sigPrim[tok] || /^[A-Z]/.test(tok)) cls = 'tk-ty';
        else if (callable) cls = 'tk-fn';
        prevWord = tok;
      }
      out += cls ? '<span class="' + cls + '">' + esc(tok) + '</span>' : esc(tok);
    }
    return out;
  }
  var REL_ORDER = ['Calls', 'Called by', 'Uses', 'Used by', 'Implements', 'Implemented by', 'Extends', 'Extended by', 'Defines', 'Defined in'];
  function show(spec, onGo) {
    var panel = document.getElementById('detail');
    var h = '<button class="x" title="Close">&times;</button>'
      + '<h2>' + esc(spec.name) + '</h2>'
      + '<div class="meta">'
      + (spec.edges != null ? '<span>' + spec.edges + ' edge' + (spec.edges === 1 ? '' : 's') + '</span>' : '')
      + (spec.lines ? '<span>' + spec.lines + ' line' + (spec.lines === 1 ? '' : 's') + '</span>' : '')
      + '</div>'
      + [spec.kind, spec.visibility, spec.language].filter(Boolean).map(function (t) { return '<span class="chip">#' + esc(String(t).toLowerCase()) + '</span>'; }).join('')
      + (spec.dotted ? '<div class="chip" style="border-style:dashed">' + esc(spec.dotted) + '</div>' : '')
      + (spec.path ? '<div class="chip mono">' + esc(spec.path) + '</div>' : '')
      + (spec.signature ? '<pre class="sig">' + highlightSig(spec.signature) + '</pre>' : '');
    var rels = spec.rels || {}, any = false;
    var heads = REL_ORDER.concat(Object.keys(rels).filter(function (k) { return REL_ORDER.indexOf(k) < 0; }));
    heads.forEach(function (hd) {
      var list = rels[hd]; if (!list || !list.length) return;
      any = true;
      h += '<div class="nb">' + esc(hd) + ' (' + list.length + ')</div><ul>'
        + list.slice(0, 60).map(function (it) {
            var label = esc(it.name) + (it.k > 1 ? ' <span class="rk">&times;' + it.k + '</span>' : '');
            return it.ref != null && onGo
              ? '<li><button data-go="' + it.ref + '">' + label + '</button></li>'
              : '<li><span class="ext">' + label + '</span></li>';
          }).join('') + '</ul>';
    });
    if (!any) h += '<div class="nb">No edges</div>';
    if (spec.href) h += '<a class="open" href="' + esc(spec.href) + '" target="_blank">Open file</a>';
    panel.innerHTML = h;
    panel.style.display = 'block';
    panel.querySelector('.x').onclick = function () { panel.style.display = 'none'; };
    if (onGo) Array.prototype.forEach.call(panel.querySelectorAll('[data-go]'), function (b) {
      b.onclick = function () { onGo(b.getAttribute('data-go')); };
    });
  }
  return { show: show, esc: esc, highlightSig: highlightSig };
})();
</` + `script>`;
}

module.exports = { TOKENS, kindColors, kindVars, themeStyle, themeScript, detailScript };
