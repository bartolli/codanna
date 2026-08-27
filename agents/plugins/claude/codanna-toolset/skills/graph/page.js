/**
 * Mount the disc into one element.
 *
 *   mountVaultGraph(root, data, deps) -> the __vg debug surface
 *
 * root  the element to build inside. Must be the page's own root -- the element carrying
 *       class="vault-graph", since every rule in page.css is scoped under it and every
 *       custom property is declared on it.
 * data  what build-graph.mjs or the plugin's adapter produced.
 * deps  { Graph, Sigma, rendering, logoMask } -- INJECTED rather than read off `window`,
 *       because in a plugin the libraries are bundled module imports and never become
 *       globals at all. The standalone page passes its UMD globals in; see shell.html.
 *
 *       Also, optionally:
 *         folderColors      { "<folder>": "g7" } -- the saved per-folder palette slots.
 *         subfolderColors   { "<folder>/<sub>": "g7" } -- as folderColors, one level down.
 *                           Keyed to match subShade and subOrder's own keys -- "" for
 *                           notes sitting directly in the folder. Pins the whole tint the
 *                           automatic ladder would otherwise compute for that subfolder.
 *         onSubfolderColors(m) as onFolderColors, for that map.
 *         folderShown       { "<folder>": true|false } -- saved per-folder visibility
 *                           DEFAULTS. Tri-state: absent means "whatever the `_` rule
 *                           says", which is hidden for a folder whose name starts with an
 *                           underscore and shown for everything else.
 *         onFolderShown(m)  as onFolderColors, for that map.
 *         panEnabled        false to start with drag-to-pan off. ABSENT MEANS ON -- a fresh
 *                           page has no saved answer, and dragging is what a graph does, so
 *                           the default cannot be "wait to be told". The corner control
 *                           flips it live; this is only where it starts.
 *         onPanEnabled(v)   as onFolderColors, for that flag.
 *         pinned            note ids to put in the hub at mount, in slot order. Filtered
 *                           against the graph on the way in: the store outlives the vault
 *                           it was written from, so a note that has since been renamed or
 *                           deleted is simply not there any more and the hub opens without
 *                           it rather than holding a slot for a note that cannot be drawn.
 *         onPinned(ids)     as onFolderColors, for that list.
 *         settingsUI        true to show the gear AND let it open the page's own panel.
 *                           The STANDALONE sets this, because nothing else there can hold
 *                           a setting.
 *         openSettings()    show the gear but hand its click to the host. The PLUGIN sets
 *                           this: the gear belongs in the view either way -- it is where
 *                           somebody looking at the disc goes to look for it -- but what
 *                           it opens is Obsidian's own settings tab, not a second panel
 *                           saying the same things. Mutually exclusive with settingsUI;
 *                           if both arrive, this one wins.
 *         onFolderColors(m) called after a change, with the new map. THE HOST PERSISTS,
 *                           not this file: the plugin writes it through saveData() and
 *                           the standalone into localStorage, and page.js knowing about
 *                           either store would put an Obsidian-forbidden API (or a
 *                           useless one) into the other target's bundle.
 *
 * WAS AN IIFE. Nothing about the body changed when it stopped being one: it already kept
 * every name to itself, which is what made this a signature change rather than a rewrite.
 */
function mountVaultGraph(root, data, deps) {
  "use strict";

  var DATA = data;
  var Graph = deps.Graph;
  var SigmaCls = deps.Sigma;
  // The programs hang off the UMD NAMESPACE object, not off the Sigma class that
  // SigmaCls resolves to -- the bundle sets both `Sigma.Sigma` and
  // `Sigma.rendering` on the same export, so `SigmaCls.rendering` is undefined and
  // reaching for a program through it throws during construction. That kills the
  // whole init inside its setTimeout, which surfaces as a page stuck on
  // "Laying out graph..." with nothing in the console.
  var RENDERING = deps.rendering || {};
  var LOGO_MASK = deps.logoMask || "";
  // The window this view lives in. Obsidian passes activeWindow so a popout schedules its
  // own timers; the standalone page passes nothing and gets its own window. Never the bare
  // global: in a popout that is the wrong window, and obsidianmd/prefer-active-window-timers
  // is an error for exactly that reason.
  var WIN = deps.win || window;
  /**
   * The document, for PAGE-LEVEL STATE ONLY -- never for finding an element.
   *
   * check-scope forbids `document.` outright, and rightly: the page is handed one element and
   * everything it touches belongs inside it, because in Obsidian a document lookup grabs the
   * app's element, or with two views open the OTHER view's. That reasoning is about finding
   * things. It does not cover "is this page being displayed at all", which is not an element,
   * cannot be scoped to a container, and is the one fact the animation genuinely needs from
   * outside itself: rAF is throttled in a hidden tab, so a cascade started there advances by a
   * wall-clock factor with no frames to spend it on and the disc lands wrong.
   *
   * So it comes through deps like the window does, which also makes it substitutable -- and a
   * host that omits it gets no visibility handling rather than a reach it did not sanction.
   * Obsidian is arguably such a host: its tabs do not change visibilityState, only the app
   * window does, so a leaf hidden behind another tab is not "hidden" by this measure and wants
   * the workspace's own hook instead.
   */
  //
  // ONE DECLARATION. This was two `var DOC` lines, and the first was dead: `root.ownerDocument`
  // ran second and won, so `deps.doc` was never consulted and the substitutability the comment
  // above describes did not exist. No host passes `doc` today, and in both hosts the two
  // expressions agree -- the standalone's root is in `window.document`, and the plugin's is in
  // the leaf's own document, which is also what `activeWindow.document` gives -- so this
  // changes no behaviour. Ordered deps-first so the fallback chain matches what the comment
  // promises, with the element's own document ahead of the window's, since it is the one that
  // stays right when the view is dragged into a popout.
  var DOC = deps.doc || (root && root.ownerDocument) || (WIN && WIN.document) || null;
  var API = null;

  // Ids carry a `vg-` prefix so they cannot collide with the host document, and the
  // prefix is added HERE rather than at the ~200 call sites, which stay $("graph").
  var ID = "vg-";
  // querySelector on the ROOT, not getElementById on the document. Two views of this page
  // in one Obsidian window would otherwise share every id, and the second one would drive
  // the first one's DOM.
  var $ = function (id) { return root.querySelector("#" + ID + id); };

  // THE TOKENS LIVE ON OUR OWN ROOT, not on the document's.
  //
  // They used to be declared on `:root`, so reading them off documentElement worked. They
  // are on `.vault-graph` now -- the page's own #app element -- because a page that has to
  // mount inside somebody else's document must not put its palette on their <html>.
  //
  // This one line is the whole cost of that move, and getting it wrong is silent: every
  // getPropertyValue returns "", every colour parses to black, and the heatmap draws every
  // day as though it were empty. Measured exactly that way -- 88 days with notes, 88 of
  // them "partially filled" -- before this was pointed at the right element.
  // The container itself, which $() cannot return: querySelector only sees descendants.
  var ROOT = root;
  // Replace an element's children from a markup string, without an innerHTML sink.
  //
  // Parsed in an inert document -- no browsing context, so nothing executes during the
  // parse -- and the resulting nodes are moved across. Every caller escapes its
  // interpolations with esc(); the rest is this page's own constants.
  var setHTML = function (el, html) {
    var parsed = new DOMParser().parseFromString("<body>" + html + "</body>", "text/html");
    el.replaceChildren.apply(el, Array.prototype.slice.call(parsed.body.childNodes));
  };

  var css = function (name) {
    return getComputedStyle(ROOT).getPropertyValue(name).trim();
  };

  // --- OKLCH, so a subfolder tint can rotate hue and nudge lightness without
  // --- drifting off the perceptual band the base palette was validated on.
  var s2lin = function (c) { return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4); };
  var lin2s = function (c) {
    c = Math.max(0, Math.min(1, c));
    return c <= 0.0031308 ? 12.92 * c : 1.055 * Math.pow(c, 1 / 2.4) - 0.055;
  };
  function relLum(h) {
    h = String(h).trim().replace(/^#/, "");
    if (h.length === 3) h = h[0] + h[0] + h[1] + h[1] + h[2] + h[2];
    var c = [0, 2, 4].map(function (i) { return s2lin(parseInt(h.slice(i, i + 2), 16) / 255); });
    return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
  }
  function hex2lab(h) {
    h = String(h).trim().replace(/^#/, "");
    if (h.length === 3) h = h[0] + h[0] + h[1] + h[1] + h[2] + h[2];
    var r = s2lin(parseInt(h.slice(0, 2), 16) / 255),
        g = s2lin(parseInt(h.slice(2, 4), 16) / 255),
        b = s2lin(parseInt(h.slice(4, 6), 16) / 255);
    var l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b),
        m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b),
        s2 = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
    return [0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s2,
            1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s2,
            0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s2];
  }
  function lab2hex(L, A, B) {
    var l = Math.pow(L + 0.3963377774 * A + 0.2158037573 * B, 3),
        m = Math.pow(L - 0.1055613458 * A - 0.0638541728 * B, 3),
        s2 = Math.pow(L - 0.0894841775 * A - 1.2914855480 * B, 3);
    var rgb = [
      lin2s(4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s2),
      lin2s(-1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s2),
      lin2s(-0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s2)
    ];
    return "#" + rgb.map(function (v) {
      var n = Math.round(v * 255).toString(16);
      return n.length < 2 ? "0" + n : n;
    }).join("");
  }
  // Rotate hue by dh degrees and shift lightness by dL, keeping chroma.
  function shade(hex, dh, dL) {
    var lab = hex2lab(hex), C = Math.hypot(lab[1], lab[2]);
    var h = Math.atan2(lab[2], lab[1]) + dh * Math.PI / 180;
    var L = Math.max(0.18, Math.min(0.92, lab[0] + dL));
    return lab2hex(L, C * Math.cos(h), C * Math.sin(h));
  }

  // Shapes are OFF: circles only. The machinery did work in the end
  // (createNodeBorderProgram lives on Sigma.rendering; borders[0] is the OUTER band
  // and the {fill:true} entry the CORE) but disc / ring / target / outline read as
  // visual noise at these node sizes, so colour carries subfolders on its own.
  // If shapes come back: an SVG data URI also needs explicit width/height, or it
  // rasterises to 0x0 and silently never draws.

  // THE PALETTE AS CHOOSABLE SLOTS.
  //
  // A folder override stores a slot KEY ("g7"), never a hex. The palette has separate
  // light and dark values -- see the two blocks in page.css -- so a stored hex would be
  // right in one theme and wrong in the other, and it would also let a colour be picked
  // that never went through the separation measurements in design/0004.
  //
  // TWELVE, including two greys. The greys are ordinary slots, pickable like any hue,
  // because "this folder should recede" is a real thing to want and the palette should
  // answer it with a choice rather than only as a side effect of running out of hues.
  var SLOT_NAMES = ["Blue", "Orange", "Aqua", "Yellow", "Green", "Magenta",
                    "Violet", "Red", "Cyan", "Orchid", "Grey", "Slate"];

  var THEME = {};
  function readTheme() {
    var surf = css("--surface-1");
    THEME = {
      dark:        relLum(surf) < 0.4,
      text:        css("--text-1"),
      dim:         css("--dim"),
      today:       css("--today"),
      edge:        css("--edge"),
      edgeHi:      css("--edge-hi"),
      surface:     surf,
      hoverBg:     css("--surface-2"),
      hoverBorder: css("--border-strong"),
      slots:    ["--g1", "--g2", "--g3", "--g4", "--g5", "--g6",
                 "--g7", "--g8", "--g9", "--g10", "--g11", "--g12"].map(css),
      // Still here, and still used: --dim's neighbours for the colorOf fallback. They
      // are no longer the overflow palette -- slots 11-12 carry the same two values as
      // real slots, and the automatic run cycles rather than falling back.
      neutrals: ["--n1", "--n2", "--n3"].map(css)
    };
    // key -> live hex, rebuilt on every theme read so an override follows the theme
    // instead of freezing whichever one was current when it was picked.
    THEME.byKey = Object.create(null);
    THEME.slots.forEach(function (hex, i) { THEME.byKey["g" + (i + 1)] = hex; });
  }
  readTheme();

  // Folder/subfolder -> slot key. Comes in from the host: the plugin reads it out of its
  // own settings, the standalone page out of localStorage. Anything unrecognised is
  // dropped rather than trusted -- this map has been through a JSON file either way, and
  // an unknown key would otherwise resolve to undefined and paint a group black. One
  // cleaner for both maps: a subfolder key is just "<folder>/<sub>" (matching subShade
  // and subOrder's own keys, "" for notes sitting directly in the folder) rather than a
  // different shape needing different validation.
  function cleanSlotMap(raw) {
    var out = Object.create(null);
    if (!raw || typeof raw !== "object") return out;
    Object.keys(raw).forEach(function (k) {
      var v = raw[k];
      if (typeof v === "string" && /^g([1-9]|1[0-2])$/.test(v)) out[k] = v;
    });
    return out;
  }
  var folderColors = cleanSlotMap(deps.folderColors);
  var subfolderColors = cleanSlotMap(deps.subfolderColors);

  // DRAG-TO-PAN, ON UNLESS THE HOST SAYS OTHERWISE. Absent means on: a fresh page has no
  // saved answer and dragging is what a graph does, so the default cannot be "wait to be
  // told". Only an explicit false turns it off -- github#4 argued for off-by-default on a
  // 10k vault, and the reverse won: the rim is unreachable without it, and a control in the
  // corner is a cheaper way to find that out than a settings tab is.
  var panEnabled = deps.panEnabled === false ? false : true;
  var onPanEnabled = typeof deps.onPanEnabled === "function" ? deps.onPanEnabled : null;

  // COMPACT DATE AXIS, ON UNLESS THE HOST SAYS OTHERWISE -- same absent-means-on contract as
  // panEnabled above, for the same reason: the better axis should not need anyone to find a
  // toggle first (github#23).
  var compactAxis = deps.compactAxis === false ? false : true;
  var onCompactAxis = typeof deps.onCompactAxis === "function" ? deps.onCompactAxis : null;

  // ARCHIVE FOLDERS: the ones whose name starts with `_`.
  //
  // A leading underscore is how a vault says "sorts last, not part of the working set" --
  // `_ Archives`, `_old`, scratch. They are still notes and still in the graph, but they
  // are not what the disc is for, so they get three things done to them: no slot in the
  // colour rotation, a recessive grey, and hidden on arrival. All three are overridable
  // per folder, and none of them is a rule about FILES -- `_scratch.md` is a note like
  // any other. This asks about the top-level group name, which is the only level that
  // owns a wedge.
  function isArchiveGroup(g) { return String(g).charAt(0) === "_"; }

  // THE ARCHIVE GREY IS A PALETTE SLOT, not a neutral off to one side.
  //
  // It was `--n3` first, which looked right and was the wrong kind of answer: the settings
  // panel could not mark it, because the colour a folder was using was not one of the
  // twelve on offer, so an archive row rang no swatch and "Auto" pointed at nothing.
  //
  // g11 rather than g12, of the two greys. It is the LOWER-CONTRAST one against the
  // surface in both themes -- measured 4.99 vs 9.51 on light and 5.16 vs 9.12 on dark --
  // which is what recede means, and it is also the darker-looking of the two in the dark
  // theme the disc opens in.
  var ARCHIVE_SLOT = "g11";

  // The eye, shared between the legend and the settings panel: same mark for the live
  // filter and for the default it returns to, because they are the same question asked
  // about two different moments.
  function eyeSvg(on) {
    var lid = '<path d="M1.6 8S4 3.9 8 3.9 14.4 8 14.4 8 12 12.1 8 12.1 1.6 8 1.6 8z"' +
              ' fill="none" stroke="currentColor" stroke-width="1.25"/>';
    return '<svg viewBox="0 0 16 16" aria-hidden="true">' + lid +
      (on ? '<circle cx="8" cy="8" r="2" fill="currentColor"/>'
          : '<path d="M3 13L13 3" stroke="currentColor" stroke-width="1.25"/>') +
      '</svg>';
  }

  // A thumbtack: filled head when pinned, outline otherwise -- the same on/off
  // convention eyeSvg uses, for the detail card's hub toggle.
  function pinSvg(on) {
    var head = '<circle cx="8" cy="5.6" r="3.35" ' +
      (on ? 'fill="currentColor"' : 'fill="none" stroke="currentColor" stroke-width="1.25"') + '/>';
    var point = '<path d="M8 8.8V14" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>';
    return '<svg viewBox="0 0 16 16" aria-hidden="true">' + head + point + '</svg>';
  }

  // A disclosure twisty, or an invisible placeholder so labels stay aligned. Shared
  // between the legend and the settings panel now that both nest subfolder rows under a
  // folder -- the same reason eyeSvg above is shared.
  function twBtn(attrs, open) {
    return attrs
      ? '<button class="tw" ' + attrs + ' aria-expanded="' + open + '">' +
        (open ? "▾" : "▸") + '</button>'
      : '<span class="tw none">▸</span>';
  }

  // Explicit per-folder visibility: true = shown, false = hidden, absent = the default
  // above. Tri-state on purpose -- "absent" has to stay distinguishable from "false", or
  // turning `_ Archives` off by hand would be indistinguishable from never having said
  // anything about it, and a later change to the default could not reach it.
  function cleanFolderShown(raw) {
    var out = Object.create(null);
    if (!raw || typeof raw !== "object") return out;
    Object.keys(raw).forEach(function (g) {
      if (typeof raw[g] === "boolean") out[g] = raw[g];
    });
    return out;
  }
  var folderShown = cleanFolderShown(deps.folderShown);

  // Hidden unless something says otherwise: an explicit choice first, then the `_` rule.
  function hiddenByDefault(g) {
    if (typeof folderShown[g] === "boolean") return !folderShown[g];
    return isArchiveGroup(g);
  }
  var SETTINGS_UI = !!deps.settingsUI;
  var openHostSettings = typeof deps.openSettings === "function" ? deps.openSettings : null;
  var saveFolderColors = typeof deps.onFolderColors === "function" ? deps.onFolderColors : null;
  var saveSubfolderColors = typeof deps.onSubfolderColors === "function" ? deps.onSubfolderColors : null;
  var saveFolderShown = typeof deps.onFolderShown === "function" ? deps.onFolderShown : null;
  var savePinned = typeof deps.onPinned === "function" ? deps.onPinned : null;

  /* ------------------------------------------------------------------ state */

  var state = {
    // Grouping is fixed to the PARA folder and there is only one layout now, so
    // both are constants rather than switchable state.
    dim: "folder",
    layout: "rings",
    hiddenSub: Object.create(null),   // "folder/sub" -> true
    hidden: Object.create(null),   // dim -> {group: true}
    // Highlighting is a SEPARATE axis from visibility, which is the whole point of
    // the eye icons: the row used to hide a group, so there was no way to say "show
    // me where this one is" without hiding everything else.
    highlight: Object.create(null),   // group -> true: pushed out and haloed
    highlightSub: Object.create(null),// "folder/sub" -> true: same, one subfolder
    // Hovering a legend row haloes its notes for as long as the pointer is on it. A
    // SEPARATE axis again, and transient: it never survives a rebuild of the legend and
    // it is never persisted, so it cannot leave the disc in a state nobody chose.
    hoverGroup: null,   // group name under the pointer, or null
    // Path keys under the pointer, as a SET rather than one string. A depth-1 row can
    // stand for several subfolders at once -- the "N smaller subfolders" tail row carries
    // every index it pools -- so one row is not one key, and hovering it has to light all
    // of them or it lights the wrong part of the wedge it points at.
    hoverSub: Object.create(null),
    // Every group starts COLLAPSED, so the legend opens as a list of the vault's
    // top-level folders and nothing else. It used to open with one level of subfolders
    // showing, on the reasoning that that level is what the pie already draws as
    // sub-wedges -- but on this vault that is 24 rows before you have asked anything,
    // and the folder names it is trying to show are the ones that get truncated. The
    // tree is still one click deep; it just is not unfolded for you.
    //
    // Filled by regroup(), which is the first place the group names exist.
    collapsed: Object.create(null),   // group -> true: its subfolder rows folded away
    tailOpen: Object.create(null),    // group -> true: "N smaller subfolders" unfolded
    // "PARA/a/b/..." -> true: that folder's children are unfolded. Any depth; the tree
    // comes from each note's own `dirs` chain, so nothing here assumes a level count.
    pathOpen: Object.create(null),
    selected: null,
    hovered: null,
    // Days marked on the heatmap: one picked by clicking, one under the pointer.
    // Both halo their notes without moving them (see isPushed), and neither is a
    // visibility filter.
    //
    // markDay IS THE WHOLE OF "MARK TODAY" NOW. There used to be a separate `markToday`
    // flag behind a sidebar button, and clicking the band's today column already did the
    // same job by the same predicate -- `created === the key`, which for that column IS
    // today. Two controls answering one question, one of which had to be found in the
    // sidebar while the answer was drawn in the band. The button went; the band is the
    // control, so the fill treatment the button owned moved onto the picked day -- see
    // nodeStyle.
    markDay: null,
    hoverDay: null,
    // The year label under the pointer, if any. Same shape as hoverDay and read the same
    // way -- a transient halo answering "where did this year go", with no push.
    hoverYear: null,
    query: "",
    until: null,        // timeline: reveal the oldest N notes, or null for all
    // DATE RANGE CAP. Two ends, either of which may be null for "no bound", in ms UTC at
    // midnight. Separate from `until` on purpose: that one reveals the oldest N notes and
    // is a growth animation, this one is a filter. They compose -- see timeFactor.
    from: null,
    to: null,
    // The RIGHT EDGE of the heatmap's 52-week window, or null for "the last 52 weeks".
    // The band was a fixed sliding window onto today, which is what made everything before
    // it unreachable: on the 10-year fixture that is nine years of the vault with no way to
    // point at it. Concepts that move the window write this.
    heatEnd: null,
    // Bow links away from the hub instead of chording across it. 91% of links cross
    // the disc, so straight is the case that would need the excuse. No longer a
    // control: these two are fixed, and the code paths for false are kept only
    // because flipping either here is still the way to compare the two renderings.
    curveEdges: true,
    // Logo colouring: true = the inner band's palette in the middle, fading out into
    // the outer's. false = the outer ring's palette across the whole mark.
    logoTwoRing: true,
    // The notes pinned into the hub, in slot order. The mark yields to them: an empty
    // list is the brain, a non-empty one is notes.
    pinned: []
  };

  /* ------------------------------------------------- graph + base layout */

  var graph = new Graph({ type: "undirected" });
  var N = DATA.nodes.length;

  DATA.nodes.forEach(function (n, i) {
    graph.addNode(String(i), {
      label: n.label, x: 0, y: 0, size: 4,
      folder: n.folder, sub: n.sub || "", dirs: n.dirs || [], ntype: n.type || "note",
      tags: n.tags || [], path: n.id, deg: n.deg,
      created: n.created || "", touched: n.touched || "",
      words: n.words || 0, ghost: !!n.ghost,
      sig: n.sig || "", lang: n.lang || ""   /* codanna: signature + language, for the detail panel */
    });
  });
  /**
   * THE RESTING WEB IS A BUDGET, and the share it buys shrinks as the vault grows.
   *
   * Sigma pays for every edge in the graph on every refresh, and hiding them does not help:
   * measured on the 10k fixture (36k links), a refresh costs 30.2ms as shipped, still 25.6ms
   * with the reducer short-circuiting every edge to hidden -- 36k reducer calls and sigma's
   * own per-edge iteration cost ~18ms even when nothing is drawn. Only absence is cheap. So a
   * vault past EDGE_RAMP_START draws only its strongest links at rest, chosen by weight, and
   * the rest exist only in the adjacency map -- until a hover or selection materialises the
   * focused note's complete web on top (see syncLazyEdges), which is what makes the omission
   * safe: every link is still reachable, it is just quiet until pointed at.
   *
   * The share ramps rather than switching: everything renders up to EDGE_RAMP_START, then the
   * share falls linearly to EDGE_FLOOR at EDGE_RAMP_END and holds there, so even a monster
   * keeps a skeleton of its strongest structure. Against the vaults this repo is tuned by:
   * the real vault (1,576 links) renders 100%; the demo vault (4,902) renders its strongest
   * 67% (~3.3k); the 10k fixture (37,373) renders 10% (~3.7k). Weight-descending selection
   * means what survives is the structure a reader would trace anyway -- at 36k the full web
   * reads as fog, and the fade-in of 36k links during a cascade is a wall of movement behind
   * the notes.
   */
  var EDGE_RAMP_START = 2000, EDGE_RAMP_END = 10000, EDGE_FLOOR = 0.10;
  /** id -> [{ o: neighbour id, w: link weight }], both directions, deduped. The one source of
   *  neighbourhood truth in both modes -- neighboursOf() reads this, never graph.neighbors(),
   *  so the halo, the focus web and the detail panel work identically whether or not the
   *  edges are materialised. */
  var adj = Object.create(null);
  var EDGE_TOTAL = 0;
  var edgeAttrsOf = function (w) { return { weight: w, size: Math.min(1.6, 0.35 + w * 0.25) }; };
  // codanna: edge strokes are capped in PIXELS. Under the identity zoomToSizeRatioFunction
  // (drawn size = size / ratio, the law the positions follow) a weight-1.6 edge draws 8px
  // wide at 5x zoom and 16px at 10x, and a hub's fan of curves merges into a ribbon wider
  // than the discs it joins. Dots must keep tracking the lattice (measureSizeScale explains
  // why a pixel cap on dots reads as swelling-then-dwindling), but connectors must stay
  // lines: the edge reducer clamps size to EDGE_MAX_PX * ratio, so the drawn stroke never
  // passes EDGE_MAX_PX. The clamp can only bind once ratio < 1.6 / EDGE_MAX_PX; the camera
  // hook re-runs the reducers only around that region, so resting zooms cost nothing.
  var EDGE_MAX_PX = 4;
  var edgeCapBinds = function (ratio) { return ratio < 1.6 / EDGE_MAX_PX; };
  var EDGE_SHOWN = 0;
  var lazyEdges = false;   // true when the resting web is partial; probes read this
  (function () {
    var seen = Object.create(null), list = [];
    DATA.edges.forEach(function (e) {
      var a = String(e.s), b = String(e.t);
      var k = a < b ? a + "\u0000" + b : b + "\u0000" + a;
      if (seen[k]) return;
      seen[k] = 1;
      EDGE_TOTAL++;
      list.push({ a: a, b: b, w: e.w, k: k });
      // codanna: the relation/direction histogram rides on the adjacency, not on edge
      // attributes -- in a budgeted vault the trimmed edges never materialise, and the
      // detail panel reads the neighbourhood from here. Each entry's `r` is normalised
      // to THIS node's perspective: r[Relation] = [outgoing, incoming].
      var rf = e.r || null, rb = null;
      if (rf && b !== a) {
        rb = {};
        for (var rel in rf) rb[rel] = [rf[rel][1], rf[rel][0]];
      }
      (adj[a] || (adj[a] = [])).push({ o: b, w: e.w, r: rf });
      if (b !== a) (adj[b] || (adj[b] = [])).push({ o: a, w: e.w, r: rb });
    });
    var share = EDGE_TOTAL <= EDGE_RAMP_START ? 1
      : EDGE_TOTAL >= EDGE_RAMP_END ? EDGE_FLOOR
      : 1 - (1 - EDGE_FLOOR) * (EDGE_TOTAL - EDGE_RAMP_START) / (EDGE_RAMP_END - EDGE_RAMP_START);
    EDGE_SHOWN = Math.round(EDGE_TOTAL * share);
    lazyEdges = EDGE_SHOWN < EDGE_TOTAL;
    if (lazyEdges) {
      // Strongest first; ties broken by key so the same vault always shows the same web --
      // a resting set that changed between loads would read as links appearing and vanishing
      // for no reason.
      list.sort(function (p, q) { return q.w - p.w || (p.k < q.k ? -1 : 1); });
      list.length = EDGE_SHOWN;
    }
    list.forEach(function (e) {
      if (!graph.hasEdge(e.a, e.b)) graph.addUndirectedEdge(e.a, e.b, edgeAttrsOf(e.w));
    });
  })();

  // Node area tracks link count; sqrt keeps hubs from swallowing the canvas.
  // The cap is not cosmetic: measured row spacing in the Rings layout is ~28px, so
  // a radius above ~13 makes neighbouring notes in the same column overlap. 11
  // leaves a visible gap while still giving a 4x range between a leaf and a hub.
  var NODE_MIN = 2.6, NODE_MAX = 11, NODE_ORPHAN = 6;
  graph.forEachNode(function (id, a) {
    // Unlinked notes would take the smallest size of all and vanish in the hub,
    // which is the opposite of useful -- being unlinked is the thing to notice.
    // They all have degree 0, so a fixed size says "special case", not "important".
    graph.setNodeAttribute(id, "size", a.deg === 0
      ? NODE_ORPHAN
      : Math.min(NODE_MAX, NODE_MIN + 1.55 * Math.sqrt(a.deg)));
  });

  measureDotTyp();

  // Which notes deserve a permanent label: strictly the best-connected ones.
  // Sigma's own label thinning is grid-based, which assumes nodes are spread out --
  // false by construction in Rings, where every hub is packed into the centre so they
  // all compete for one grid cell. That made the choice effectively arbitrary and
  // could drop rank 1 entirely, so the ranking is decided here. Ties break on label
  // so the labelled set is identical on every reload.
  var hubRank = Object.create(null);
  (function () {
    graph.nodes().slice().sort(function (a, b) {
      return graph.getNodeAttribute(b, "deg") - graph.getNodeAttribute(a, "deg") ||
             String(graph.getNodeAttribute(a, "label"))
               .localeCompare(String(graph.getNodeAttribute(b, "label")));
    }).forEach(function (id, i) { hubRank[id] = i; });
  })();

  // Stable subfolder ordering per PARA folder: biggest first, name as tie-break.
  var subOrder = Object.create(null);
  // "folder/sub" -> note count, from the SAME tally subOrder sorts by -- not a second
  // graph walk. buildLegend used to keep its own local copy of this, recomputed on every
  // render; it now reads this one, and the settings panel and the __vg api do too, since
  // all three need to say how big a subfolder is.
  var subCount = Object.create(null);
  (function () {
    var tally = Object.create(null);
    graph.forEachNode(function (_id, a) {
      var f = a.folder, sb = a.sub || "";
      if (!tally[f]) tally[f] = Object.create(null);
      tally[f][sb] = (tally[f][sb] || 0) + 1;
    });
    Object.keys(tally).forEach(function (f) {
      subOrder[f] = Object.keys(tally[f]).sort(function (x, y) {
        return tally[f][y] - tally[f][x] || x.localeCompare(y);
      });
      subOrder[f].forEach(function (sb) { subCount[f + "/" + sb] = tally[f][sb]; });
    });
  })();

  var base = {};   // id -> {x, y} from ForceAtlas2, before any group separation

  // Graph-space distance for one layout unit (one row, one note along a row).
  // Fixed on purpose: see the note where it is used.
  var UNIT = 160;

 
 
  /* -------------------------------------------------------------- grouping */

  // Unlinked notes are their OWN GROUP, not their folder's. They used to be
  // sunflower-packed into the hub hole, which put them nowhere the rest of the language
  // applies: no wedge, no legend row, no way to filter or count them, and their folder
  // silently under-reported its size. As a group they get all of that, and they land in
  // whichever band their size earns like anything else.
  //
  // Named in parentheses so it sorts with "(vault root)" ahead of the numbered folders,
  // and so it cannot collide with a real folder name.
  var UNLINKED = "(unlinked)";
  function groupOf(id) {
    // From the ADJACENCY, not graph.degree(): in a budgeted vault the graph holds only the
    // resting share of the web, so a note whose links were all trimmed would read as unlinked
    // -- measured with the graph fully empty, all 10k notes classified as (unlinked), the
    // legend collapsed to one eye and the disc laid out as a single wedge. adj always holds
    // every link, so the answer is the same in every mode.
    if (!adj[id]) return UNLINKED;
    return graph.getNodeAttribute(id, "folder");
  }

  // Colour is assigned from FULL-dataset group sizes and cached per dimension, so a
  // filter never repaints the survivors.
  // Twelve categorical slots -- ten hues and two greys -- at the author's request. For
  // the record: only four hues clear the all-pairs colour-vision gate on freely-scattered
  // marks, and twelve cannot. What makes it workable here is that colour is NOT the only
  // channel: each group owns a contiguous wedge separated by a 2 degree gap, carries a
  // label on the rim, and is listed in the legend with its count.
  var SLOT_COUNT = 12;
  var groupColor = Object.create(null);   // group -> literal hex, rebuilt on regroup
  var groupSlot = Object.create(null);    // group -> slot key it is on, "" for none
  // group -> the slot it would be on with no override at all. Recorded rather than left
  // to be recomputed, for the same reason groupSlot is: an override discards the
  // automatic key right after using it to advance the rotation counter, and nothing else
  // reproduces "i % 12, archives skipped" without duplicating the loop that does it.
  var groupAutoSlot = Object.create(null);
  var order = {};   // dim -> [group names, biggest first]

  function computeOrder() {
    var count = {};
    graph.forEachNode(function (id) {
      var g = groupOf(id);
      count[g] = (count[g] || 0) + 1;
    });
    // Name order, not size order. For PARA folders that is their numbered order
    // (01, 02, 03, ...), so wedges run round the disc in the same sequence as the
    // vault's own folder list and a group keeps its colour as the vault grows.
    // Note: subfolder order stays size-based -- the "N smaller subfolders" fold
    // depends on knowing which are smallest.
    var names = Object.keys(count).sort(function (a, b) {
      // THREE RANKS, and the reason is that neither `_` nor `(` is a real folder name
      // competing with the others. Archives first, then the pseudo-folders --
      // "(vault root)" for notes sitting loose at the top and "(unlinked)" for notes
      // nothing points at -- then everything the vault actually filed. So the entries
      // that are not part of the working set stay together at the head of the list
      // instead of one of them landing above the archives and one below.
      //
      // This does not move any colour: archives take the grey slot without consuming
      // one, so the first pseudo-folder is still the first group in the rotation.
      var rank = function (s) {
        var c = s.charAt(0);
        return c === "_" ? 0 : c === "(" ? 1 : 2;
      };
      return rank(a) - rank(b) || a.localeCompare(b, undefined, { numeric: true });
    });
    order[state.dim] = names;
    return count;
  }

  var counts = {};
  function buildColors() {
    groupColor = Object.create(null);

    var names = order[state.dim] || [];

    // IT GOES ROUND. Folder n takes slot n, and folder 13 comes back to slot 1.
    //
    // It used to stop at ten and drop everything past that into the neutrals, on the
    // reasoning that a repeated hue is a lie about identity. That trade is the wrong way
    // round: a repeat is still separated by its wedge, its rim label and its legend row,
    // whereas the grey tail was one undifferentiated blob where nothing was separated
    // from anything. The greys are slots 11 and 12 now, so grey is still reachable --
    // as a choice, for a folder that should recede, rather than as what you get for
    // being thirteenth.
    //
    // Overrides only apply to the folder dimension: they are keyed by folder name, and
    // any other grouping would be matching those names against something else entirely.
    var byFolder = state.dim === "folder" ? folderColors : Object.create(null);

    // AN OVERRIDE CHANGES EXACTLY ONE FOLDER. Position decides every other colour, and
    // nothing here looks at what anyone else picked.
    //
    // The first version tried to keep the palette a bijection: an override claimed its
    // slot, the automatic run stepped over it, and picking a slot another folder held
    // swapped the two. Both halves were wrong, and for the same reason -- they treated
    // "two folders share a hue" as a mistake to prevent. It is a choice. Grouping three
    // folders under one colour to say they belong together is a thing to want, and the
    // clever version made it unreachable while also moving folders nobody touched: one
    // pick could re-seat four other wedges, so the disc repainted around the one change
    // you were looking at.
    //
    // So: no claiming, no stepping over, no swapping. Duplicates are allowed because
    // they are allowed to be deliberate.
    //
    // ARCHIVES ARE OUT OF THE ROTATION, and that is why the counter is separate from the
    // index. Spending a hue on `_ Archives` costs twice: the archive gets a colour that
    // says "look at me", and every folder after it is pushed a slot along, so which hue a
    // working folder gets depends on how many archives happen to sort before it. A
    // recessive grey and no slot consumed fixes both. An explicit pick still wins -- the
    // rule is a default, not a prohibition.
    // WHICH SLOT EACH GROUP ENDED UP ON is recorded rather than recomputed. The settings
    // panels have to mark it, and the arithmetic is no longer `i % 12`: archives are
    // skipped, so the only thing that knows is the loop that did the skipping. Both UIs
    // read it back through the api.
    groupSlot = Object.create(null);
    groupAutoSlot = Object.create(null);
    var auto = 0;
    names.forEach(function (g) {
      var k = byFolder[g];
      var picked = (k && THEME.byKey[k]) ? k : "";

      // ARCHIVES NEVER CONSUME A SLOT, with or without a pick of their own. That is the
      // whole of "out of the rotation": `auto` does not advance here, so which hue a
      // working folder gets cannot depend on how many archives sort before it.
      if (isArchiveGroup(g)) {
        var akey = picked || ARCHIVE_SLOT;
        groupColor[g] = THEME.byKey[akey];
        groupSlot[g] = akey;
        // An archive's automatic slot is ARCHIVE_SLOT unconditionally -- an override
        // changes what it's using, never what "automatic" means for it.
        groupAutoSlot[g] = ARCHIVE_SLOT;
        return;
      }

      // EVERY OTHER FOLDER CONSUMES ITS POSITION, whether or not it uses the colour that
      // position hands it. `auto++` happens before the pick is considered, deliberately.
      //
      // Returning early on a pick -- which is how this read when the archive counter went
      // in -- silently reintroduced the blast radius this whole design exists to avoid:
      // an overridden folder did not advance the counter, so every folder after it slid
      // one slot along. Measured on the 17-folder vault, one click on one folder
      // recoloured FOURTEEN groups and repainted 624 notes outside the folder touched.
      // It was index-based (`i % SLOT_COUNT`) before the counter, which had this right by
      // construction; the counter has to be told.
      var key = "g" + ((auto++ % SLOT_COUNT) + 1);
      var use = picked || key;
      groupColor[g] = THEME.byKey[use];
      groupSlot[g] = use;
      // `key`, not `use`: the pick already won for groupSlot above, but the automatic
      // slot is the position's own answer regardless of what's currently applied.
      groupAutoSlot[g] = key;
    });
    buildSubShades();
  }

  // The settings UIs need three things and none of them should reach into internals:
  // what can be picked, what is picked now, and what the groups are called.
  function paletteInfo() {
    return SLOT_NAMES.map(function (name, i) {
      return { key: "g" + (i + 1), name: name, hex: THEME.slots[i] };
    });
  }

  // Apply a new folder -> slot map and repaint everything that carries a group colour.
  //
  // A rebuild is NOT needed and is not done: colour is not an input to the layout, so the
  // ring plan, the band lock and every node position are untouched. What does have to be
  // told is each thing that cached a colour of its own -- the renderer's node reducers,
  // the logo's conic gradient, the heatmap's per-day mix, and the legend's swatches.
  // Visibility defaults do not touch colour, so this only replaces the map. The caller
  // applies it to the live filter -- see pickVisible -- because a *default* changing and
  // the disc changing are two decisions, and only one of them belongs to a host that is
  // loading saved settings at boot.
  function applyFolderShown(map) {
    folderShown = cleanFolderShown(map);
    return folderShown;
  }

  function applyFolderColors(map) {
    folderColors = cleanSlotMap(map);
    buildColors();
    if (renderer) renderer.refresh();
    try { placeLogo(); } catch { /* logo not mounted yet */ }
    try { heatBuild(); } catch { /* heatmap not built yet */ }
    try { buildLegend(); } catch { /* legend not built yet */ }
    return folderColors;
  }

  // As applyFolderColors, one level down. buildColors() is NOT called -- a subfolder pin
  // cannot change any group's own colour, so re-deriving groupColor would be repainting
  // things that did not change. buildSubShades() alone is enough, and it is also what
  // applyFolderColors ends in (via buildColors()), which is why a folder recolour keeps
  // respecting every subfolder pin without this file needing to say so twice.
  function applySubfolderColors(map) {
    subfolderColors = cleanSlotMap(map);
    buildSubShades();
    if (renderer) renderer.refresh();
    try { placeLogo(); } catch { /* logo not mounted yet */ }
    try { heatBuild(); } catch { /* heatmap not built yet */ }
    try { buildLegend(); } catch { /* legend not built yet */ }
    return subfolderColors;
  }

  // Whether ANY of g's subfolders carries a pin right now. Both nesting gates below
  // read this, alongside the ordinary "more than one subfolder" test -- without it, a
  // pin outlives its own picker: pin a folder's one differentiated subfolder, then let
  // attrition (notes moved elsewhere) shrink that folder to a single subfolder, and the
  // twisty that used to reach the pin disappears from the legend, the settings panel
  // AND the Obsidian settings tab at once, with the pin still silently in effect and no
  // remaining UI to clear it short of "Reset all", which drops every override in the
  // vault. A single differentiated subfolder is ordinarily nothing to nest -- see
  // buildSubShades' own `subs.length < 2` skip -- but a PIN is a deliberate choice, made
  // through a UI that has to stay reachable for as long as the pin exists.
  function groupHasPinnedSub(g) {
    return (subOrder[g] || []).some(function (sb) {
      return !!subfolderColors[g + "/" + sb];
    });
  }

  function colorOf(group) {
    return groupColor[group] || THEME.neutrals[0];
  }

  // Subfolders get a tint of their PARA folder's colour, not a colour of their own:
  // hue rotated a few degrees with a small lightness step, so "03 - Resources" still
  // reads as one family while People / Partners / Rezepte separate inside it. This is
  // deliberately BELOW the categorical separation floor -- it is a nested cue on top
  // of an already-labelled group, not an independent colour channel.
  //
  // ...UNLESS subfolderColors pins one, which skips the ladder entirely and paints it
  // one of the twelve slots instead -- a full-strength colour with none of the
  // "below the floor" restraint above, because a pin is a deliberate choice to make one
  // subfolder stand out, not a nested cue.
  var subShade = Object.create(null);
  // "folder/sub" -> the slot key it is ON: the pinned one, or "" for the automatic
  // ladder. "" on purpose rather than omitting the key -- an automatic tint is never one
  // of the twelve slot hexes, so there is nothing among them to ring as "current", and
  // the settings UIs need to say that rather than guess.
  var subSlot = Object.create(null);

  // Four steps, not one per subfolder. Spreading N tints evenly across one hue
  // family collapses as N grows -- measured, nine subfolders land ~3 apart, below
  // the just-noticeable threshold, so nothing is actually differentiated. Four
  // steps hold adjacent separation at 6-10 (OKLab dE x100), which is legal given
  // the legend swatches and tooltips that name the subfolder. Subfolders past the
  // third therefore SHARE the last step; the legend says so rather than implying
  // each has its own shade.
  // Stepping symmetrically around the base colour forces one end toward the
  // surface and it loses contrast -- measured, the darkest step fell to 2.25:1 on
  // the dark surface. So the ladder always steps AWAY from the surface: lighter on
  // dark, darker on light. That buys a starker spread (adjacent dE 5.6-9.5, up from
  // ~3) AND better contrast, and it means the biggest subfolder keeps the folder's
  // own colour instead of being tinted.
  // How far a family may rotate is NOT a global constant: measured against this
  // palette, blue has 106deg of hue to its nearest neighbour but yellow only 69,
  // so a fixed rotation either wastes blue's headroom or turns yellow green. Each
  // family gets 40% of its own half-gap, computed from the live palette.
  var SLICE_GAP = 2;      // degrees of empty space between GROUPS
  // ...and between sub-wedges INSIDE a group. Smaller than SLICE_GAP on purpose:
  // the group boundary has to stay the more prominent of the two, or the disc reads
  // as one flat ring of subfolders rather than folders that contain them. The tint
  // ladder already separates sibling sub-wedges; this just stops them touching.
  var SUB_GAP = 0.3;
  // An angular gap cannot solve edge collisions on its own, and the arithmetic says
  // why: `u` is the fraction ACROSS a row, so a row's first and last notes sit right
  // on the wedge edges, and all that separates them from the neighbouring cell's
  // edge notes is the gap's arc -- 1 degree at r=1334 is 23 graph units against the
  // ~156 that two max-size dots need. Even 2 degrees only reaches 46.
  //
  // So notes are also held off the edges by an absolute ARC, converted to a
  // fraction of the cell's own reference width. Capped, because a narrow wedge
  // cannot afford much: without the cap a 16-note cell wanted 30% of its span at
  // each end, which squeezes its interior enough to trade edge collisions for
  // interior ones.
  // Tuned by sweeping, not derived -- because the pad trades three things off at
  // once (edge collisions, interior collisions, and how ragged the disc's outer edge
  // gets) and only measurement finds the corner. Swept at a fixed window:
  //
  //   arc  max   cross  interior  outer-radius spread  rows
  //     0  0     1      0         800  (baseline)      7-7
  //    25  0.08  0      0         800  (baseline)      7-8
  //    35  0.10  0      0         800  (baseline)      7-8   <- chosen
  //    55  0.16  0      0         960                  8-9
  //    70  0.22  0      0         1280                 8-11
  //
  // 70/0.22 was the first guess and it was wildly over-specified: it cleared the
  // collisions but pushed narrow cells from 8 rows to 11, which is the sparse-spoke
  // failure mode, and added 480 units of raggedness. 35/0.10 clears them at the
  // baseline 800 spread -- i.e. for free, since that raggedness exists with no pad
  // at all (cells' last rows are partly filled, so their outer radii differ anyway).
  // BOTH ZERO, and that is the point: within-row centring in placeCell now provides
  // the edge clearance for free, so the inset is not needed at all. Measured with the
  // pad at 0 AND the gap at 0 degrees: 0 collisions, 0 off-lattice rows of 418. The
  // mechanism is left in as a safety valve, and because the sweep that retired it is
  // worth keeping (see the table in the note).
  var EDGE_PAD_ARC = 0;
  var EDGE_PAD_MAX = 0;
  // Minimum wedge width. Not cosmetic: at the hub radius an arc of MIN_SPAN has to
  // be wide enough that two neighbouring cells' innermost notes do not touch.
  // Measured, 1.4 degrees left them ~9px apart against ~17px of node; 6 clears it.
  // The cost is proportional honesty at the bottom end -- the five smallest groups
  // take ~8% of the circle for 2.3% of the notes.
  // The inner ring is drawn at this fraction of its packed radius. Rows and
  // capacities are computed on the unscaled geometry and the whole band is scaled
  // afterwards, so the proportions are untouched -- it is purely a size trim.
  var INNER_SCALE = 0.8;
  // How much of the locked hub-to-ring span the inner band fills; the rest is the channel
  // between the rings. At module scope because the sub-split gate needs a band's depth before
  // the band is packed, which is above where this used to live.
  var INNER_FILL = 0.8;
  /**
   * How wide the channel between two wedges is, per band.
   *
   * The inner ring reads gappier than the outer at the same width, because the same channel is
   * a larger share of a shorter row: at half the radius a wedge holds half the notes, so a gap
   * of a given width is twice as much of what is around it. Half for the inner band, by
   * measurement of how it looks rather than by derivation.
   */
  var GAP_BAND = { i: 0.5, o: 1 };
  /**
   * The visible channel's half-width, as a fraction of the band's own room -- so the gap scales
   * with the lattice rather than being a fixed number of units that means something different
   * on every vault.
   *
   * This is a CLEARANCE between dot RIMS, which is the thing a person sees. It used to be a
   * margin to dot CENTRES, and centres are the wrong reference: dot size varies with link
   * weight, so equal centre spacing leaves unequal gaps. Measured edge to edge on the 10k
   * vault, the channel ran 28 to 89 units across the rows of one boundary -- a 3.2x variation
   * that reads as a ragged seam.
   */
  var CLEAR_OF_ROOM = 0.12;

  var MIN_SPAN = 6 * Math.PI / 180;
  // Nest whenever a group HAS subfolders. This used to require 12+ notes, because
  // back when the minimum wedge was 1.4 degrees a 1-note subfolder became an
  // invisible sliver. MIN_SPAN is 6 degrees now, which guarantees every sub-wedge
  // is visible, so the threshold only served to hide real structure -- the quarter
  // folders under 05 - Weekly Reviews (10 notes) never showed up.
  // How far a highlighted note steps out, in ROWS (SP = 1, so 0.9 is just under one
  // row of spacing). Sized against the headroom that already exists rather than by
  // eye: the normalisation box is pinned at maxR * 1.02 and fit() frames at 1.08, so
  // there is ~6% of slack outside the outermost notes. 0.9 rows on a ~13.3-row disc
  // is 6.8% -- enough to read as protruding, small enough that the pushed wedge does
  // not need the box widened, which would shrink the resting disc for everyone.
  var HL_PUSH = 0.9;

  // How far the density solve is allowed to spread the lattice (github#13). sqrt has no
  // ceiling of its own, so isolating one folder out of a 1500-note vault would otherwise
  // ask for a spacing of 30 and draw a handful of boulders on the rim. 2.6 lets a vault
  // be filtered to ~15% of itself before the disc starts shrinking again instead of
  // spreading -- which covers isolating a single PARA folder, the gesture this is for.
  var DENSITY_MAX = 2.6;



  // ONE DESCRIPTOR PER BAND, and the reason it exists is a tally rather than a preference.
  //
  // The two rings are packed independently -- a single spacing made each answer for the other's
  // filtering, which closed the gap between them from 843 units to 89 -- so every quantity
  // below is per band. Held as PAIRS of scalars, which is what this replaces, the unqualified
  // name meant the outer band and the inner one needed an `I`. Six bugs in one session were a
  // site reading the unqualified name:
  //
  //   the seam pitch, sized by the outer ring's spacing on both rings;
  //   the dot ramp, calibrated from the outer spacing and applied to both, so the inner ring's
  //     notes touched (clearance 29) while the outer ring gapped (239);
  //   pitchUnits()'s argument omitted, defaulting to the outer band;
  //   dotFit's room compared against the outer pitch inside the inner band;
  //   the band depth feeding seamFall taken as an integer rather than the walked value;
  //   the pixel floor scaled by the room in one place and not the other.
  //
  // Every one of those is unwriteable against a keyed object: there is no bare name to reach
  // for, so the band has to be named, so the wrong band has to be chosen deliberately.
  //
  //   sp      the lattice spacing this band's last plan used -- a ROW's width, not a lattice
  //           unit's. They were the same number until the density solve, and conflating them
  //           is why dot size did not respond to filtering at all.
  //   rows    how deep the band is RIGHT NOW. The seam falls off against this rather than the
  //           locked full-vault depth: filter a 10,000-note vault to 500 and it should read
  //           like a 500-note vault, gaps included.
  //   room    the tangential step a note has, as a low percentile over the band. See dotPx.
  //   ramp    display px = m * attr size + b, never below lo. Per band, because a dot is a
  //           fraction of ITS OWN pitch.
  //   gapDeg, nG   the seam width and the seam count, for the probe and the report.
  //
  // The factor a band's radii are DRAWN at is bandScale(), a function rather than a field --
  // see below for why. REF_ROWS and DOT_MIN_PX are declared further down the file, so the
  // fields that would default to them start at 0 and the readers carry the fallback, exactly
  // as `lastRows[k] || REF_ROWS` did.
  //
  // BUILT ON FIRST USE, not at this statement, because this statement is not reached first.
  // mountVaultGraph calls measureDotTyp near the top of its body, which reaches dotUnits and so
  // pitchUnits -- above every one of these declarations. The scalars this replaces survived that
  // by accident: `(lastSPI || 1)` reads undefined and carries on, where `BAND.i` on undefined
  // throws, so the page did not boot at all on the first attempt at this change.
  //
  // A function declaration is hoisted and a var initialiser is not, so the construction lives
  // inside bandOf and every writer goes through it too. Nothing may touch BAND directly.
  var BAND = null;
  function bandOf(k) {
    if (!BAND) {
      BAND = {
        i: { key: "i", sp: 1, rows: 0, room: 0, ramp: { m: 1, b: 0, lo: 0 }, gapDeg: 0, nG: 0 },
        o: { key: "o", sp: 1, rows: 0, room: 0, ramp: { m: 1, b: 0, lo: 0 }, gapDeg: 0, nG: 0 },
      };
    }
    return k === "i" ? BAND.i : BAND.o;
  }
  // The factor a band's radii are DRAWN at, read rather than stored for the same reason:
  // INNER_SCALE is declared below the first caller too, so a descriptor built early would
  // capture undefined and keep it for the session.
  function bandScale(k) { return k === "i" ? INNER_SCALE : 1; }

  // ONE ROW OF THE LATTICE AT FULL VAULT, IN GRAPH UNITS -- and deliberately NOT the live
  // pitch, which is a different number at every filter state.
  //
  // The seam and a wedge's end margins are quoted in rows, so they need a row to measure
  // against. Using the LIVE pitch was the obvious reading and is wrong twice over:
  //
  //   * it makes the gap a function of the filter, which is the thing three earlier fixes were
  //     about. Toggling one folder visibly moved every channel on the disc -- reported as "you
  //     can see the gaps moving when toggling 08 Meeting Notes".
  //   * the live pitch is PER BAND now, and there is only one lastSP. It holds the outer band's,
  //     so the inner ring's seams were sized by the outer ring's spacing: hide outer folders,
  //     SP_O climbs, and the inner ring's gaps grow for a reason that has nothing to do with
  //     the inner ring. Reported as the inner gaps growing while outer folders were toggled.
  //
  // The full-vault pitch is one number, the same in both bands, and it does not move. A gap is
  // a statement about the vault's structure, so that is the right clock for it. What DOES scale
  // with the live lattice is the dots -- see dotUnits, which is about a dot and keeps the live
  // pitch on purpose.
  //
  // TIMES INNER_SCALE IN THE INNER BAND, which is the factor that made the inner ring's
  // channels read as much too wide. Inner radii are DRAWN at INNER_SCALE, so the inner band's
  // real step is 0.8 of its lattice pitch -- measured, 117 units against the outer band's 159.
  // Spending a full UNIT of margin there put the zero point alone at 160/117 = 1.37 steps,
  // before any seam: a boundary wider than a note-to-note gap by half again, purely from using
  // the wrong ring's ruler. Measured channels per band, before: inner 1.53/1.69/1.37 against
  // outer 1.27/1.20/1.07 on the three vaults.
  //
  // THE LIVE PITCH, per band, and times INNER_SCALE in the inner one. A seam is a distance
  // between notes, so it belongs on the same ruler the notes are spaced by -- and that ruler is
  // the band's own live spacing. The locked pitch was tried and is wrong in the direction that
  // was not being watched: it holds a filtered 10,000-note disc to the seam its full self
  // earned, when what a person sees on screen is 500 notes that should look like 500 notes.
  //
  // Per band and never one shared number, which is the error that made the inner ring's gaps
  // grow while outer folders were toggled.
  function pitchUnits(band) {
    return UNIT * (bandOf(band).sp || 1) * bandScale(band);
  }

  var NEST_MIN = 2;
  // Group folding is OFF now that there are 10 hues: it existed only because
  // groups past slot 4 all shared one grey and merged into an unreadable mass.
  // With its own colour, its own 2-degree gap and its own label, a 3-note group
  // reads fine as a thin slice. Set this above 0 to bring the shared wedge back.
  var SMALL_GROUP = 0;
  var SUB_SLOTS = 4;
  var SUB_NAMED = 3;
  var HUE_BUDGET_FRACTION = 0.60;   // share of its half-gap a family may rotate
  var SUB_L_SPAN = 0.28;            // how far the ladder travels in lightness
  var SUB_L_LIMIT = 0.90;           // stop before the top step washes out

  function hueOf(hex) {
    var l = hex2lab(hex);
    return ((Math.atan2(l[2], l[1]) * 180 / Math.PI) % 360 + 360) % 360;
  }
  // Degrees this group may rotate before it starts impersonating another group.
  function hueBudget(basecol) {
    var h = hueOf(basecol), gap = 180;
    Object.keys(groupColor).forEach(function (g) {
      var c = groupColor[g];
      if (c === basecol) return;
      var lab = hex2lab(c);
      if (Math.hypot(lab[1], lab[2]) < 0.02) return;   // neutrals have no hue to clash with
      var d = Math.abs(h - hueOf(c));
      d = Math.min(d, 360 - d);
      if (d < gap) gap = d;
    });
    return gap * HUE_BUDGET_FRACTION;
  }

  function subTintIndex(folder, sub) {
    var subs = subOrder[folder] || [];
    var k = subs.indexOf(sub || "");
    return k < 0 ? 0 : Math.min(k, SUB_SLOTS - 1);
  }

  // WHICH CELL A NOTE'S SUBFOLDER MAPS TO, kept separate from subTintIndex on purpose --
  // that function is also the source of a subfolder's COLOUR (buildSubShades), and a
  // subfolder's tint should not shift just because its geometric slot did. Below its band's
  // own row depth, a subfolder's notes redirect to the pooled-tail slot instead of earning
  // their own rank -- splitFor already refuses to split a FOLDER into sub-wedges when its
  // total can't fill the band's rows (github#18/#19 lineage); this is the same question per
  // SUBFOLDER, so a lone sparse piece doesn't get its own arc just because its siblings
  // earned the split (github#31).
  //
  // Both `n` and `depth` are the CALLER's numbers (buildWedgePlan's own liveSub and
  // bandDepth), taken as parameters rather than looked up here -- specifically because an
  // early version read the whole-vault subCount instead of a live count for `n`, and let a
  // subfolder clear the bar on notes that were not even on screen under a date range or
  // hidden-folder filter -- the same class of bug already documented above for the
  // folder-level gate ("it also cannot see a filter").
  function subCellIndex(folder, sub, n, depth) {
    var idx = subTintIndex(folder, sub);
    if (idx === SUB_SLOTS - 1) return idx;
    return n >= (depth || REF_ROWS) ? idx : SUB_SLOTS - 1;
  }

  function buildSubShades() {
    subShade = Object.create(null);
    subSlot = Object.create(null);
    Object.keys(subOrder).forEach(function (f) {
      var subs = subOrder[f];
      var basecol = colorOf(f);
      var lab = hex2lab(basecol);
      var grey = Math.hypot(lab[1], lab[2]) < 0.02;   // neutrals have no hue to turn
      // Lightness targets are spaced evenly between the base and a per-family end
      // point rather than being fixed deltas. Fixed deltas hit the wash-out cap and
      // the top two steps collapsed onto each other -- which is why simply turning
      // the numbers up made separation WORSE (adjacent dE 6.4 -> 4.1), not better.
      //
      // Computed once per folder, and only when a ladder will actually run -- a pin
      // never needs them, and neither does a folder with nothing to differentiate.
      var haveLadder = subs.length >= 2 && !grey;
      var sign = THEME.dark ? 1 : -1;
      var budget = haveLadder ? hueBudget(basecol) : 0;
      var Lend = haveLadder
        ? (THEME.dark ? Math.min(SUB_L_LIMIT, lab[0] + SUB_L_SPAN)
                      : Math.max(1 - SUB_L_LIMIT, lab[0] - SUB_L_SPAN))
        : 0;
      subs.forEach(function (sb) {
        var pk = f + "/" + sb;
        // A PIN SKIPS THE LADDER ENTIRELY, whether or not this folder has enough
        // subfolders to need one -- pinning the sole subfolder of a two-folder vault
        // is still a real thing to want.
        var pin = subfolderColors[pk];
        if (pin && THEME.byKey[pin]) {
          subShade[pk] = THEME.byKey[pin];
          subSlot[pk] = pin;
          return;
        }
        subSlot[pk] = "";
        if (subs.length < 2) return;             // nothing to differentiate
        if (grey) { subShade[pk] = basecol; return; }  // one lightness axis; ladder would collide
        var t = subTintIndex(f, sb) / (SUB_SLOTS - 1);
        subShade[pk] = shade(basecol, sign * budget * t, (Lend - lab[0]) * t);
      });
    });
  }

  // Group colour, tinted by subfolder when the folders are what we are looking at.
  //
  // UNLINKED IS A GROUP, NOT A FOLDER, and the folder dimension is the one place that can
  // forget it. Every other dimension asks groupOf; this one used to go straight to the
  // note's own folder, so a degree-0 note wore its folder's tint while the legend showed
  // it under one swatch -- measured 0 of 12 matching on a 700-note vault, 9 distinct
  // colours under a single legend row (github#3). `(vault root)` is NOT the same case:
  // the builder writes that as a real folder value, so colorOf resolves it already.
  function nodeColor(id) {
    var a = graph.getNodeAttributes(id);
    if (state.dim !== "folder") return colorOf(groupOf(id));
    if (groupOf(id) === UNLINKED) return colorOf(UNLINKED);
    return subShade[a.folder + "/" + (a.sub || "")] || colorOf(a.folder);
  }

  function isHidden(group) {
    var h = state.hidden[state.dim];
    return !!(h && h[group]);
  }

  /* --------------------------------------------------- positions per group */

 

  /* ------------------------------------------------------- rings layout */

  // A pie chart made of notes. Each group owns one wedge, its angle proportional
  // to the group's share of the vault; inside the wedge, notes fill concentric
  // rings from the middle outwards, best-connected first -- so hubs sit near the
  // hub of the disc and leaf notes land on the rim.
  //
  // Uniform packing density makes every wedge reach the same outer radius
  // regardless of its angle (area grows with the angle, and so does the node
  // count), which is what makes it read as a pie rather than a set of spokes.
  // group -> which ring it lives in, fixed for the life of this data. See the
  // band lock in buildWedgePlan.
  var bandLock = null;
  // The two bands' base radii, also fixed for the life of this data. The hub
  // radius used to be solved from the GLOBAL note count and the outer band's base
  // was derived from the inner band's row count, so enabling something in one
  // ring re-packed the other. Locking both makes the rings independent: each
  // one's rows depend on its own weights, at its own fixed base.
  var geomLock = null;
  /**
   * THE WEDGE OVERLAY -- wedge edges and band radii, drawn over the disc.
   *
   * Every animation question asked here so far ("does the wedge close at constant speed",
   * "does the neighbour stay put", "is the note centred in its wedge") is a question about
   * the ENVELOPE, and the envelope is the one thing the page never draws. Answering them
   * from note positions alone means re-deriving the boundary in a probe script, in a
   * different angle convention from the one placeCell uses -- which is exactly how a note
   * sitting dead centre in its wedge got reported as 100 degrees off it.
   *
   * So the layout hands its own boundaries out, and this draws them. `cells` is refilled by
   * every ringsLayout pass while `on`, in the SAME terms the placement uses (a seam count and
   * two wedge fractions, converted to an angle per radius by seamAt) -- not a copy of the
   * answer in another form. On by default in a --dev build; `?wedges` / `?nowedges` override.
   */
  var DBG = { on: false, cells: null, canvas: null };
  // The overlay's structural hue -- band radii and seam centres, the two things that belong to
  // the disc rather than to a folder. Kept as one constant so they cannot drift apart.
  var SEAM_YELLOW = "rgb(255,196,0)";
  var SEAM_YELLOW_45 = "rgba(255,196,0,0.45)";
  // Kept for reference: locking the packing width per cell made a folder's rows
  // immune to other folders, but density then never adapted -- see the row count
  // note in buildWedgePlan.
  var bandRefLock = null;
  // Also kept for reference: freezing each note's SLOT -- every note in the cell's
  // whole-vault list counting 1 and holding its place whether visible or not -- was
  // tried 2026-08-21 to stop a depth-2 subfolder toggle repacking its parent cell.
  // It did stop the repack, but a hidden note then leaves a hole instead of the cell
  // closing up, and the fade read worse than the movement it removed. Reverted. The
  // depth-2 radial movement is still open; see the note.
  var ringsMerged = Object.create(null);   // groups folded into the shared wedge
  var MERGED = "\u0001merged";

  // The wedge PLAN -- which notes sit in which cell, in which row, at which
  // fraction across the wedge -- is built from the whole vault and never from
  // what happens to be visible. That is the fix for notes appearing to swap
  // seats: hiding something changes each wedge's ANGLE, but never any note's
  // row or its position within the row, so a reflow is a rotate-and-stretch and
  // nothing crosses over anything else.
  // Sigma renders graph +y UPWARD, so a plain accumulating angle sweeps
  // anticlockwise starting at 6 o'clock. Layouts accumulate a "sweep" from 0
  // instead and convert here, which puts the first group at 12 o'clock and runs
  // clockwise -- the direction people read a pie chart.
  function sweepAngle(sw) { return Math.PI / 2 - sw; }
  function angleSweep(a) {
    var t = (Math.PI / 2 - a) % (2 * Math.PI);
    return t < 0 ? t + 2 * Math.PI : t;
  }

  // A note with no links has nothing to be near, so it goes in the hub hole rather
  // than on the rim. Coreness 0 and degree 0 are the same set -- any note with a
  // link survives the first peel -- so this is exactly the 0-core.
  function isOrphan(id) { return !adj[id]; }   // adjacency, not graph.degree -- see groupOf

  // THE SEAM IS A WIDTH, NOT AN ANGLE, and measuring it in degrees is why it looked wrong.
  //
  // SLICE_GAP was 2 degrees, tuned on a 450-note vault where that reads as a clean channel
  // between wedges. A degree is not a width, though: it buys arc in proportion to radius,
  // and the disc's radius grows with the vault. Measured at the outer ring:
  //
  //   454 notes    r 2141   2.000 deg    75 units   0.47 of a row pitch
  //   1402 notes   r 3879   1.911 deg   129 units   0.81 of a row pitch
  //   10001 notes  r 9885   0.000 deg     0 units   0.00
  //
  // On SCREEN all three were about the same 9px, because the camera fits the disc -- so the
  // setting looked consistent and was not. Against the lattice the notes actually sit on,
  // the same setting bought nearly twice the channel on the middle vault, which is exactly
  // where the gaps were reported as too big.
  //
  // So the seam is measured in ROW PITCHES -- the one length the whole disc is built on -- and
  // the angle falls out of the radius it sits at. That reproduces the old ramp's INTENT, the gap shrinking as the vault grows, as
  // arithmetic rather than as a hand-drawn line between two magic note counts, and it retires
  // the last place in the layout where note count stood in for something it is not a measure
  // of.
  //
  // The row pitch is UNIT. The lattice is very nearly square, so this is also about one
  // note-to-note step ALONG a row: measured, the along-row spacing at the outermost row of
  // three vaults is 161, 178 and 165 against a 160 row pitch -- within 12% on vaults spanning
  // 22x the note count, which is what makes a single constant safe here.
  //
  // ONE SEAM FOR EVERY BOUNDARY, group and subfolder alike, and it is the SUB-wedge width --
  // the narrower of the two. There were two widths on the reasoning that a group boundary
  // matters more than a subfolder one and should read as wider; in practice the colours and
  // the legend already carry that distinction, and the wide version reads as a slice missing
  // from the disc rather than as a seam between neighbours.
  //
  // AND THE CHANNEL HAS PARALLEL EDGES. A constant ANGLE is a wedge: it buys arc in
  // proportion to radius, so the channel fanned open toward the rim -- four times the width
  // at the outer row that it had at the inner one, on a band four rows deep. Sizing it in
  // width and dividing by the radius each note actually sits at inverts that: the angle
  // shrinks as the radius grows, every row is separated by the same arc, and the locus of the
  // edge is a straight line offset half a seam from the radius rather than the radius itself.
  // (A point at radius r offset by angle w/2r sits at perpendicular distance r*sin(w/2r), which
  // is w/2 for any r -- so the two edges of a channel are genuinely parallel, not merely
  // closer to it.)
  //
  // Per BAND, because the two rings sit at different radii and one angle cannot be the same
  // width at both.
  //
  // The circularity the note-count proxy existed to dodge is real but avoidable: the radius
  // here is the LOCKED one, fixed once from the full-vault plan, so it is a constant by the
  // time any of this runs rather than something the current plan is still deriving. For the
  // single plan that produces the lock, the old degrees-and-note-count rule stands in.
  // AND IT FALLS OFF AS A BAND GETS DEEPER. A seam that is a fixed width reads as generous on
  // a five-row ring and as a canyon on a twenty-three-row one: the deeper the band, the more
  // wedges it carries and the more of them any one seam competes with, so the same channel
  // asks for more of the eye. Rows are the right measure of that -- they say how big the disc
  // is AND how dense, where a note count says neither on its own.
  //
  // The exponent is fitted, not derived, and the three points it was fitted to are worth
  // keeping: at 5 outer rows (a 454-note vault) half the base seam, at 9 rows (1402 notes) a
  // quarter, and at 23 rows (10,001 notes) none to speak of. (REF/rows)^1.5 gives 0.50, 0.21
  // and 0.05 against those -- 12, 5 and 1 graph units of seam, the last of which is a fifth of
  // a pixel on screen and is the "no gap" the biggest vault wants.
  //
  // This is the third thing to stand where gapScale's note-count ramp used to. It is the same
  // intent -- less seam as a vault grows -- against a quantity that actually describes the
  // geometry rather than standing in for it.
  // Raised from 0.075 once the margins stopped reserving a band-typical dot at every wedge
  // end (see the margin note in placeCell). With the notes' ink landing ON their boundary, the
  // seam IS the visible channel -- at 0.075 it was 12 units, so the two edges of every seam
  // sat on top of each other and read as one doubled line instead of as a gap. At 0.3 the
  // channel measures 94 to 97 units in the outer band with a spread of 2, against the 136 it
  // used to show when two thirds of it was margin nobody could see.
  var SEAM_ROWS = 0.3;      // of a row pitch, at REF_ROWS

  // AND NEVER MORE THAN THIS, whatever the pitch does. The seam is a fraction of the live
  // pitch, which is right -- a sparse ring wants a proportionally sparse seam -- until the
  // pitch itself runs away: a heavily filtered band spreads its spacing by up to DENSITY_MAX,
  // and a seam riding that comes out several times wider than any gap on a full disc. On a
  // mostly-hidden demo vault it ate visible arcs out of the ring, which reads as the circle
  // not being closed rather than as a wide seam.
  //
  // Against UNIT and not against the live pitch, because the point is to bound the runaway
  // rather than to scale with it.
  var SEAM_MAX_ROWS = 0.16;
  var REF_ROWS = 5;
  var SEAM_FALL = 1.5;      // (REF_ROWS / rows) ^ this
  var GAP_FULL_TO = 1000;     // pre-lock fallback only, from here down
  var GAP_ZERO_AT = 10000;
  function gapScale() {
    var n = graph.order;
    if (n <= GAP_FULL_TO) return 1;
    if (n >= GAP_ZERO_AT) return 0;
    return 1 - (n - GAP_FULL_TO) / (GAP_ZERO_AT - GAP_FULL_TO);
  }

  // The angle that buys SEAM_ROWS of row pitch at this band's outer edge, or the legacy
  // rule if the geometry is not locked yet. `frac` lets sub-gaps ride the same width.
  // How much of the base separation a band of this depth gets. The band's OWN depth, because
  // the two rings differ and a shallow inner ring should not be held to the outer one's seam.
  function seamFall(band) {
    var k = band === "i" ? "i" : "o";
    var rows = bandOf(k).rows || REF_ROWS;
    return Math.pow(REF_ROWS / Math.max(1, rows), SEAM_FALL);
  }

  function seamAngle(band, frac) {
    var k = band === "i" ? "i" : "o";
    var r = geomLock && geomLock.bandR ? geomLock.bandR[k] : 0;
    if (!r) return SLICE_GAP * Math.PI / 180 * gapScale() * frac;
    var w = SEAM_ROWS * seamFall(band) * pitchUnits(band) * (GAP_BAND[k] || 1);
    var cap = SEAM_MAX_ROWS * UNIT;
    if (w > cap) w = cap;
    return (w * frac) / r;
  }

  function gapFor(nGroups, band) {
    var g = seamAngle(band, 1);
    return g * nGroups > Math.PI ? Math.PI / Math.max(1, nGroups) : g;
  }

  // THE SEAM AT ONE RADIUS, and the arc left over for wedges there. Called per note, because
  // the whole point is that the answer differs per row -- a constant width costs a different
  // angle at every radius.
  //
  // The clamp is the affordability rule the band allocation already had, applied here because
  // this is now where the total is known: near the hub a fixed width is a large angle, and a
  // deep enough inner ring could otherwise spend the whole circle on seams. Scaling the seam
  // rather than refusing it keeps the arithmetic continuous, so an inner row does not jump
  // when a group arrives.
  var SEAM_CAP = 0.45;      // of the circle, at any one radius
  // THE BOUNDARY, STATED AS WHAT IT ACTUALLY CONTROLS: how much further apart two notes are
  // across a wedge boundary than two notes inside one.
  //
  // It used to be stated as "half a row pitch of margin, plus this note's own radius, at each
  // end" -- and that phrasing is why three rounds of shrinking the SEAM changed nothing a
  // person could see. Measured, the boundary came to 1.43, 1.47 and 1.46 times a normal step
  // on the three vaults: the same ratio everywhere, and almost none of it the seam. The
  // additive radius was most of it, worth ~126 units of extra width at every boundary on the
  // disc, and the seam it dwarfed was 24 falling to 12.
  //
  // Half a pitch per side with nothing added is the ZERO point: two wedges each holding their
  // end note half a step in from their own edge puts those two notes exactly one step apart,
  // which is a boundary you cannot see. Everything above that is the seam a person reads, so
  // that is the number to name and to scale.
  //
  // EXCESS_BASE is what the disc had (0.44 of a step) and the falloff takes it from there, so
  // "half for a five-row band, a quarter at nine, none at twenty-three" is what comes out.
  var MARGIN_ROWS = 0.5;    // the zero point: half a pitch from the wedge edge, per side
  // HOW MUCH OF THE CHANNEL TO KEEP at REF_ROWS. The channel is what a boundary has OVER one
  // ordinary step, and this scales exactly that -- so 0.5 means "half the gap you can see",
  // which is what was asked for, rather than half of some term that turns out to contribute
  // little of it.
  //
  // Two earlier attempts moved the wrong number. Shrinking the SEAM took it from 24 units to
  // 12 and changed the measured channel from 1.60 to 1.60 -- the seam is a tenth of it. Adding
  // an excess ON TOP of the existing margin made it wider still (1.60 -> 1.90), because the
  // margin and the dot term were already the whole excess. Measured channels over an ordinary
  // step, before any of this: 1.60, 1.72, 1.52 on the three vaults -- so the excess to scale is
  // 0.60, 0.72 and 0.52 of a step, and it is emergent rather than a constant anyone set.
  var EXCESS_KEEP = 0.35;   // of the channel, at REF_ROWS

  // A TYPICAL dot's radius, in graph units. The end margin corrects for how this note differs
  // from typical rather than adding its whole radius: adding it made every boundary wider by
  // two radii, where the thing it was for -- both dots' EDGES equidistant from the seam -- only
  // needs the DIFFERENCE between them. A hub steps in, a leaf steps out, and a boundary between
  // two ordinary notes costs nothing at all.
  // PER BAND, because the correction is "how does this note differ from a typical one" and a
  // typical note is not the same size in both rings: the two bands have their own spacing, so
  // their dots have their own scale. Measuring against the outer band's typical dot inflated
  // every inner-band margin by the difference -- measured, the inner ring's channel sat at
  // 1.52 of a step against the outer ring's 1.20, with the zero point already correct at 1.03.
  // A correction whose average is not zero is not a correction, it is a bias.
  var DOT_TYP_I = 0, DOT_TYP_O = 0;
  var dotTyp = function (band) { return band === "i" ? DOT_TYP_I : DOT_TYP_O; };
  function measureDotTyp() {
    var sizes = [];
    graph.forEachNode(function (id, a) { sizes.push(a.size || 4); });
    sizes.sort(function (x, y) { return x - y; });
    var mid = sizes.length ? sizes[Math.floor(sizes.length / 2)] : 4;
    DOT_TYP_I = dotUnits(mid, "i");
    DOT_TYP_O = dotUnits(mid, "o");
  }

  // A DOT'S RADIUS IN GRAPH UNITS. The layout needs a length, and `size` is in pixels -- but
  // the biggest dot is now pinned at DOT_OF_PITCH of the row pitch (see measureSizeScale), so
  // the conversion is a ratio of constants with no camera in it. That is the whole reason the
  // size rule was written that way round: a zoom-dependent radius could not be used here.
  // A dot's radius in graph units. The LIVE pitch, and per band: a dot is drawn against the
  // spacing it actually has, which is the whole point of solving that per ring. `band` is "i"
  // or "o"; anything else takes the outer, which is the one a caller without a band is
  // almost certainly asking about.
  function dotUnits(size, band) {
    var z = size || 4;
    if (z > NODE_MAX) z = NODE_MAX;
    return DOT_OF_PITCH * pitchUnits(band) / bandScale(band) * (z / NODE_MAX);
  }

  // THE BAND IS NOT OPTIONAL. pitchUnits() with no band answers for the OUTER one -- bandOf
  // returns BAND.o for anything that is not "i" -- so every seam on the disc was measured in
  // outer-band rows, including the inner band's. Hide the outer ring's folders and SP_O climbs,
  // and the inner ring's seams grew with it: measured, 48 units to 109, a 2.3x change to a band
  // whose own contents did not move. That is the exact failure pitchUnits' own note says it
  // exists to prevent ("per band and never one shared number"), reintroduced by dropping the
  // argument at the one call site that matters most.
  /**
   * A CELL'S BOUNDARY SWEEP AT ONE RADIUS -- the only place this is worked out.
   *
   * The overlay used to re-derive it from a captured summary, in its own copy of the algebra,
   * and every time the placement changed the two drifted: edges drawn where no note was, a
   * centre line that missed the notes it was supposed to bisect, and a morning of "there is
   * another line". A cell now carries the radius-INDEPENDENT part of each boundary (pLead,
   * pTrail, with the wrap rotation already folded in) and both callers add the same half-seam
   * through the same seamAt. One formula, two readers, nothing to drift.
   */
  function edgeSweep(c, which, rGraph) {
    var sm = seamAt(rGraph, c.nB, c.bandKey);
    return which === "lead" ? c.pLead + sm.gap / 2 : c.pTrail - sm.gap / 2;
  }

  function seamAt(r, nBoundaries, band) {
    var g = r > 1e-6 ? (SEAM_ROWS * pitchUnits(band)) / r : 0;
    var tot = g * nBoundaries;
    var cap = 2 * Math.PI * SEAM_CAP;
    if (tot > cap) { g *= cap / tot; tot = cap; }
    return { gap: g, avail: 2 * Math.PI - tot };
  }

  // ONE allocator, for every caller that divides the circle among cells.
  //
  // There were THREE copies of this arithmetic: share() in buildWedgePlan, the allocation
  // in ringsLayout, and a dead gapPlan/availPlan pair that was computed and never read.
  // Two of them counted groups with an INTEGER, and the one that positions the wedges is
  // where the toggle jump lived for an entire afternoon -- fixing the other one first
  // changed nothing visible and looked like the fix had failed.
  //
  // The differences between the two live callers are real, so they are OPTIONS rather than
  // one copy having quietly forgotten something:
  //
  //   subGaps   buildWedgePlan spends no sub-gaps. Its output is the reference width the
  //             row counts are solved against, not a rendered arc.
  //   clamp     only the rendered allocation can overflow the circle, because only it
  //             spends sub-gaps; it shrinks gap and subGap together rather than eating
  //             into the wedges.
  //   totFloor  the two callers floor the weight total differently (1e-4 vs 1e-6).
  //             Preserved exactly -- unifying it would move every wedge for no reason.
  //
  // PRESENCE IS "IS ANYTHING HERE", NOT "HOW FULL IS IT". A wedge earns one gap by
  // existing; a folder showing 30 of its 100 notes is one wedge and wants one gap, exactly
  // as it did at 100. So the gap total must not be a function of the note count -- and for
  // a long time it was, twice over.
  //
  // Weight over seats was the reading before, and it fails in both directions. Its own
  // note recorded the failure it was chosen to avoid -- weight ALONE keeps a 55-note folder
  // pinned at 1 all the way down to its last note and then collapses in two frames -- but
  // the denominator brought a worse one: seats are whatever the current plan happens to
  // seat, so the same group reads 0.3 mid-animation, 0.48 at rest with a range applied, and
  // 1.0 once the departed notes leave the plan. Measured: nG 7 -> 3.38 across settle() on a
  // date change, and 6.716 -> 7 merely from scrubbing the timeline.
  //
  // Both problems have one answer, and it is not a better formula here. The COLLAPSE case
  // is a group on its way out, and a group on its way out is something the cascade knows
  // about before the first frame -- so it supplies the presence itself, walked from 1 to 0
  // across the whole animation (opts.groupPres). What is left for this function is only
  // "does this group have anything at all", which min(1, weight) answers, ramps smoothly
  // over the first or last note's fade, and does not care how many notes there are.
  //
  // Seats are therefore not consulted at all any more, and the parameter is gone: every
  // reading of them was a way of dividing by a number that had nothing to do with the
  // question.
  //
  // ...EXCEPT WHEN THE PLANNER SUPPLIES IT (opts.groupPres). Weight over seats is the right
  // reading of "how present is this group" only while the seats are the group's own final
  // count. During a cascade they are not: the plan is rebuilt every frame from what is
  // still on screen, so a group that KEEPS 30 of its 100 notes reads 30/100 = 0.3 while
  // the departing 70 are still seated, and 1.0 the moment they leave the plan. Nothing
  // about that group's gap should have moved -- it is still there, still one wedge, still
  // entitled to one gap -- and the whole reservation moved twice instead.
  //
  // A legend toggle never showed it, which is why it survived: hiding a folder takes every
  // note in it to zero together, so presence runs 1 -> 0 cleanly and the survivors stay
  // pinned at 1. Only a filter that thins groups WITHOUT emptying them separates the two
  // readings, and the date range is the first such filter this page has had.
  //
  // So the cascade hands in a presence walked between the two packings by its own progress,
  // exactly as it already does for row counts -- one planner, one clock, and a group's gap
  // is about whether the group is there rather than how full it is.
  function allocateBand(list, weightOf, opts) {
    var TWO = 2 * Math.PI;
    var tot = 0, gw = Object.create(null);
    list.forEach(function (c) {
      tot += weightOf(c);
      var g = gw[c.g] || (gw[c.g] = { w: 0 });
      g.w += weightOf(c);
    });
    var presOf = function (c) { return Math.min(1, weightOf(c)); };
    var given = opts.groupPres || null;
    var groupPres = Object.create(null), nG = 0;
    Object.keys(gw).forEach(function (k) {
      var p = (given && given[k] !== undefined) ? given[k] : gw[k].w;
      groupPres[k] = p < 0 ? 0 : p > 1 ? 1 : p;
      nG += groupPres[k];
    });
    var nSub = 0;
    if (opts.subGaps) {
      var firstOf = Object.create(null);
      list.forEach(function (c) {
        if (!firstOf[c.g]) { firstOf[c.g] = 1; return; }
        nSub += presOf(c);
      });
    }
    var gap = gapFor(nG, opts.band);
    // The SAME seam between subfolders as between folders -- see SEAM_ROWS. It stays a
    // separate name because the two are counted separately (nG against nSub) and the
    // reference allocation spends no sub-seams at all.
    var subGap = opts.subGaps ? gap : 0;
    var gapTotal = gap * nG + subGap * nSub;
    if (opts.clamp && gapTotal > TWO * opts.clamp) {
      var k = (TWO * opts.clamp) / gapTotal;
      gap *= k; subGap *= k; gapTotal *= k;
    }
    var avail = TWO - gapTotal;

    /* ------------------------------------------------------------- minimum arc --
     * A WEDGE MUST BE AT LEAST AS WIDE AS A NOTE.
     *
     * Arc is allotted in proportion to note count, which is what makes every cell equally full
     * and puts one pitch on the whole ring. It has no floor, and a folder of one note in a
     * 10,000-note vault therefore gets about a thousandth of the circle: measured, 14 - Reading
     * List came out at 0.1 degrees and 00 - Inbox at 0.4. A wedge that narrow is narrower than
     * the note it holds -- the note sits at its centre, its distance to its own boundary is
     * ~3.5 units, and dotPx correctly refuses to draw a dot that would cross out of it. Result:
     * radius 3 against 39-55 for the big folders, for a note whose degree is 7 and which is in
     * no way less important than its neighbours. Reported as small folders rendering tiny.
     *
     * So every cell is floored at the angular width of one lattice step, which is exactly the
     * room one note needs, and the surplus is taken back from the cells that are ABOVE the floor
     * in proportion to how far above they are. Cells at the floor are untouched, so the floor
     * cannot be undone by the redistribution that pays for it.
     *
     * Nothing is floored when there is no lock to measure a step against -- the one unfiltered
     * plan that produces geomLock -- and nothing is floored for a cell with no weight, which is
     * entitled to no arc at all (github#5).
     */
    var floorAng = 0;
    if (opts.band && geomLock && geomLock.bandR) {   // lastMinArc is set below, for the probe
      var rRef = geomLock.bandR[opts.band === "i" ? "i" : "o"] || 0;
      // A THIRD OF A STEP, not a whole one, and the three were measured.
      //
      // A whole step is what one note needs, and giving it to every cell costs the rows that
      // cell cannot reach: on the 454-note vault the worst gap went 1.80x the row median to
      // 2.87x, because several folders there hold fewer notes than the band is deep and each
      // then held a full step of arc it could not fill. Measured across the three:
      //
      // Measured on the 10k vault, once a lone note stopped being bounded by its own cell's
      // "step" (see the nRow > 1.5 guard in the placement loop):
      //
      //   floor      worst gap     small-folder dot radius
      //   0.35 step    1.93x                16 - 17
      //   0.8  step    2.73x                37 - 38
      //   1.3  step    3.73x                38
      //
      // 0.8 is the knee. It is where a one-note folder draws at the same size as a crowded one
      // -- 37 against the 34 of a 47-note folder -- and past it the gap keeps growing while the
      // dot does not, because the dot is then bounded by the band's room instead of by its own
      // wedge. The cost is real and is the rows that cell cannot reach: a folder holding fewer
      // notes than the band is deep now holds most of a step of arc in each of them.
      if (rRef > 1e-6) floorAng = 0.8 * pitchUnits(opts.band) / rRef;
    }
    var shareMap = null;
    if (floorAng > 0 && tot > opts.totFloor) {
      shareMap = Object.create(null);
      // THE FLOOR SCALES WITH PRESENCE, so a wedge that is on its way out gives its arc up
      // continuously instead of holding a full note's width until the frame it vanishes.
      //
      // The floor exists because a wedge must be at least as wide as the note it holds. That is
      // a statement about a note that is THERE. Applied flat, it also fires for a cell whose
      // notes are nearly gone: at weight 0.003 a cell was still floored to a full lattice step,
      // and released the whole of it in the single frame present() culled it. Measured on hiding
      // a one-note folder, 06 - Monthly Reviews -- a neighbour, not the folder being toggled --
      // rotated 10.08 degrees in one frame, 153 units at its radius.
      //
      // Scaled by min(1, w) the floor is a full step for any cell holding a real note, since a
      // visible note weighs 1, and shrinks with the last of a departing cell's weight. Nothing
      // changes at rest; the cliff at the cull becomes a ramp.
      var floorFor = function (w, c0) {
        // Driver two: the floor otherwise holds one full note-width until the group's weight
        // drops below one -- i.e. until the LAST note fades -- which is precisely the arc
        // refusing to close. On the ramp it falls (rises) linearly with the same clock.
        if (colWalk && c0 && colWalk[c0.g] !== undefined) return floorAng * colWalk[c0.g].f;
        return floorAng * (w > 1 ? 1 : w < 0 ? 0 : w);
      };
      var over = 0, under = 0, live = [];
      list.forEach(function (c) {
        var w = weightOf(c);
        var raw = w > 0.0001 ? avail * (w / Math.max(opts.totFloor, tot)) : 0;
        shareMap[c.k] = raw;
        if (raw <= 0) return;
        live.push(c);
        var fl = floorFor(w, c);
        if (raw < fl) under += fl - raw;
        else over += raw - fl;
      });
      // AS MUCH OF THE LIFT AS IS AFFORDABLE, ramped -- not all of it or none.
      //
      // This used to bail out to shareMap = null whenever the surplus above the floor could not
      // cover the deficit below it. That is a switch, and every share in the band changes when it
      // throws: affordability depends on the live weights, so it can flip mid-cascade, and when
      // it did, 04 - Daily Notes' wedge start moved 38.55 degrees in a single frame with no cell
      // appearing, no cell leaving, and the gap count and width both unchanged.
      //
      // lift is the fraction of the deficit that can be paid for, so it walks to 1 rather than
      // arriving there. At lift 1 this is the old expression exactly; below it, cells under the
      // floor come up part of the way and the ones above pay proportionally. Continuous across
      // the boundary in both directions, which is the property the switch could not have.
      var lift = under > 0 && over > 0 ? Math.min(1, over / under) : 0;
      if (lift > 0) {
        var take = (under * lift) / over;
        live.forEach(function (c) {
          var raw = shareMap[c.k], fl = floorFor(weightOf(c), c);
          shareMap[c.k] = raw < fl ? raw + (fl - raw) * lift : raw - (raw - fl) * take;
        });
      } else {
        shareMap = null;
      }
    }

    return {
      tot: tot, nG: nG, nSub: nSub, gap: gap, subGap: subGap, avail: avail,
      /** The angular floor actually applied, or 0. For the probe. */
      minArc: (function () { lastMinArc = shareMap ? floorAng : 0; return lastMinArc; })(),
      groupPres: groupPres, presOf: presOf,
      shareOf: function (c) {
        if (shareMap && shareMap[c.k] !== undefined) return shareMap[c.k];
        return avail * (weightOf(c) / Math.max(opts.totFloor, tot));
      },
      // The share as a plain FRACTION of whatever arc is going. The rendered placement needs
      // this rather than shareOf, because the arc going depends on the radius and so cannot be
      // baked in here.
      fracOf: function (c) {
        // The share as a FRACTION of the arc going, so the sweep can work per radius. Derived
        // from the floored share when there is one, or the two disagree and the wedges stop
        // filling the circle.
        if (shareMap && shareMap[c.k] !== undefined) {
          return avail > 1e-9 ? shareMap[c.k] / avail : 0;
        }
        return weightOf(c) / Math.max(opts.totFloor, tot);
      }
    };
  }

  // weightOf lets a note count as a FRACTION of a place. Density -- how many rows
  // a cell needs, how wide its reference wedge is, where the hub sits -- is a pure
  // function of these weights, so feeding it opacity makes the packing re-derive
  // continuously as notes fade instead of switching from one packing to another.
  // Every frame is therefore a valid grid, and at opacity 0 the result is
  // identical to what a plain rebuild over the survivors produces.
  // rowsOf, when given, supplies a cell's row count as a REAL number so an
  // animation can walk the integers between two packings. Without it the count is
  // the plain integer, which is what a resting disc must have: a fractional count
  // blends two grids one row apart, and at rest that reads as a smeared disc
  // rather than a packed one.
  function buildWedgePlan(onlyVisible, weightOf, rowsOf, spIn) {
    var W = weightOf || function () { return 1; };
    var all = order[state.dim] || [];
    var nested = state.dim === "folder";
    var SEP = "\u0000";
    var byCell = {}, cellsOf = {}, planTotal = 0;
    var presMax = Object.create(null);

    // A SUB-WEDGE HAS TO BE ABLE TO HOLD A NOTE PER ROW, or it is dead arc.
    //
    // The gate used to be the group's own size alone, which says nothing about the wedges the
    // split produces. Measured on a 454-note vault: 02 - Areas, 4 notes over 3 subfolders, came
    // out as three cells of one and two notes, each allotted a fair 4 degrees -- and a 4-degree
    // wedge at that radius holds 0.26 notes per row, so rowsNeeded gave each of them 4 rows to
    // fit one note and the other three rows were empty. Four such cells adjacent, with their
    // seams, was a 641-unit hole in one row against a 172-unit step. Same cause as dot radii
    // coming out 8 beside 60: dotFit measures the room a note has, and beside a hole that room
    // is enormous.
    //
    // COUNTED FROM WHAT IS ON SCREEN, not from the vault. The first cut of this read counts[],
    // the whole-vault tally, on the theory that a static answer cannot pop mid-animation. It
    // cannot -- and it also cannot see a filter: under a one-month range 03 - Resources still
    // split on its 138 notes while ten were showing, which is the same dead arc again, and was
    // reported in the outer ring the moment the inner one stopped doing it.
    //
    // Stability comes from WHICH membership instead of from ignoring it. willShow is the
    // DESTINATION -- what the disc is settling into -- so it does not move while a cascade
    // runs, and the live per-frame plan agrees with the destination packing throughout. The
    // answer changes exactly once, when the filter does, before any frame is drawn; planA is
    // built on the old membership and keeps the old split, planB the new, and notes interpolate
    // between the two packings the way they already do for rows and spacing. Counting alpha
    // instead would let the threshold fall mid-flight, which is a change of cell IDENTITY
    // rather than of weight, and that is the one thing the cascade cannot walk.
    var liveG = Object.create(null);
    var liveN = Object.create(null);
    // COUNTED FROM WHAT IS ON SCREEN, same reasoning as liveN just below and the same lesson
    // learned the hard way for the FOLDER-level gate (comment above): subCount is a whole-vault
    // tally taken once at load, so under a filter a subfolder can clear subCellIndex's threshold
    // on notes that are not even showing. liveSub is the live, filter-aware answer subCellIndex
    // actually reads (github#31).
    var liveSub = Object.create(null);
    graph.forEachNode(function (id) {
      // MEMBERSHIP AT REST IS THE MEMBERSHIP THE CASCADE ENDS ON, and it was not.
      //
      // planKeep, during a cascade, is "staying or still on screen". Absent -- at rest -- this
      // fell back to visible(), the FOLDER filter alone, so a note excluded by the DATE range
      // stayed a member at weight 0. That looks harmless and it is the jump at the end of an
      // animation: measured on a date range, the resting plan carried 11 cells where the final
      // frame carried 9 -- the extra two being empty sub-cells of one folder -- and the same arc
      // divided 11 ways instead of 9 moved wedges by up to 10.6 degrees. Radii and dot sizes
      // were identical to the unit, which is why it read as a sideways twitch with no cause.
      //
      // willShow IS that destination. At rest nothing is departing, so "staying or on screen"
      // and "showing" are the same set, and the two agree by construction rather than nearly.
      if (onlyVisible && !(planKeep || willShow)(id)) return;
      // A PINNED NOTE HAS LEFT THE RING, same as the placement loop below excludes it (see
      // that loop's own comment). Counting it here anyway inflates liveG/liveN/liveSub --
      // the inputs bandDepth/splitFor/subCellIndex gate on -- with notes that never occupy a
      // ring cell, letting a folder pass the split threshold (or a band read deeper than it
      // is) on notes sitting in the hub. The same "gate reads a count the packing can't see"
      // defect already fixed for date-range filters (github#18/#19/#31), just not extended
      // to pinning until now.
      if (isPinned(id)) return;
      var g0 = groupOf(id);
      // A WEIGHT, NOT A COUNT -- and the second filter that used to stand here, rejecting
      // anything outside willShow, is gone with it.
      //
      // liveG feeds bandDepth, and bandDepth is what the split gate below compares against. As a
      // count over willShow it took the DESTINATION's value from the first frame of a cascade,
      // while at rest it had the source's -- so the number the gate tested changed in one step
      // between the resting disc and frame 1. Gates flipped for folders that were not being
      // toggled at all, and the cell structure teleported before a single note had moved.
      // Measured on hiding 04 - Daily Notes: 50 notes jumped 2482 units TANGENTIALLY on frame 2,
      // in 02 - Areas as well as 04, with rows, gaps, room and arc all identical across the
      // frame. Structure changing under notes that were still where the source put them.
      //
      // Summed as weights it is the same number at rest -- a visible note weighs 1 -- and walks
      // continuously during a cascade, because that is what the weights do. The gate can still
      // flip, but it flips once, when its input genuinely crosses the threshold, and it moves the
      // one folder whose split actually changed. Same treatment the row count and the band room
      // already get: interpolate the INPUT and let the planner stay the only planner.
      var wv = W(id);
      liveG[g0] = (liveG[g0] || 0) + (wv > 1 ? 1 : wv < 0 ? 0 : wv);
      // AND A PLAIN COUNT over the same set, for the SPLIT GATE alone.
      //
      // Two different questions are being asked of this loop. "How deep is this band" is about
      // how much is on screen, and wants the weights -- that is liveG above, and making it a
      // weight is what stopped the band depth jumping between rest and frame 1. "Does this
      // folder have enough notes to be worth splitting into subfolder columns" is not about
      // the fade at all: it is a property of the folder, and the answer must not change while
      // the folder is on its way out, or its columns merge underneath the notes still in them.
      //
      // Measured on hiding 04 - Daily Notes: the gate flipped at 45% of the cascade, while the
      // folder was still half visible, and its wedge start moved 38.9 degrees in one frame.
      //
      // The set this loop walks is the UNION of what is staying and what is still on screen, so
      // a count over it is the folder at its fullest -- the source count while it leaves, the
      // destination count while it arrives -- and is constant for the whole cascade. At rest the
      // union is just what is visible, so the count is unchanged there.
      liveN[g0] = (liveN[g0] || 0) + 1;
      var sk = g0 + "/" + (graph.getNodeAttributes(id).sub || "");
      liveSub[sk] = (liveSub[sk] || 0) + 1;
    });
    // AGAINST THE BAND'S REAL DEPTH, not a constant.
    //
    // "Each sub-wedge can fill a column" is a claim about ROWS, and the gate was testing it
    // against REF_ROWS, which is 5. The demo vault's outer band is 9 rows deep, so a folder of
    // 25 notes over 4 subfolders passed the test (25 >= 4*5) and should not have (25 < 4*9):
    // each sub-wedge got 6 or 7 notes for a 9-row band, put a note in some rows and none in
    // others, and its arc stood EMPTY in the rows it could not reach. Measured on 15 - Courses:
    // four sub-wedges of 7, 6, 6 and 6, none of them reaching the rim row, and their four arcs
    // side by side were a 773-unit gap in a row whose ordinary spacing is 175.
    //
    // No placement rule removes that. Spanning the notes across the full depth puts the dead
    // arc in the middle rows, centring them puts it at both edges, capping the row count moves
    // it outward. Seven notes in a nine-row band leave two rows empty wherever they go, so the
    // split is what has to be refused.
    //
    // NOT CIRCULAR, which is the reason it can be done here at all. The depth of a band follows
    // from its AREA and the number of notes IN it -- the same single division solveBand does --
    // and neither term depends on how any group is split. So the gate can know the depth before
    // the cells exist. Without a geometry lock, on the one unfiltered plan that produces it,
    // REF_ROWS stands in.
    var bandLive = { i: 0, o: 0 };
    Object.keys(liveG).forEach(function (g) {
      bandLive[bandLock && bandLock[g] ? "i" : "o"] += liveG[g];
    });
    var depthOfBand = function (isInner) {
      if (!geomLock) return REF_ROWS;
      var n = bandLive[isInner ? "i" : "o"];
      var thick = isInner ? (geomLock.rOuter - geomLock.r0) * INNER_FILL
                          : geomLock.maxR - geomLock.rOuter;
      var scale = isInner ? INNER_SCALE : 1;
      var base = isInner ? geomLock.r0 : geomLock.rOuter;
      if (!(thick > 0) || !(n > 0.5)) return REF_ROWS;
      var T = thick * scale, R = (base + thick / 2) * scale;
      var rw = Math.round(T / Math.sqrt(2 * Math.PI * R * T / n));
      return rw < 1 ? 1 : rw > 200 ? 200 : rw;
    };
    var bandDepth = { i: 0, o: 0 };
    var splitOf = Object.create(null);
    var splitFor = function (g) {
      if (splitOf[g] === undefined) {
        var bk = bandLock && bandLock[g] ? "i" : "o";
        if (!bandDepth[bk]) bandDepth[bk] = depthOfBand(bk === "i");
        var nSubs = (subOrder[g] || []).length;
        // CAPPED AT SUB_SLOTS for the threshold, not the raw subfolder count. However
        // many real subfolders a folder has, subTintIndex never hands out more than
        // SUB_SLOTS distinct cells -- the smallest ones pool into the "N smaller
        // subfolders" tail and share the last one. Gating on the raw count asks a
        // folder to prove it can fill rows for pieces that were never going to be
        // drawn separately, which made 03 - Resources (9 subfolders, 60 notes) and
        // 05 - Meeting Notes (14 subfolders, 85 notes) fail a bar meant for 9 and 14
        // pieces when both only ever draw 4.
        var splitPieces = Math.min(nSubs, SUB_SLOTS);
        splitOf[g] = nested && nSubs > 1 &&
                     (liveN[g] || 0) >= Math.max(NEST_MIN, splitPieces * bandDepth[bk]);
      }
      return splitOf[g];
    };

    graph.forEachNode(function (id) {
      // While a cascade runs, "who gets a slot" is the UNION of what is staying
      // and what is still on screen on its way out. Filtering to visible() alone
      // gave a departing note no slot at all, so nothing held its space (the
      // wedge vanished instead of closing) and it had no target position (so it
      // faded at stale coordinates on top of the reflowed disc).
      // MEMBERSHIP AT REST IS THE MEMBERSHIP THE CASCADE ENDS ON, and it was not.
      //
      // planKeep, during a cascade, is "staying or still on screen". Absent -- at rest -- this
      // fell back to visible(), the FOLDER filter alone, so a note excluded by the DATE range
      // stayed a member at weight 0. That looks harmless and it is the jump at the end of an
      // animation: measured on a date range, the resting plan carried 11 cells where the final
      // frame carried 9 -- the extra two being empty sub-cells of one folder -- and the same arc
      // divided 11 ways instead of 9 moved wedges by up to 10.6 degrees. Radii and dot sizes
      // were identical to the unit, which is why it read as a sideways twitch with no cause.
      //
      // willShow IS that destination. At rest nothing is departing, so "staying or on screen"
      // and "showing" are the same set, and the two agree by construction rather than nearly.
      if (onlyVisible && !(planKeep || willShow)(id)) return;
      // A PINNED NOTE HAS LEFT THE RING. Without this it keeps its seat and the wedge is
      // drawn around a hole where it used to be -- the note is in the hub and its chair is
      // still at the table. Excluding it from the plan is what makes the ring re-densify,
      // and it is the same mechanism a hidden note uses, minus the fade: the note is still
      // fully present, it is just no longer part of this packing.
      if (isPinned(id)) return;
      var g = groupOf(id), a = graph.getNodeAttributes(id);
      var split = splitFor(g);
      // splitFor(g) just above always populates bandDepth[bk] for this g's band before
      // returning (see its own body), so the lookup here is never stale or missing.
      var bk = bandLock && bandLock[g] ? "i" : "o";
      var key = split
        ? g + SEP + subCellIndex(g, a.sub, liveSub[g + "/" + (a.sub || "")] || 0, bandDepth[bk])
        : g;
      if (!byCell[key]) {
        byCell[key] = [];
        (cellsOf[g] || (cellsOf[g] = [])).push(key);
      }
      byCell[key].push(id);
      planTotal += W(id);
      // HOW PRESENT THIS GROUP IS, as the LARGEST weight among its notes.
      //
      // This is what a gap between wedges is reserved against, and it has to answer "does this
      // group exist at all" rather than "how much of it is left". The max does both jobs the
      // sum cannot: it stays at 1 while a group merely THINS, because one full-weight note is
      // enough, and for a group that is LEAVING it reaches 0 at the moment its last note does
      // -- which is the same moment the cell is culled by present(). So the reservation and the
      // cell wink out together instead of one outliving the other.
      var pw = W(id);
      // Driver three: the seam otherwise stands at the group's max alpha -- full width until
      // the last note fades, then a snap. On the ramp it closes with the wedge.
      if (colWalk && colWalk[g] !== undefined) pw = colWalk[g].f;
      if (!(presMax[g] >= pw)) presMax[g] = pw;
    });

    // Groups too small for a readable wedge share one. Measured here, the
    // smallest few would each need ~11 degrees to read apart -- 16% of the circle
    // for 3.4% of the notes -- so at their proportional 2-4 degrees they merged
    // into a grey mass that looked like a cluster nobody asked for.
    ringsMerged = Object.create(null);
    var big = [], smallIds = [];
    all.filter(function (g) { return cellsOf[g]; }).forEach(function (g) {
      if ((counts[g] || 0) >= SMALL_GROUP) { big.push(g); return; }
      ringsMerged[g] = true;
      cellsOf[g].forEach(function (k) { smallIds = smallIds.concat(byCell[k]); });
    });

    var cells = [];
    big.forEach(function (g) {
      var ks = cellsOf[g];
      if (nested) {
        ks.sort(function (x, y) {
          return (+(x.split(SEP)[1] || 0)) - (+(y.split(SEP)[1] || 0));
        });
      }
      // k is the cell's identity -- group plus tint slot. It is derived from
      // global data (subOrder, counts), never from which notes happen to be
      // included, so the SAME cell key means the same cell in any packing.
      // That is what lets two packings be blended cell by cell.
      ks.forEach(function (k) { cells.push({ g: g, k: k, list: byCell[k] }); });
    });
    if (smallIds.length) cells.push({ g: MERGED, k: MERGED, list: smallIds });
    if (!cells.length) return null;

    cells.forEach(function (c) {
      // Best-connected first, so hubs sit near the centre of the disc.
      //
      // Grouping a POOLED cell by subfolder first was tried, to make a highlighted
      // tail folder move as a contiguous block instead of a scatter. It is not worth
      // it: total cross-collisions only fell 9 -> 7 across the tail folders and one
      // case got WORSE (04 Weekly Summaries 0 -> 3), while it perturbs the resting
      // position of every pooled cell and costs the hubs-near-the-centre property.
      c.list.sort(function (a, b) { return hubRank[a] - hubRank[b]; });
      c.wsum = 0;
      c.list.forEach(function (id) { c.wsum += W(id); });
    });

    // Reference angles: a cell's share of the vault, fixed, so the row plan below
    // never shifts when something is hidden.
    var TOTAL = planTotal;
    var MIN = MIN_SPAN, TWO = 2 * Math.PI;
    // The disc is split into TWO bands rather than floored.
    // A cell too small for a fair wedge (its proportional angle would fall under
    // MIN_SPAN) moves to an inner ring, where it is proportional among its peers
    // over the FULL circle. Measured on this vault that turns eight 6-degree
    // slivers into wedges of 14-97 degrees, and keeps the main ring strictly
    // proportional -- flooring them in one band spent 48 degrees (13% of the
    // circle) on 26 notes (6% of the vault).
    // Decide the band per GROUP, not per cell. Deciding per cell let one of a
    // group's sub-wedges sit inner while a sibling sat outer -- 07 - Weekly
    // Reviews rendered half in the middle and half in the main ring once a
    // re-pack moved the threshold past one of its quarters. A group's sub-wedges
    // are nested inside its arc, so they belong in the same band by definition.
    var smallAt = TOTAL * (MIN / TWO);
    var groupInner = {};
    cells.forEach(function (c) {
      var small = c.wsum < smallAt;
      if (groupInner[c.g] === undefined) groupInner[c.g] = small;
      else groupInner[c.g] = groupInner[c.g] || small;   // any small cell -> inner
    });

    // THE TWO BANDS ARE INDEPENDENT. Which ring a group lives in is decided once,
    // from the data as loaded, and then never changes -- filtering must never
    // migrate a group from one ring to the other. Only a fresh load of the data
    // may reassign it.
    //
    // Without this lock the split follows whatever is on screen, and the inner
    // band is proportional over the FULL circle: a group crossing the threshold
    // part-way through a fade leaps to the inner ring and spreads over ~300
    // degrees. Measured, that put 161 surviving notes inside the departing
    // wedge's arc; it also made wedges appear in the inner ring first and then
    // jump outward.
    if (bandLock) cells.forEach(function (c) {
      if (bandLock[c.g] !== undefined) groupInner[c.g] = bandLock[c.g];
    });
    cells.forEach(function (c) { c.inner = groupInner[c.g]; });
    var inner = cells.filter(function (c) { return c.inner; });
    var outer = cells.filter(function (c) { return !c.inner; });
    // Only when there is no outer band IN THE DATA -- i.e. before bandLock exists, at
    // load, on a vault small enough that every group is a sliver. Guarding on
    // bandLock matters because this line overrides the lock applied just above, and
    // `!outer.length` is also true whenever FILTERING has hidden every outer group.
    // In that case it moved all the inner cells to the outer band's base radius, so
    // the inner ring teleported outward on the single frame the last outer note
    // stopped being present: measured, 01 - Projects went from r=479 to r=1175 in one
    // frame, a 696-unit jump, at frame 206 of 210 with the outer band already still.
    // An empty outer band is a legitimate state -- the disc is just the inner ring,
    // small, which is what "fewer notes make a smaller disc" means here.
    if (!outer.length && !bandLock) {    // tiny vault: one band, no split
      inner.forEach(function (c) { c.inner = false; });
      outer = cells; inner = [];
    }

    // Reference width only -- no sub-gaps, no clamp. See allocateBand for why those are
    // options rather than an oversight.
    var share = function (list, band) {
      var a = allocateBand(list,
                           function (c) { return c.wsum; },
                           { subGaps: false, clamp: null, totFloor: 0.0001, band: band });
      // Recorded so the probe can show the gap total shrinking frame by frame -- the whole
      // point of making it continuous is that this series has no cliff in it.
      lastGapN[band] = Math.round(a.nG * 1000) / 1000;
      list.forEach(function (c) { c.band = a.shareOf(c); });
    };
    share(inner, "i");
    share(outer, "o");

    // The row COUNT has to follow the live span, or density collapses: isolate a
    // 55-note folder with the reference locked and it keeps 7 rows sized for its
    // ~47-degree share of the whole vault while now spanning 360, so it draws as a
    // scattered cloud across 7 sparse rings instead of a disc.
    //
    // Taking it live is safe in a way that is worth spelling out. cum[] below is
    // built from cap[row] / capSum, and a constant factor on every capacity
    // cancels in that ratio -- so WHICH notes land in WHICH row depends only on
    // the row count, never on the span. The span therefore has exactly one effect,
    // the count, and the count is walked by animation progress rather than read
    // off the live weights, so it changes smoothly.
    cells.forEach(function (c) { c.bandRef = c.band; });

    // Uniform density fills an annulus, so n notes need pi*(R^2 - r0^2) of area.
    // Fixing the hole at a FRACTION of the outer radius and solving for r0 keeps
    // the hub the same relative size whether 440 notes are showing or 55 -- a
    // fixed r0 gave a 32% hole at full size and a 69% one when filtered down.
    // THE DENSITY KNOB (github#13). SP is the lattice spacing -- radial between rows
    // AND tangential along them, which is what makes the packing uniform -- and it was
    // a hard 1. That made the disc a function of how many notes the VAULT holds rather
    // than how many are on screen: measured, screen row pitch was 19.481px at every
    // filter state of a 500-note vault and 12.064px at every state of a 1500-note one,
    // so filtering 503 notes down to 62 moved the median dot from 4.254px to 4.208px
    // and left 25% of the disc's radius as empty margin.
    //
    // Solve it instead. A lattice of spacing s holds 1/s^2 notes per unit area, so an
    // annulus with a hole at HOLE*R holds n = pi*R^2*(1-HOLE^2)/s^2, i.e.
    // R = s*sqrt(n / (pi*(1-HOLE^2))). Setting that equal to the FULL-vault R gives
    //
    //     s = sqrt(n_full / n_visible)
    //
    // exactly -- not an approximation, the same uniform-density assumption the r0
    // formula below already makes. The disc then always fills its box, notes move
    // outward as their neighbours leave, and each one gets proportionally more room.
    //
    // CONTINUOUS, WHICH IS THE WHOLE POINT. The retired REPACK_BELOW mechanism below
    // switched plan basis at 55% and its three failures were all failures of the
    // THRESHOLD: inconsistent either side, two packings inside one animation, and call
    // sites disagreeing about which side they were on. A smooth function of the visible
    // weight has no side to be on. It also holds the two invariants for free -- a
    // departing note sits at weight 0 and contributes nothing to planTotal, so
    // zero-weight invariance survives; and both the lean and padded plans derive it
    // from their own planTotal, so parity survives.
    //
    // Capped, because sqrt grows without bound: one surviving note would otherwise be
    // one boulder on the rim. At DENSITY_MAX the disc stops filling the box and starts
    // shrinking again, which is the honest end of the behaviour rather than a cliff.
    var HOLE = 0.3;
    // SUPPLIED, DURING A CASCADE, for the same reason rowsOf is. Deriving it here from
    // planTotal is right at rest and wrong mid-animation: the cascade weights membership
    // by alpha, so planTotal slides every frame and the lattice breathed with it --
    // measured, biggest single-frame radial step went from 0 to 94 units against a row of
    // 160, i.e. the whole disc shifting half a row per frame. The cascade hands in the
    // interpolation between its two endpoint packings instead, exactly as it does for
    // rows, so the last frame and rest agree by construction.
    var fullTotal = geomLock && geomLock.total > 0 ? geomLock.total : planTotal;
    var density = (spIn && typeof spIn === "object") ? (spIn.o || 1)
      : spIn > 0 ? spIn
      : (planTotal > 0.0001
          ? Math.min(DENSITY_MAX, Math.sqrt(fullTotal / planTotal)) : 1);
    var SP = density;

    // ...AND THEN ONE PER BAND, because the two rings are packed independently and a single
    // spacing makes each answer for the other's filtering.
    //
    // Hiding OUTER folders raises the disc-wide density -- fewer notes on screen, same box --
    // so the inner ring spreads outward even though its own occupancy never changed, while the
    // outer ring loses rows faster than the spreading puts back. The two close on each other.
    // Measured on the 1402-note vault, hiding outer groups one at a time: the inner ring's
    // outer edge went 1528 -> 1767 -> 1954 -> 2552 while the outer ring's fell 3761 -> 3762 ->
    // 3375 -> 3030, and the clear space between them collapsed 843 -> 89 units.
    //
    // A band's spacing is now solved from ITS OWN occupancy, so hiding a folder moves the ring
    // that folder is in and leaves the other one alone.
    //
    // Only once the split is LOCKED. Before that -- the one plan that produces bandLock -- the
    // split is what the balancer is still searching for, so per-band totals would depend on the
    // candidate being scored. That plan is the unfiltered one, where the density is 1 and the
    // two are the same number anyway.
    // HANDED IN DURING A CASCADE, never re-derived per frame. github#13 already established
    // this for the single spacing -- "hand the cascade its spacing instead of letting it
    // re-derive one per frame" -- and making the spacing per band quietly broke it: spIn fed
    // only `density`, so the two bands went back to solving themselves from the live weights on
    // every frame.
    //
    // That is visible and it is ugly. The row counts are integers and the rim solve below lands
    // on them, so a spacing re-derived per frame is a step function of the frame: the disc
    // jiggles, the inner ring stops animating at all because its own occupancy has not changed
    // enough to move its solve, and then it jumps when the solve finally ticks. Reported as
    // exactly that, disabling 04 and then 03.
    //
    // Walked between the two packings' own values, it is smooth by construction -- and the rim
    // pin comes along for free, because each endpoint solved its own pin at rest.
    var given = (spIn && typeof spIn === "object") ? spIn : null;
    // The band room, when the caller is walking it between two packings. Same channel as the
    // spacing, because it is the same kind of quantity and must not be re-derived per frame.
    var givenRoom = given && given.room ? given.room : null;
    var SP_I = given && given.i > 0 ? given.i : SP;
    var SP_O = given && given.o > 0 ? given.o : SP;
    var bandDensity = function (cells, key) {
      if (!geomLock || !geomLock.bandTotal) return SP;
      var full = geomLock.bandTotal[key] || 0, now = 0;
      cells.forEach(function (c) { now += c.wsum; });
      if (!(full > 0.0001) || !(now > 0.0001)) return SP;
      return Math.min(DENSITY_MAX, Math.sqrt(full / now));
    };
    var r0 = geomLock ? geomLock.r0 : Math.max(1.5, HOLE * Math.sqrt(
      Math.max(1, TOTAL) / (Math.PI * (1 - HOLE * HOLE))));

    // Rows needed to hold n notes, accumulating each row's capacity. The capacity
    // is proportional and unfloored, for the same reason as in placeCell: flooring
    // it clamped a narrow cell's rows to one note each, so a 19-note cell asked
    // for 8 rows and drew as a thin sparse spoke instead of a packed wedge.
    //
    // A CELL WITH NO WEIGHT NEEDS NO ROWS, and the floor below is why that has to be said
    // out loud. A departing cell stays seated in the plan at weight 0 while it fades, so
    // `Math.max(1, k)` handed it one row anyway -- and that row reaches the band
    // balancer's split search, which is how 738 zero-weight members moved the hub radius
    // and put the padded plan's maxR one row outside the lean plan's (github#5). The
    // cascade's row recorder already worked around the same floor by hand; the geometry
    // never did.
    // `sp` defaults to the disc-wide spacing, which is what the balancer wants: it is scoring
    // candidate splits and there is no per-band answer until one is chosen.
    function rowsNeeded(span, n, st, sp) {
      if (!(n > 0)) return 0;
      var p = sp > 0 ? sp : SP;
      var i = 0, r = st, k = 0;
      while (i < n && k < 500) { i += Math.max(0.05, span * r / p); r += p; k++; }
      // NEVER MORE ROWS THAN NOTES. The loop answers "how many rows of THIS arc does it take
      // to hold n notes at the lattice pitch", and for a narrow arc that answer exceeds n --
      // measured, a 4-degree wedge holds 0.26 notes per row, so one note was told it needed
      // four rows. One note fits in one row. It does not FILL one, which is a different thing
      // and not one that more rows repair: the extra rows are empty, they are dead arc in
      // every row the cell fails to reach, and because the band's depth is the deepest cell in
      // it, a single note in a small folder was setting a whole band four rows deep.
      //
      // That depth is what made the dots uneven. Rows are filled in proportion to their arc,
      // so the angular STEP should be one pitch everywhere -- but a row holds an integer
      // number of notes, and the shortest arc takes the worst rounding. A row with capacity
      // 2.5 given 3 notes is 20% crowded where a row with capacity 20 given 21 is 5%, dotFit
      // shrinks by exactly that, and the innermost row -- always the shortest -- always loses.
      // Reported as inner rows carrying visibly smaller dots than the rows outside them.
      // Fewer, fuller rows is the fix: the same notes over less depth round better.
      var cap = Math.ceil(n - 1e-9);
      return Math.max(1, cap > 0 && k > cap ? cap : k);
    }

    // The edge inset, per cell, from the cell's own band base. Held here so the row
    // count and the placement agree: capacity above is "one note per row-pitch of
    // arc", so if placement only uses (1 - 2*pad) of the arc and the count does not
    // know, the innermost row is handed more notes than it has room for. That was
    // measured as 2 crowded pairs in the month cells at 2.5-3.1px.
    function padFor(base, ref) {
      var refArc = base * (ref || 0) * UNIT;
      return refArc > 1e-6 ? Math.min(EDGE_PAD_MAX, EDGE_PAD_ARC / refArc) : 0;
    }
    // The span the notes actually occupy, which is what density must be solved for.
    function usableRef(c, base) {
      c.pad = padFor(base, c.bandRef);
      return c.bandRef * (1 - 2 * c.pad);
    }

    // Hoisted above the balancer, which needs it to place the outer base.
    // In LATTICE units, so it scales with SP: the gap between the rings has to stay the
    // same number of rows wide as the rows spread apart, or a filtered disc shows two
    // bands nearly touching where the full one showed a clear channel.
    var GUTTER = 1.6 * SP;

    // The inner ring is deliberately THINNER than the outer, not equal to it. Equal
    // thickness was tried first and reads oddly: the inner band sits at a smaller radius,
    // so the same thickness there is a much larger share of its own annulus and the middle
    // of the disc looks heavy. 0.75 was the next attempt and still read as two competing
    // rings; a little over half lets the outer ring carry the disc outright and the inner
    // read as subordinate to it.
    //
    // A TARGET, not a guarantee. Rows are integers, so the reachable ratios are a coarse
    // grid -- on a 450-note vault a single row is a third of a band -- and the two hard
    // rules (no small folder in the outer ring; see PIN_BELOW) come first. The balancer
    // gets as close as the constraints allow and the suite reports what it achieved.
    var BAND_RATIO = 0.55;   // target inner thickness as a fraction of the outer

    // BALANCE THE TWO BANDS' THICKNESS by moving whole groups between them.
    //
    // Size alone decides badly. Every group too small to earn its minimum wedge goes
    // inner, and on a large vault that is most of them -- measured on a 10,000-note
    // fixture, inner came out 35 rows deep against the outer band's 9. Two rings of
    // wildly different thickness read as a mistake rather than as two bands.
    //
    // THIS HAS TO RUN HERE, not where the split is decided. A first version sat next to
    // the size threshold ~150 lines up and was a silent no-op: `r0` is declared below it,
    // so `var` hoisting made it `undefined`, rowsNeeded returned 1 for every group, no
    // move ever looked like an improvement, and the measured thickness went 35/9 -> 34/9.
    // Everything the measurement needs -- r0, rowsNeeded, usableRef, GUTTER, and share --
    // only exists by this point.
    //
    // A trial has to re-run share(), because a cell's reference width is its share WITHIN
    // ITS BAND: moving one group changes the row count of every cell in both bands. That
    // is also why there is no closed form and why this is greedy descent -- repeatedly
    // make the single move that most reduces the difference, stop when none does. Groups
    // number in the tens and rowsNeeded is arithmetic, so the whole search is microseconds.
    //
    // Guarded on bandLock so it runs ONCE, on the full data, before the lock is taken --
    // the same guarantee the lock exists for. Re-balancing under a filter would migrate a
    // group between rings mid-cascade, which is the bug the lock was added to stop.
    if (!bandLock) (function balanceBands() {
      var names = [];
      cells.forEach(function (c) { if (names.indexOf(c.g) < 0) names.push(c.g); });
      if (names.length < 2) return;
      var assign = {};
      cells.forEach(function (c) { assign[c.g] = !!c.inner; });

      // SMALL FOLDERS ARE PINNED INNER. They are only in the inner ring in the first
      // place because they are too small to earn their minimum wedge, and the outer ring
      // is where that hurts most: at the larger radius a three-note folder is three lonely
      // dots holding open a 6-degree slice, and the minimum wedge distorts the
      // proportionality of every wedge beside it. Balancing must not buy an even pair of
      // rings by exiling them there.
      //
      // So the search only permutes groups the size rule put OUTER -- it may pull a big
      // group inward, and push it back out, and nothing else. That also shrinks the search
      // space enough that 16 groups still enumerate exhaustively: on the 10,000-note
      // fixture seven of the sixteen are pinned, leaving 2^9.
      // WHAT COUNTS AS SMALL IS ABSOLUTE, not a share of the vault. `smallAt` is
      // TOTAL * (MIN_SPAN / 2pi) because it answers a different question -- "is this group
      // too small to earn its minimum wedge" -- and that scales with the vault. Reusing it
      // as the pin meant a 148-note folder was "small" in a 10,000-note vault and got
      // nailed into the hub, which put 32 cells and ~550 notes in the inner ring and made
      // it 19 rows deep against the outer band's 7. No radius or assignment recovers from
      // that, and the ring came out thicker than the one outside it.
      //
      // A folder is small when it is a HANDFUL OF NOTES, at any vault size: below that it
      // is a few lonely dots holding open a minimum wedge at the rim, which is the thing
      // worth preventing. Above it, a folder can hold its own in the outer ring whatever
      // fraction of the vault it happens to be.
      //
      // On a 450-note vault this changes nothing -- smallAt is 7.5 there, so the two
      // thresholds already agree.
      var PIN_BELOW = 10;                      // notes; fewer than this never leaves the hub
      var groupNotes = {};
      var totalNotes = 0;
      cells.forEach(function (c) {
        groupNotes[c.g] = (groupNotes[c.g] || 0) + c.list.length;
        totalNotes += c.list.length;
      });
      var pinnedInner = {};
      names.forEach(function (g) {
        if (assign[g] && (groupNotes[g] || 0) < PIN_BELOW) pinnedInner[g] = true;
      });
      var movable = names.filter(function (g) { return !pinnedInner[g]; });
      if (!movable.length) return;             // nothing may move; the size rule stands

      // JOINT over the split AND the hub radius. Evaluating a split at the BASE radius and
      // only then solving r0 is two-stage and measurably wrong: on the 450-note vault it
      // chose 2/4 rows for a 0.27 ratio when 3/3 reaches 0.80, which is closer to the
      // target -- the split had been scored against a radius the layout then moved away
      // from. share() is the expensive part and depends only on the split, so it runs once
      // per split and the radius scan reuses it.
      var spanFor = function (ins, outs, rv) {
        var iR = 0;
        ins.forEach(function (c) {
          var r = rowsNeeded(usableRef(c, rv), c.wsum, rv);
          if (r > iR) iR = r;
        });
        var rOut = ins.length ? rv + iR * SP + GUTTER : rv;
        var oR = 0;
        outs.forEach(function (c) {
          var r = rowsNeeded(usableRef(c, rOut), c.wsum, rOut);
          if (r > oR) oR = r;
        });
        // Compared as the VISIBLE BAND: n rows of notes span (n - 1) gaps, not n, so a
        // 3-row band is two pitches thick and not three. Optimising rows x pitch instead
        // put the real vault at 0.32 while the balancer believed it had 0.40 -- the
        // off-by-one is a fifth of the answer on a band this shallow. Scaled by
        // INNER_SCALE, because equal row counts are not equal drawn thickness.
        return {
          inner: Math.max(0, iR - 1) * SP * INNER_SCALE,
          outer: Math.max(0, oR - 1) * SP,
          iR: iR, oR: oR,
          // The hole as a share of the whole disc. Needed here because growing the hub to
          // hit a thickness ratio trades against the one thing HOLE exists to hold fixed.
          holeShare: (rOut + oR * SP) > 0 ? rv / (rOut + oR * SP) : 1
        };
      };

      // Best (cost, radius) for one split, scanning the hub outward. Bounded at 3x: past
      // that the hole stops being a hole and becomes the composition.
      var R0_BASE = r0;
      // THE HOLE MAY NOT RUN AWAY. HOLE is 0.3 precisely so the hub stays the same
      // FRACTION of the disc at any size -- its own note records that a fixed r0 once gave
      // "a 32% hole at full size and a 69% one when filtered down". Letting the balancer
      // grow the hub freely reintroduced that: measured, 47% of the disc on a 450-note
      // vault and 58% at 10,000, which is also why the centre logo visibly grew, since it
      // is drawn at half the hole. A little headroom above 0.3 buys most of the balancing;
      // beyond that the hole is the composition and the rings are trim.
      var HOLE_MAX = 0.36;
      var evaluate = function (a) {
        var ins = [], outs = [];
        cells.forEach(function (c) { (a[c.g] ? ins : outs).push(c); });
        // An empty band is the degenerate case the guard further up exists for, not a
        // balanced one. Price it out of the search.
        if (!ins.length || !outs.length) return { cost: Infinity, r0: R0_BASE };
        share(ins, "i"); share(outs, "o");
        cells.forEach(function (c) { c.bandRef = c.band; });

        // HOW FAR THIS SPLIT IS FROM BEING SIZE-ORDERED: the biggest folder it puts inside
        // minus the smallest it puts outside, as a share of the vault. Zero when every
        // outer folder is at least as big as every inner one -- which is exactly "the
        // largest folders are on the rim". Depends only on the split, so it is computed
        // once rather than per radius.
        //
        // TWO SIMPLER VERSIONS FAILED FIRST, and both failures say something:
        //
        //   TOTAL INNER SHARE cannot express this at all. The thickness target effectively
        //   fixes how much mass the inner band needs, so every candidate satisfying it has
        //   about the same total -- measured 27.5% before and 27.5% after, the term merely
        //   picking a different combination adding to the same figure. It moved two big
        //   folders out and pulled the second-largest in.
        //
        //   BIGGEST INNER FOLDER ALONE got the 10k vault exactly right and left the
        //   450-note one with an 11-note folder on the rim while a 59-note folder sat
        //   inside. Once the largest inner folder is fixed, moving anything smaller across
        //   does not change the term, so those candidates were ties again.
        //
        // The DIFFERENCE is the thing being asked for: not "less inside", not "nothing big
        // inside", but "nothing inside that is bigger than something outside".
        var biggestInner = 0, smallestOuter = Infinity;
        names.forEach(function (g) {
          var n = groupNotes[g] || 0;
          if (a[g]) { if (n > biggestInner) biggestInner = n; }
          else if (n < smallestOuter) smallestOuter = n;
        });
        // No outer groups is the degenerate case priced out above; guard anyway so this
        // reads as 0 rather than as -Infinity leaking into the cost.
        if (!isFinite(smallestOuter)) smallestOuter = biggestInner;
        var innerPeak = totalNotes
          ? Math.max(0, biggestInner - smallestOuter) / totalNotes
          : 0;

        var bc = Infinity, br = R0_BASE;
        for (var m = 100; m <= 300; m += 5) {
          var rv = R0_BASE * (m / 100), t = spanFor(ins, outs, rv);
          var c2 = Math.abs(t.inner - BAND_RATIO * t.outer) +
                   (t.iR > t.oR ? INVERT_WEIGHT * (t.iR - t.oR) : 0) +
                   SIZE_WEIGHT * SP * innerPeak +
                   (t.inner >= t.outer ? 1000 : 0) +
                   // Priced, not forbidden: on a vault where nothing else works the least
                   // bad answer is still a slightly larger hub, and a wall would leave the
                   // search sitting on its starting point.
                   (t.holeShare > HOLE_MAX ? 4000 * (t.holeShare - HOLE_MAX) : 0);
          if (c2 < bc - 1e-9) { bc = c2; br = rv; }
        }
        return { cost: bc, r0: br };
      };
      // Fewer inner rows than outer is a PREFERENCE, not a hard rule, and that ordering
      // is forced rather than chosen. Three things were asked of this split:
      //
      //   1. no small folder in the outer ring        -- hard, and it wins
      //   2. inner rows <= outer rows                 -- preferred
      //   3. inner thickness ~= 0.75 x outer          -- target
      //
      // They are not simultaneously satisfiable. On a 10,000-note vault seven folders fall
      // under the minimum-wedge threshold and are therefore pinned inner, and those seven
      // alone need 34 rows at the hub radius -- so no assignment of the remaining nine
      // gets the inner ring below the outer's 9. The only lever that would is moving small
      // folders outward, which rule 1 forbids: an earlier version reached 20/22 rows
      // exactly by exiling them, which is what rule 1 was added to stop.
      //
      // So (2) is a weighted term. The balancer still prefers the right ordering and pays
      // for inverting, but a hard wall made every candidate infeasible and left the search
      // sitting on its starting point -- measured, 34/9 with nothing improved.
      var INVERT_WEIGHT = 0.5;   // per row of inversion, in drawn units

      // AND A FOURTH, WEAKER PREFERENCE: the big folders belong OUTSIDE.
      //
      //   4. as little of the vault inside as the rest of this allows
      //
      // The three rules above are all geometry -- thickness, row counts, hole size -- and
      // geometry does not care WHICH folders make up a band, only how many rows they need.
      // So among splits that score the same the search kept whichever it reached first,
      // and on the 10k vault that was `05 - Meeting Notes` (1679 notes) and `01 - Projects`
      // (1066) INSIDE while Journal (48), Clippings (92) and Literature Notes (148) sat on
      // the rim. Geometrically fine, and backwards: the outer ring has the circumference,
      // so that is where the folders that need room should go, and a huge folder in the hub
      // is what makes the inner band deep enough to crowd the middle.
      //
      // Expressed as a share of the vault rather than a folder count, and multiplied by SP
      // so it is in the same drawn units as everything else it is added to -- otherwise the
      // weight would mean something different at every disc size. Deliberately small: at
      // 0.275 inner share it costs about half of one row of inversion, so it breaks ties
      // and nudges near-ties, and the thickness target still wins whenever it disagrees.
      // "If possible", which is the whole of what was asked for.
      //
      // The pinned-inner folders are a constant in this term -- they cannot move, so they
      // shift every candidate equally and cannot bias the choice between them.
      var SIZE_WEIGHT = 5.0;
      var cost = function (a) { return evaluate(a).cost; };

      // EXHAUSTIVE when it is cheap, greedy when it is not. Single-move descent gets
      // stuck: on the 449-note vault it settled at a 0.32 ratio while an assignment
      // reachable only by moving two groups at once scores far better. With few groups
      // every split can simply be tried -- 9 groups is 512 candidates, and one candidate
      // is a share() pass over a few dozen cells.
      var EXHAUSTIVE_UP_TO = 14;               // 2^14 = 16384 candidates
      if (movable.length <= EXHAUSTIVE_UP_TO) {
        var bestMask = -1, bestCost = Infinity;
        for (var mask = 0; mask < (1 << movable.length); mask++) {
          for (var b = 0; b < movable.length; b++) assign[movable[b]] = !!(mask & (1 << b));
          var cm = cost(assign);
          if (cm < bestCost) { bestCost = cm; bestMask = mask; }
        }
        for (var b2 = 0; b2 < movable.length; b2++) {
          assign[movable[b2]] = !!(bestMask & (1 << b2));
        }
      } else {
        // Greedy descent, still only over the movable set. Gets stuck where exhaustive
        // does not -- measured, single-move descent settled at a 0.32 ratio on a vault
        // where a two-group move reaches 0.80 -- which is why the cheap case is exhausted.
        var best = cost(assign);
        for (var pass = 0; pass < 60 && best > 1e-9; pass++) {
          var move = null, bc = best;
          for (var i = 0; i < movable.length; i++) {
            assign[movable[i]] = !assign[movable[i]];
            var c2 = cost(assign);
            assign[movable[i]] = !assign[movable[i]];
            if (c2 < bc - 1e-9) { bc = c2; move = movable[i]; }
          }
          if (!move) break;
          assign[move] = !assign[move];
          best = bc;
        }
      }

      // Apply, and leave the pipeline consistent: the trials above mutated c.band and
      // c.bandRef as they went, so the winning assignment is re-shared here rather than
      // left holding whichever trial ran last.
      cells.forEach(function (c) { c.inner = !!assign[c.g]; });
      inner = cells.filter(function (c) { return c.inner; });
      outer = cells.filter(function (c) { return !c.inner; });
      share(inner, "i"); share(outer, "o");
      cells.forEach(function (c) { c.bandRef = c.band; });

      // The hub radius comes from the joint search above -- the winning split already
      // knows which radius produced its score, so it is read back rather than re-solved.
      r0 = evaluate(assign).r0;
    })();

    // Inner band first, from the hub outward; the main band starts past it.
    if (!given) {
      SP_I = bandDensity(inner, "i");
      SP_O = bandDensity(outer, "o");
    }

    // WHY THERE IS NO CONSTANT-EXTENT SOLVE HERE, having built one twice.
    //
    // Holding each ring's thickness fixed and letting the spacing fall out of it is the right
    // shape for the problem -- a disc that keeps its size under filtering is what a person
    // wants -- and it does not survive contact with the row count.
    //
    // The two relations are: rows = what this spacing needs (rowsNeeded RISES with spacing,
    // because a wider row holds fewer notes), and spacing = span / (rows - 1). The composite is
    // monotone DECREASING, so plain iteration flips between two extremes rather than settling --
    // measured on the 10k vault, the inner band landed on 31 rows at 4.096 units, a spacing 60x
    // narrower than the counts had been solved against, which drew a lattice of mostly-empty
    // rows (spread 112 against 0 for a clean band). Damping with the geometric mean converges,
    // and converges to a DEGENERATE point: as the spacing narrows the capacity per row grows
    // without bound, one row suffices, and that drives the spacing back to the cap. Measured
    // there: 18 rows at 0.313 units, spread 164.
    //
    // Fixing the spacing and NOT re-deriving the count holds the span exactly and is the same
    // failure by another route: the counts then belong to a spacing nobody drew.
    //
    // What would work is not an iteration at all. A band of thickness T over an angular span A
    // at radius r has a known area, and a square lattice of pitch s holds area/s^2 notes -- so
    // s and rows = T/s come out of one division, consistent by construction, with the count
    // never solved against a spacing it will not be drawn with. That is a rewrite of rowsNeeded
    // rather than a wrapper around it, so it is left for its own pass rather than bolted on at
    // the end of this one. The per-band density solve below is what ships: it stops the two
    // rings converging, which was the reported bug, and leaves the rim moving under heavy
    // filtering, which is measured in .ai-context/changelog-detail.md.
    // FILL THE BOX. A wedge is a box in polar coordinates -- an inner radius, an outer radius,
    // and two edges running out to the seams -- and the job is to fill it as evenly as the note
    // count allows. Everything reported this session was one consequence of not doing that:
    // holes where a cell held arc in rows it had no notes for, interior steps wider than the
    // boundary gaps beside them, dots undersized because they are drawn against the radial
    // pitch while sitting at a wider tangential step, and quantisation that always fell hardest
    // on the innermost row because it has the shortest arc.
    //
    // rowsNeeded answers the wrong question. It asks how many rows of a GIVEN spacing it takes
    // to hold n notes -- so it is solved per cell, against a spacing that came from somewhere
    // else, and its answer is always at LEAST enough, which means always a bit too much. The
    // band then ends up deeper than its notes can fill: measured on this vault at a 98-note
    // range, the outer band had capacity for about 90 notes over 3 rows and held 57, so its
    // arcs came out 1.58x wider than one pitch and its tangential step was 626 units against a
    // 396 pitch. Exactly the ratio the dots were missing.
    //
    // A band of thickness T at mean radius R has area 2*pi*R*T, and a square lattice of pitch s
    // holds area/s^2 notes. So s = sqrt(area/n) and rows = T/s come out of ONE division,
    // consistent with each other by construction -- no iteration, and no count solved against a
    // spacing it will not be drawn with. Earlier attempts to reach this by feeding a row count
    // back into a spacing solve did not converge: the composite is monotone decreasing, so plain
    // iteration flips between extremes and damping converges to a degenerate point. This is why
    // it had to be a rewrite rather than a wrapper.
    //
    // Every cell then takes the BAND's row count, not its own. With arc allocated in proportion
    // to note count, equal depth makes every cell equally full, which is what puts one step on
    // the whole ring. Low-count folders are the exception and are handled before this: a folder
    // too small to fill a column does not get its own sub-wedge, and a cell with fewer notes
    // than the band is deep is placed one note per row down the middle of it.
    var solveBand = function (list, base, thick, scale, sp) {
      if (!list.length) return { sp: sp, rows: 0 };
      var n = 0;
      list.forEach(function (c) { n += c.wsum; });
      // A BAND WITH NO WEIGHT HAS NO DEPTH, and it has to say so rather than fall through to a
      // count derived from the thickness. Hiding the dominant folder leaves the outer band
      // holding nothing but that cell at weight 0: an empty LIST returns 0 rows below, so a
      // list of empty cells has to as well, or seating them moves maxR -- measured 13 against
      // 21 on the dominant-folder fixture, which is zero-weight invariance (github#5) failing
      // on maxR while every row count matched.
      if (!(n > 0.0001)) return { sp: sp, rows: 0 };
      // Handed a spacing (a cascade walking between two packings), the depth follows from it, so
      // the two stay consistent without re-deriving anything the caller has already decided.
      if (given || !(thick > 0) || !(n > 0.5)) {
        // ROUNDED, and leaving it fractional was tried and reverted the same night. The theory
        // was sound -- the handed-in spacing walks continuously, so round(thick / sp) ticks
        // partway through a cascade, and c.rows decides whether a cell is centred down the
        // radius, which is a discrete change of placement. It bought exactly what it promised on
        // the frame-step check (10k: one frame moving the outer band 54 units against a 47
        // budget, down to 50 against 55) and it broke the invariant that matters: the last frame
        // of a cascade stopped BEING the resting layout, by 785 units of radius, 8419 of
        // tangential travel and 162% of dot size on the dominant-folder fixture. A count that is
        // an integer at rest and a real number on the final frame is two different layouts, and
        // no amount of frame smoothness is worth that.
        var rk = Math.round(thick > 0 && sp > 0 ? thick / sp : 1);
        return { sp: sp, rows: rk > 0 ? rk : 1 };
      }
      var T = thick * scale, R = (base + thick / 2) * scale;
      var s = Math.sqrt(2 * Math.PI * R * T / n);
      var rw = Math.round(T / s);
      if (rw < 1) rw = 1;
      if (rw > 200) rw = 200;
      return { sp: thick / rw, rows: rw };
    };

    // The thickness each band is entitled to, from the locked geometry. Without a lock -- the
    // one unfiltered plan that produces it -- there is nothing to fill yet, so the old per-cell
    // count stands and sets the lock for everyone after.
    // A FIXED SHARE OF THE LOCKED HUB-TO-RING SPAN, not that span minus the live GUTTER. The
    // gutter is 1.6 of the disc-wide spacing, and under filtering that spacing is up to
    // DENSITY_MAX -- so the gutter grew until the inner band had almost no thickness left to
    // fill. Measured: the inner band came out at ONE row holding 34 notes, a step of 65 units,
    // and three overlapping pairs. The band's thickness must not be a function of a number that
    // moves; the lock is there precisely so it is not.
    var thickI = geomLock ? (geomLock.rOuter - geomLock.r0) * INNER_FILL : 0;
    var innerRows = 0;
    if (geomLock && thickI > 0) {
      var si = solveBand(inner, r0, thickI, INNER_SCALE, SP_I);
      SP_I = si.sp; innerRows = si.rows;
      // AN EMPTY CELL TAKES NO ROWS. Every cell with notes takes the band's depth -- that is
      // what makes them equally full -- but a cell whose notes are all hidden has to come out at
      // zero, or seating it changes the answer. That is the zero-weight invariance (github#5),
      // and rowsNeeded used to give it for free by returning 0 for n <= 0; assigning the band
      // depth unconditionally lost it on every folder of every fixture.
      inner.forEach(function (c) { c.rows = c.wsum > 0.0001 ? innerRows : 0; });
    } else {
      inner.forEach(function (c) {
        c.rows = rowsNeeded(usableRef(c, r0), c.wsum, r0, SP_I);
        if (c.rows > innerRows) innerRows = c.rows;
      });
    }
    var rOuter = geomLock ? geomLock.rOuter
               : (inner.length ? r0 + innerRows * SP_I + 1.6 * SP_I : r0);

    var thickO = geomLock ? geomLock.maxR - geomLock.rOuter : 0;
    var maxR = rOuter, outerRows = 0;
    if (geomLock && thickO > 0) {
      var so = solveBand(outer, rOuter, thickO, 1, SP_O);
      SP_O = so.sp; outerRows = so.rows;
      outer.forEach(function (c) { c.rows = c.wsum > 0.0001 ? outerRows : 0; });
      maxR = rOuter + outerRows * SP_O;
    } else {
      outer.forEach(function (c) {
        c.rows = rowsNeeded(usableRef(c, rOuter), c.wsum, rOuter, SP_O);
        if (c.rows > outerRows) outerRows = c.rows;
        var r = rOuter + c.rows * SP_O;
        if (r > maxR) maxR = r;
      });
    }
    // WHY THERE IS NO CORRECTION PASS HERE, having tried one.
    //
    // The open-loop solve leaves maxR a few percent past the locked extent -- measured,
    // reach 1.072 at a density of 1.023 -- and the obvious repair is to feed that back
    // and scale SP until maxR lands on the lock. It was written, and it MADE THINGS
    // WORSE: pitch * sqrt(shown) spread 1.10x -> 1.15x, because the loop drove SP back
    // to 1 in almost every state and undid the whole change.
    //
    // The reason is worth keeping, because it contradicts what this was built on.
    // Filtering a vault BARELY MOVES THE DISC'S RADIUS: maxR is the max over cells, and
    // hiding some folders leaves the deepest survivor holding all of its own notes, so
    // it still reaches the rim. Measured on the baseline, reach was 1.000 with 481, 465
    // and 382 of 503 notes showing -- there was no empty margin to reclaim at all. What
    // filtering does is make the disc SPARSER inside a radius that hardly changes.
    //
    // And that radius is quantised in whole rows. The outermost row already sits flush
    // against the box, so any spreading at all pushes it a full row out: 2.3% more
    // spacing bought 7% more radius. There is no SP between "no change" and "one row
    // over", which is exactly why a loop targeting the lock can only pick SP = 1.
    //
    // So the overshoot is accepted and handled where it belongs -- in the camera, which
    // is the thing that decides how much of the box is on screen. See fitRatio().
    // Rows sit SP apart in every cell -- spacing is never rescaled per cell, which
    // is what keeps density uniform. Each cell's first row is at its band's inner
    // edge, so columns grow outward and a cell ends where its notes run out.
    // Lay a cell's notes out on a grid of exactly `rows` rows. Position is a
    // continuous function of how far through the cell's weight a note sits: s
    // slides smoothly, the row changes only where s crosses a row boundary, and
    // the serpentine keeps u continuous across that boundary (an even row ends at
    // u = 1 and the next row starts at 1 and runs back down).
    // Where a note sits, as ONE continuous formula. Capacity grows linearly with
    // radius, so the cumulative capacity up to a continuous row x is proportional
    // to base*x + SP*x^2/2. Inverting that turns "how far through this cell's
    // weight am I" straight into a row coordinate, with no binning step anywhere.
    //
    // This replaced blending two adjacent integer grids. That blend was the last
    // source of jumps: a note's row differs by up to one between the two grids,
    // and the serpentine parity can differ with it, so u could swing from one end
    // of the wedge to the other -- measured at 1996 units in a frame. Here the row
    // count may be fractional and the coordinate simply slides: the row ticks when
    // the coordinate crosses a boundary, and the triangle wave keeps u continuous
    // across that tick (an even row runs 0 -> 1, the next runs 1 -> 0).
    function placeCell(c, rows, base, bandRows) {
      // The spacing of the band this cell is in -- rows sit this far apart, and the capacity
      // arithmetic below inverts against the same number.
      var SP = c.inner ? SP_I : SP_O;
      // ONE STABLE ORDER, and weight decides what a note costs.
      //
      // This used to partition into live and dead at W > 0.0001 and lay out live-then-dead. The
      // angular position below is a RUNNING SUM over this sequence, so the frame an arriving
      // note's weight crossed that threshold it moved from the end of the sequence to its
      // natural place -- and every note after it got a new position. Measured on one folder
      // toggle: 2482 units TANGENTIALLY with zero radial movement on frame 2, and 2235 on frame
      // 127 as the departing notes fell back below it. A whole cell reshuffling in one frame,
      // twice, at the two moments an animation is most visible.
      //
      // The partition was never needed. A note of weight 0 adds 0 to the running sum, so it
      // already costs its neighbours nothing -- that is what zero-weight invariance means, and
      // the check behind it only ever covered rows and maxR, which is why this survived. Keeping
      // c.list's own order means there is no reordering EVENT: a note grows into its seat from
      // zero and shrinks out of it, and nothing else has to move to make room.
      var seq = c.list;
      var wTot = 0;
      seq.forEach(function (id) { wTot += W(id); });
      // How many notes are really here, as a continuous number rather than a count. Used for
      // centring, which stepped a whole block by a row whenever a count changed.
      var nEff = wTot;

      var total = base * rows + SP * rows * rows / 2;
      // Computed from the cell's FIXED reference width, never the live span, for the
      // same reason its row count is: a pad that moved with other folders' weights
      // would slide every note sideways whenever anything else was toggled.
      // Reuses the pad the row count was solved with (see usableRef), so placement
      // and density cannot disagree. Recomputing it here is the fallback for a plan
      // built before that ran.
      var pad = typeof c.pad === "number" ? c.pad : padFor(base, c.bandRef);
      var span = 1 - 2 * pad;
      // PASS 1 -- which row each note lands in. Unchanged: a continuous row
      // coordinate inverted out of the cumulative capacity, so the count may be
      // fractional and the row simply ticks as it crosses a boundary.
      // A CELL WITH FEWER NOTES THAN ITS BAND IS DEEP GOES DOWN THE RADIUS, one note per row,
      // as a block centred in the band.
      //
      // The capacity inversion below is the right answer for a cell that fills its arc and the
      // wrong one for a narrow cell that cannot. A 4-degree wedge holds a quarter of a note per
      // row, so its notes pile into the innermost rows and the rest of its arc is empty at
      // every radius outside them -- and because a cell holds its arc for the WHOLE band, that
      // emptiness is a hole in every row it fails to reach. Measured on a 454-note vault, one
      // pair of folders had a gap between them at every date range tried: 1111, 1148, 1027 and
      // 1614 units against row medians of 331 to 767. Capping the row count moved that hole
      // outward instead of closing it, since the dead rows simply changed ends.
      //
      // One note per row occupies the arc at every radius the cell spans, so there is nothing
      // left to be a hole. Each note is then alone in its row, which the angular pass already
      // centres.
      //
      // CENTRED, and on INTEGER rows. A block of n rows in a band of R leaves R-n to divide,
      // and half of an odd remainder is a fractional row -- which is off the lattice, and the
      // lattice is a stated invariant with a check behind it. Rounded, so a one-row remainder
      // lands on one side rather than half a row off on both.
      var centred = bandRows > 0 && nEff > 0.0001 && nEff < bandRows - 0.0001;
      var cStart = centred ? Math.round((bandRows - nEff) / 2) : 0;
      var recs = [], acc = 0;
      seq.forEach(function (id, idx) {
        var w = W(id);
        var s = wTot > 0.0001 ? (acc + w / 2) / wTot : 0.5;
        acc += w;
        s = s < 0 ? 0 : s > 1 ? 1 : s;

        var target = s * total;
        var pp = SP > 1e-9
          ? (-base + Math.sqrt(Math.max(0, base * base + 2 * SP * target))) / SP
          : target / Math.max(1e-9, base);
        if (pp < 0) pp = 0;
        if (pp > rows - 1e-9) pp = Math.max(0, rows - 1e-9);
        // The row is an INTEGER bucket, and the radius comes from it, so every frame
        // is a packed grid rather than a blend of two.
        //
        // Taking the radius from the continuous coordinate instead was tried on
        // 2026-08-22 and reverted the same day. It does remove the end-of-toggle
        // teleport -- measured, 04's worst single-frame step fell 160 -> 2 -- but it
        // puts every note off-lattice on every intermediate frame, which reads as a
        // smeared disc for the whole animation. That is the same failure the row-count
        // note above describes at rest, and trading one bad frame for ~120 mushy ones
        // is the wrong way round. See the changelog.
        // FROM THE WEIGHT FRACTION, not from the index. One note per row either way -- for n
        // equal weights s is (i + 0.5) / n and this is exactly i -- but keyed on a continuous
        // quantity, so a note fading in ticks one row of its own instead of renumbering the
        // block behind it.
        var cRow = 0;
        if (centred) {
          var top = Math.max(0, Math.ceil(nEff - 0.0001) - 1);
          cRow = cStart + Math.min(Math.floor(s * nEff), top);
        }
        recs.push({ id: id, w: w, row: centred ? cRow : Math.floor(pp) });
      });

      // PASS 2 -- where in that row it sits, measured WITHIN THE ROW rather than
      // taken from the fractional part of pp. This is the whole fix for sub-wedges
      // needing a wide gap.
      //
      // `frac` is where the cumulative sum happened to be when it crossed an integer,
      // which is arbitrary in [0,1) -- so a note could land hard against a wedge
      // edge, and the only thing separating it from the neighbouring sub-wedge's edge
      // note was the angular gap. Measured, that put pairs 17-67 graph units apart
      // where two dots need ~126, and shrinking the dot cap to 8 did not help because
      // the separation was the problem, not the size.
      //
      // A note's own half-share of its row's weight is its margin, so the first and
      // last notes of every row sit half a step in from the edges automatically --
      // half a row pitch, ~80 units, which is the clearance that was being bought
      // with a 10-22% inset before. Rows in adjacent sub-wedges now meet one step
      // apart, so they read as continuing each other.
      //
      // Weight-based, not `(i + 0.5) / n`, so it stays animation-safe: a fading note
      // gives up its share continuously instead of vanishing from the distribution.
      // And crossing a row boundary is still continuous because the serpentine
      // reverses -- the last note of row k and the first of row k+1 both sit near
      // u = 1.
      // Each row's total, and the half-share its FIRST and LAST notes would take. Those two
      // are what the margin used to be, and they are what gets stretched away below.
      var rowW = Object.create(null), rowFirst = Object.create(null), rowLast = Object.create(null);
      // ...and how fat the notes at the two ends of each row are. The margin is measured to a
      // note's EDGE, so the end notes' own radii are part of where their centres go.
      var edgeA = Object.create(null), edgeB = Object.create(null);
      recs.forEach(function (r) {
        rowW[r.row] = (rowW[r.row] || 0) + r.w;
        if (rowFirst[r.row] === undefined) rowFirst[r.row] = r.w;
        rowLast[r.row] = r.w;
        // THE SIZE ATTRIBUTE, not a radius. A radius needs the band's room, which is not known
        // here and is known where the margin is spent -- and dotUnits() answers from the PITCH,
        // which is a different number from the room whenever a row is not exactly full.
        var dz = graph.getNodeAttribute(r.id, "size") || 4;
        if (edgeA[r.row] === undefined) edgeA[r.row] = dz;
        edgeB[r.row] = dz;
      });
      var rowAcc = Object.create(null);
      var out = [];
      recs.forEach(function (r) {
        var before = rowAcc[r.row] || 0, tot = rowW[r.row] || 0;
        var t = tot > 1e-9 ? (before + r.w / 2) / tot : 0.5;
        // STRETCHED TO FILL THE WEDGE, so the margin can be added as a constant arc instead of
        // being whatever half a note's share happens to come to here.
        //
        // The weight-based centring above leaves the first note at half its own share from 0
        // and the last at half of its own from 1. That margin is half a column pitch only
        // where a wedge's notes-per-row matches the disc's density; a wedge holding two notes
        // in a row has a share-per-note far wider than the lattice, so it held its end notes
        // much further in than a dense neighbour did -- and the channel between the two
        // stopped being centred on the boundary they share. Measured on the wrap seam at 12
        // o'clock: the outer band, dense and uniform, sat within 0.35 degrees, while the inner
        // band wandered to 4.5 -- 67 units, and visibly off.
        //
        // Taken from the row's own first and last weights rather than from each note's, so the
        // map is monotone: a per-note stretch reorders neighbours as soon as their weights
        // differ, which during a fade is always.
        var hA = tot > 1e-9 ? (rowFirst[r.row] || 0) / (2 * tot) : 0;
        var hB = tot > 1e-9 ? (rowLast[r.row] || 0) / (2 * tot) : 0;
        var keep = 1 - hA - hB;
        if (keep > 1e-9) t = (t - hA) / keep;
        if (t < 0) t = 0; else if (t > 1) t = 1;
        rowAcc[r.row] = before + r.w;
        var rr = (base + r.row * SP) * (c.inner ? INNER_SCALE : 1);
        var u0 = (r.row % 2 === 1) ? 1 - t : t;
        // The serpentine reverses odd rows, so which end of the WEDGE this row's first note
        // sits at flips with it. The allowances flip with it too, or the fat note gets the
        // thin note's clearance every other row.
        var eA = edgeA[r.row] || 0, eB = edgeB[r.row] || 0;
        out.push({ id: r.id, r: rr, u: pad + u0 * span,
                   eA: (r.row % 2 === 1) ? eB : eA,
                   eB: (r.row % 2 === 1) ? eA : eB });
      });
      return out;
    }

    cells.forEach(function (c) {
      var base = c.inner ? r0 : rOuter;
      // At rest this is the integer count, so rows land on the lattice. During a
      // cascade it is walked from the source count to the destination's, and the
      // formula above takes a fractional count directly.
      var rf = rowsOf ? rowsOf(c) : c.rows;
      if (!rf) rf = c.rows;
      c.slots = placeCell(c, rf, base, c.inner ? innerRows : outerRows);
    });

    // THE ROOM EACH BAND HAS, as a property of the PLAN. dotPx sizes a whole band from one
    // figure so that size stays monotone in link weight; that figure has to come from here
    // rather than from the placed notes, for the same reason the spacing does. Derived per
    // frame from live weights it is a step function of the frame -- notes per row is an INTEGER
    // -- and every dot in the band breathed for the length of a cascade, measured at 252% in a
    // single frame with 72 of 122 frames moving more than 5%.
    //
    // Handed in during a cascade, exactly as the spacing and the gap presence are, so the last
    // frame and rest agree by construction rather than by coincidence.
    var roomOf = function (list) {
      var v = [];
      list.forEach(function (c) {
        if (!c.slots || !c.slots.length) return;
        var rn = Object.create(null);
        c.slots.forEach(function (sl) { rn[sl.r] = (rn[sl.r] || 0) + 1; });
        c.slots.forEach(function (sl) {
          var n = rn[sl.r] || 1;
          var step = (c.band || 0) * sl.r * UNIT / n;
          if (step > 1) v.push(step);
        });
      });
      if (!v.length) return 0;
      v.sort(function (x, y) { return x - y; });
      // A TENTH PERCENTILE, and the two neighbouring choices were both measured and are both
      // worse. The MINIMUM is the only value that cannot overlap, and one tight pair then sets
      // the size for its whole band -- the inner band's median dot came out at a quarter of
      // what its room allowed. The FIFTH is not a small step from the tenth: the low tail is
      // notes sitting in wedges only a few degrees wide, and it is steep enough that moving one
      // notch collapsed every dot in the vault onto the pixel floor, diameter over step 0.02 to
      // 0.10. The tenth holds 0.31 to 0.43 across every range with a single overlapping pair,
      // 19 units on a step of 330, at one of them.
      return v[Math.floor(v.length * 0.1)];
    };
    var roomPlan = givenRoom || { i: roomOf(inner), o: roomOf(outer) };

    // A BAND'S DEPTH, FRACTIONAL WHILE A CASCADE RUNS. The integer c.rows is what places notes
    // on the lattice, and it is the wrong thing to REPORT: this figure becomes lastRows, and
    // seamFall is a function of it -- (REF_ROWS/rows)^1.5, which multiplies every channel width
    // and every end margin on the disc.
    //
    // Measured on a folder toggle: rowsOuter went 5 to 4 in one frame and the falloff went 1 to
    // 1.398 with it, so every seam in the ring widened 40% between two frames. At one row it is
    // 11.18, so the step off two rows is larger still. That is the jump reported as the planner
    // failing to reach its end state cleanly -- and the positions were never the problem, which
    // is why walking them harder never helped.
    //
    // rowsOf is the cascade's own interpolation between the two endpoint packings' counts, so
    // taking the depth from it makes the falloff move at the same rate as everything else. At
    // rest rowsOf is absent and this is the integer, so the lattice is untouched.
    /**
     * How deep a band is. ONE DEFINITION, at rest and in motion.
     *
     * At rest this is the area solve -- solveBand's own row count for the band, which is what
     * every cell in it is given. During a cascade it used to be the MAX over cells of the walked
     * per-cell count, with `|| c.rows` behind it, and those are two different quantities:
     *
     *     var v = rowsOf(c) || c.rows || 0;
     *
     * rowsAt returns 0 for a cell in neither endpoint plan ("cell knows best"), and the fallback
     * then reads THIS pass's freshly solved count -- the resting definition -- so such a cell
     * silently switched definition mid-animation. A cell's key carries its split state, so a
     * group whose split differs between the endpoint plans and the live frame has no matching
     * key and takes exactly that path. Measured on the intro, band depth walked 1 -> 1.3 and
     * then jumped to 4, above BOTH endpoints, before settling back to 3. A depth that overshoots
     * its destination and returns is a disc that swells and re-packs mid-animation, and it feeds
     * the seam falloff and the split gate while it does it.
     *
     * The cascade hands in the walk between the two ENDPOINT depths instead -- each of which is
     * a resting area solve -- so the quantity is the same one throughout, is bounded by its own
     * endpoints, and cannot step.
     */
    var depthOf = function (list, fallback, band) {
      var given = spIn && typeof spIn === "object" && spIn.depth ? spIn.depth[band] : 0;
      if (given > 0) return given;
      return fallback;
    };

    return { cells: cells, maxR: maxR, total: planTotal, r0: r0, rOuter: rOuter,
             sp: SP_O, spInner: SP_I, density: density, room: roomPlan,
             // The sub-split gate's inputs, so a probe can see WHY a group did or did not
             // split rather than inferring it from the cell count.
             dbgLive: liveG, dbgSplit: splitOf, presMax: presMax,
             rows: { i: depthOf(inner, innerRows, "i"),
                     o: depthOf(outer, outerRows || REF_ROWS, "o") } };
  }

  // RETIRED 2026-08-22. This used to switch the plan basis on how much of the vault
  // was left on screen -- whole-vault above 55%, a visible-only rebuild below -- and
  // that threshold was the root of a whole afternoon of "jumps". Planning must not
  // depend on HOW MANY notes were toggled:
  //
  //   - it made behaviour inconsistent. Hiding `08 - Meeting Notes` (221 notes) crossed
  //     the line and re-densified; hiding `04 - Daily Notes` (55) did not, so the same
  //     gesture shrank the ring or held it depending on the folder's size.
  //   - a toggle that CROSSED the line animated from a whole-vault planA to a
  //     visible-only planB -- two different packings inside one animation.
  //   - and any code path that hardcoded one side of it silently disagreed with the
  //     other, which is exactly how the ring came to change size after the animation
  //     had finished.
  //
  // The plan is now always built from what is visible. The failure that motivated the
  // whole-vault side -- notes appearing to swap seats on a light filter -- does not
  // apply: a cell's notes are sorted by hubRank, so hiding some COMPACTS the rest
  // inward in order and nothing crosses anything else. Density is then always right,
  // and there is one planner with one basis at every call site.
  var REPACK_BELOW = 0.55;   // kept only so the changelog entry has something to point at

  // planIn lays the same notes out under a DIFFERENT plan than the live one,
  // which is how the cascade interpolates between two packings.
  //
  // strict omits notes this plan did not place, instead of falling back to their
  // current position. That fallback is right for the normal callers -- a hidden
  // note should stay where it is -- but it silently hides "this note has no seat
  // in this packing" from the cascade, which needs to know: it was blending
  // departing notes toward the position they were already at, so a closing wedge
  // never migrated radially while the ring re-densified around it.
  function ringsLayout(planIn, strict) {
    // BEFORE ANY PLACEMENT, because the margins are spent during it. Assigned at the end of the
    // pass -- which is where the measured value has to be written -- the margin would be reading
    // the previous pass's number, which is the staleness this override also removes.
    if (roomNow) {
      if (roomNow.i > 1) bandOf("i").room = roomNow.i;
      if (roomNow.o > 1) bandOf("o").room = roomNow.o;
    }
    // A cell's row count is what sets its density, and rows come from the plan.
    // Filter the vault down hard and the full-vault plan leaves each row holding
    // ~2 notes while its wedge grows to 120 degrees -- measured, 55 notes over 8
    // rows with 88-degree gaps, a spidery disc instead of a filled one. Below this
    // share of the vault the plan is rebuilt from the visible notes, which
    // re-densifies it.
    //
    // The threshold is deliberately generous: hiding one subfolder (82% left)
    // stays on the full-vault plan, which is what stops notes swapping seats.
    // Isolating a group is a big enough change that a reflow is expected anyway.
    var shownCount = 0;
    graph.forEachNode(function (id) { if (visible(id)) shownCount++; });
    // Weight the resting plan by visibility, exactly as a cascade weights it by
    // opacity. Without this the resting plan counted every note at full weight --
    // so a hidden folder still shaped everyone else's bands and row counts, and
    // the layout settle() assigned was the FULL-vault packing. The cascade would
    // close the wedge correctly and then the whole disc would snap back to a
    // different arrangement, which is the "jumps to a completely different frame"
    // at the end of every animation. Since alpha is exactly visible ? 1 : 0 once a
    // cascade settles, the two now agree by construction.
    var plan = planIn || pinnedPlan ||
      buildWedgePlan(true,
                     function (id) { return alpha[id] || 0; });
    // A NULL PLAN IS NOT NECESSARILY AN EMPTY DISC. buildWedgePlan returns null when it has
    // zero RING cells, which a pinned note deliberately leaves (see the "A PINNED NOTE HAS
    // LEFT THE RING" comment above) -- so pinning every currently-visible note (PIN_MAX = 13,
    // genuinely reachable on a small or filtered vault) hits this exact branch with the hub
    // still full. Returning null unconditionally used to mean applyLayout's `if (!targets)`
    // no-op skipped hubPlace entirely, so the hub notes kept whatever position they last had
    // (or none, on first load) and never got seated into their slots.
    //
    // r0 falls back to the locked hub radius from the last layout that DID have a ring --
    // geomLock is set once at load and by the time a person can pin thirteen notes there has
    // always been an earlier successful layout to set it from. The 1.5 floor only matters if
    // this is somehow reached before that, and matches buildWedgePlan's own r0 floor.
    if (!plan) {
      if (!pinnedIds().length) return null;
      var hubOut = {};
      hubPlace(hubOut, geomLock ? geomLock.r0 : 1.5, UNIT);
      return hubOut;
    }

    // Live angles: each wedge takes its share of what is VISIBLE, so hiding
    // something makes the rest grow back into a full circle.
    // TWO measures per cell. `geom` is how much room the cell is allocated;
    // `live` is the opacity-weighted count actually on screen. Their ratio is how
    // far open the wedge is within its own arc.
    //
    // What a note contributes to `geom` is the whole trick, and it has to be
    // SYMMETRIC between arriving and leaving or the ring breaks:
    //
    //   - A note on its way OUT counts whatever opacity is left of it. Hiding a
    //     group turns visible() false on frame one, so counting only visible
    //     notes struck the cell out of the allocation instantly and its
    //     neighbours snapped wider while its own notes were still fading in
    //     place on top of them.
    //   - A note on its way IN counts its opacity too, in `trade` mode. Counting
    //     it as a whole slot up front allocated the new wedge its full final
    //     span immediately, while `open` still rendered it at zero width -- so
    //     the ring carried a hole the size of the incoming group (~150 degrees
    //     for 08 - Meeting Notes) that the dots then popped into. Ramping it
    //     makes the other wedges give up their space at exactly the rate the new
    //     one takes it, so the circle stays full and only gets denser.
    //
    // In `draw` mode there is no ring to keep full, so an arriving note does
    // claim its whole slot and the occupied arc grows clockwise instead.
    var live = 0;
    plan.cells.forEach(function (c) {
      c.geom = 0; c.live = 0;
      c.slots.forEach(function (sl) {
        var al = alpha[sl.id] || 0;
        // willShow, not visible. A note on its way out counts what is left of it, and
        // the date range is a way out like any other -- visible() only knows about
        // hidden folders, so an excluded note was counting as a WHOLE SEAT at opacity
        // zero. Same asymmetry the cascade's destination plan had (see willShow), one
        // level down.
        var will = willShow(sl.id);
        // Driver one of the wedge-arc ramp: a fully-toggled single-cell group's rendered
        // share walks linearly instead of tracking the staggered alpha sum -- and it is the
        // GROUP total n0 * ramp, not a per-slot sum, because a faded note leaving the plan
        // takes its slot with it and a per-slot sum stepped down one share per cull: measured
        // as a 1.55-degree single-frame arc step, 16x the mean, mid-hide.
        //
        // BOTH TERMS, and this is the whole reason the neighbours stay still. The reservation
        // and the placement have to carry the SAME presence weight or the ring rotates -- the
        // seam allocator reads presMax while the drawn width is share x (live/geom), so
        // ramping geom alone left the seam on the ramp and the width on the staggered alpha
        // sum. Two clocks, and the wedge next door swung 4.66 degrees out and back across a
        // toggle that moves it 0.00 on develop. With live tied to geom the open fraction is 1
        // and both terms are the ramp, which is exactly the relationship a group of notes
        // fading together has anyway (presMax = a, geom = n * a) -- just on an even clock.
        if (colWalk) {
          var cw = colWalk[groupOf(sl.id)];
          if (cw !== undefined) { c.geom = c.live = cw.n * cw.f; return; }
        }
        c.geom += (fullRing || !will) ? al : 1;
        c.live += al;
      });
      live += c.geom;
    });
    var shown = plan.cells.filter(function (c) { return c.geom > 1e-4; });
    if (!shown.length || !live) return null;
    // How deep this plan reaches, for fit(). Recorded here rather than measured off node
    // positions later: this is plan.maxR, the same quantity geomLock holds for the full
    // vault, so the two divide to exactly 1 on an unfiltered disc. The outermost NOTE sits a
    // little inside the lattice radius it was packed against, so measuring positions made a
    // full disc read as 96% of itself and fit() zoomed slightly in on nothing.
    lastMaxR = plan.maxR || lastMaxR;
    // Recorded beside lastMaxR and for the same reason: both are properties of the
    // packing on screen, and both are read from render-time code that has no plan.
    if (plan.sp > 0) bandOf("o").sp = plan.sp;
    if (plan.spInner > 0) bandOf("i").sp = plan.spInner;
    if (plan.rows) { bandOf("i").rows = plan.rows.i; bandOf("o").rows = plan.rows.o; }

    // The inner and main bands are each a full circle, so they are allocated
    // separately -- a small cell competes only with the other small cells.
    var TWO = 2 * Math.PI;
    var pos = {};
    var fit = Object.create(null);   // id -> room to its nearer row neighbour
    // The last note placed at each radius, ACROSS cells: keyed by row, so the note at the end
    // of one wedge and the note at the start of the next are compared. Reset per band, since
    // the two rings have their own radii and never share a row.
    var lastAt = null, firstAt = null;
    // The interior step of every row of every cell, per band, collected as placement computes
    // it. See where bandRoom is taken from this: it has to be a function of the PLAN, not of
    // the positions that come out of it.
    var roomPool = { i: [], o: [] };
    // Per note, the step of the tightest row of the cell it belongs to. A bound, not a scale --
    // see dotPx. Collected per cell and fanned out to its notes once placement is done, since
    // the tightest row is not known until every row has been walked.
    var cellRoomNext = Object.create(null);
    var cellMin = Object.create(null), cellOf = Object.create(null);
    // Per note, its distance to the nearer edge of its own wedge. A hard cap on the drawn
    // radius -- see where it is filled, and dotPx.
    var edgeCapNext = Object.create(null);
    var dbgCells = DBG.on ? [] : null;
    if (probe) { lastStart = Object.create(null); lastArc = Object.create(null); lastBand = Object.create(null); }
    [true, false].forEach(function (isInner) {
      var band = shown.filter(function (c) { return !!c.inner === isInner; });
      if (!band.length) return;
      var tot = 0;
      band.forEach(function (c) { tot += c.geom; });
      // The RENDERED allocation: sub-gaps and the affordability clamp both apply here.
      // Shared with buildWedgePlan through allocateBand -- see that function for the whole
      // story, including why the group count has to be continuous.
      var a = allocateBand(band,
                           function (c) { return c.geom; },
                           { subGaps: true, clamp: 0.45, totFloor: 1e-6,
                             // FROM THE PLAN, on the same clock as everything else it packs.
                             //
                             // This used to be gapPres, walked 1 -> 0 across the whole cascade
                             // on the cascade's own clock. A note's opacity runs on a per-note
                             // fade clock that finishes far sooner: measured on a vault where
                             // 07 - Yearly Reviews holds a single note, the note reached
                             // present()'s 0.004 floor at 32% of the span and its cell was
                             // correctly culled -- while the walked reservation still stood at
                             // 0.767. Three quarters of a gap released in one frame, and every
                             // wedge boundary in the band shifted to absorb it: 10.66 degrees
                             // on 06 - Monthly Reviews, in a toggle where no note moved more
                             // than 162 units. The wedge and its seams have to shrink on the
                             // clock of the notes they belong to, and now they do.
                             groupPres: plan.presMax || null,
                             band: isInner ? "i" : "o" });
      var gap = a.gap;
      bandOf(isInner ? "i" : "o").gapDeg = Math.round(gap * 180 / Math.PI * 1000) / 1000;
      bandOf(isInner ? "i" : "o").nG = Math.round(a.nG * 1000) / 1000;
      // The SUB-seam count too. Every boundary costs a gap, and a sub-wedge boundary is a
      // boundary: nSub feeds gapTotal exactly as nG does, so a wedge shift with nG flat is
      // invisible without this. It is presence-weighted like nG, but the NUMBER of sub-cells
      // jumps when a group's split gate flips, and that changes avail for the whole band.
      bandOf(isInner ? "i" : "o").nSub = Math.round(a.nSub * 1000) / 1000;
      band.forEach(function (c) {
        c.span = a.shareOf(c);
        if (probe && lastArc) {
          lastArc[c.g] = (lastArc[c.g] || 0) + c.span * 180 / Math.PI;
          lastBand[c.g] = isInner ? "i" : "o";
        }
      });
      // EVERY BOUNDARY COSTS ONE SEAM, group or subfolder, so there is a single count to
      // spend. The reference radius is the locked one, used only for the probe's readout and
      // for nothing the disc depends on.
      lastAt = Object.create(null);
      firstAt = Object.create(null);
      var nB = a.nG + a.nSub;
      var refR = geomLock && geomLock.bandR ? geomLock.bandR[isInner ? "i" : "o"] : 0;
      // The boundary rays are fixed at this radius; only the half-seam inset moves with r.
      // Absent (no geomLock yet, first pass) the old per-radius sweep still applies.
      // refR IS ALREADY IN GRAPH UNITS -- geomLock.bandR is what seamAngle divides a width by
      // directly. Multiplying by UNIT again made the reference gap 160 times too small: 0.0004
      // radians, so every wedge lost a full LOCAL seam with nothing reserved against it and the
      // small ones lost half their arc. Measured, 06's wedge came out 5.5 degrees at the inner
      // row against a nominal 11.9, with its note 7 degrees off the middle.
      // NAMED sBand, NOT sRef, and that is not cosmetic: the probe readout inside the
      // placement loop below declares its own `var sRef`, which var-hoists to the top of that
      // callback and shadows this one for the whole of it. The ray path read the shadow --
      // undefined unless the probe branch happened to run -- so it silently never executed and
      // the disc kept the old accumulated boundaries while every measurement said otherwise.
      // Same class as the roomPool/pool hoist that put every note at the origin.
      var sBand = refR > 0 ? seamAt(refR, nB, isInner ? "i" : "o") : null;
      // PER-ROW SHARES -- see rowArcOn. Presence-weighted, so a wedge leaving a row gives its
      // arc up as its notes fade rather than the frame they are culled.
      var rowShare = null;
      if (rowArcOn()) {
        rowShare = Object.create(null);
        var presIn = Object.create(null);
        band.forEach(function (c0, ci) {
          c0.slots.forEach(function (sl0) {
            var w0 = alpha[sl0.id] || 0;
            if (!(w0 > 0.004)) return;
            if (w0 > 1) w0 = 1;
            var rk0 = Math.round(sl0.r * 1000);
            var arr0 = presIn[rk0] || (presIn[rk0] = []);
            if (!(arr0[ci] >= w0)) arr0[ci] = w0;
          });
        });
        Object.keys(presIn).forEach(function (rk0) {
          var arr0 = presIn[rk0], tot0 = 0;
          band.forEach(function (c0, ci) { tot0 += a.fracOf(c0) * (arr0[ci] || 0); });
          if (!(tot0 > 1e-9)) return;
          var acc0 = 0, sb0 = 0, seams0 = [], before0 = [], frac0 = [];
          band.forEach(function (c0, ci) {
            var p0 = arr0[ci] || 0;
            sb0 += p0;                       // one seam before every cell present in this row
            seams0[ci] = sb0;
            before0[ci] = acc0;
            frac0[ci] = a.fracOf(c0) * p0 / tot0;
            acc0 += frac0[ci];
          });
          rowShare[rk0] = { seams: seams0, before: before0, frac: frac0, nB: sb0 };
        });
      }

      // A gap BEFORE EVERY GROUP, including the first -- that leading one is the wrap
      // boundary between the last group and the first, so the ring still reads as even.
      //
      // Every placed increment carries the same presence weight the reservation used, or the
      // two disagree and the wedges stop filling the circle. Placing one gap per group makes
      // the total exactly gap * nG, equal to the reservation, with nothing left over.
      //
      // It used to be half a gap at the start and half left at the end, which made the
      // leading offset depend on *which group happens to be first in the sweep*. When that
      // group's presence reached zero it dropped out of `band`, `band[0]` became the next
      // group at presence 1, and the offset jumped 0 -> gap/2 in a single frame: the whole
      // ring rotated ~1 degree. Reported as "toggling 03 makes 08 move slightly left on the
      // last frame". Now every term is continuous, so no group's departure can rotate the
      // disc. Costs a constant half-gap of rotation against the old layout -- 1 degree,
      // fixed, and invisible.
      // COUNTERS, NOT ANGLES. The sweep used to accumulate radians, which forced one gap width
      // on the whole band; it now accumulates how many SEAMS and how much of the wedge FRACTION
      // lie before each cell, and the angle is worked out per note from its own radius. Both
      // terms carry exactly the presence weights the reservation used, as before -- that is what
      // keeps a departing group from rotating the disc.
      var seamsBefore = a.groupPres[band[0].g], fracBefore = 0, prevG = null;
      band.forEach(function (c, cIdx) {
        if (prevG !== null) seamsBefore += (c.g !== prevG) ? a.groupPres[c.g] : a.presOf(c);
        prevG = c.g;
        var frac = a.fracOf(c);
        // Recorded for the probe: the leading edge of each group's wedge, which is what
        // moves when the gap reservation changes. Only the first cell of a group writes,
        // so this is the group's own start rather than its last subfolder's.
        if (probe && lastStart && lastStart[c.g] === undefined) {
          var sProbe = seamAt(refR, nB, isInner ? "i" : "o");
          lastStart[c.g] = Math.round((sProbe.gap * seamsBefore + sProbe.avail * fracBefore) *
                                      180 / Math.PI * 1000) / 1000;
        }
        // The wedge keeps its FINAL start angle and only its span opens, so an
        // arriving note fans its own wedge wider instead of shoving every other
        // wedge round the disc. Normalising by the running total instead would
        // hand the first arrival a 300-degree share that shrinks as the rest
        // land -- the whole pie sloshing about while it fills.
        // The wedge grows CLOCKWISE from its leading edge, and the ring stays
        // contiguous: theta advances by the OPEN span, not the full one, so a
        // half-arrived wedge is half as wide and its neighbour butts straight up
        // against it. A growing wedge therefore opens up *between* its two
        // neighbours and pushes what follows round the circle, which is what
        // makes the motion read along the circumference instead of radially.
        var open = c.geom > 1e-6 ? c.live / c.geom : 0;
        // THE RADIUS-INDEPENDENT PART OF THIS CELL'S TWO BOUNDARIES, with the wrap rotation
        // folded in. Everything that reads a boundary -- the notes below, the overlay, the
        // probes -- goes through edgeSweep from here.
        c.bandKey = isInner ? "i" : "o";
        c.nB = nB;
        if (sBand) {
          var A0c = sBand.gap * seamsBefore + sBand.avail * fracBefore;
          c.pLead = A0c - sBand.gap;
          c.pTrail = A0c + sBand.avail * frac * open;
        } else {
          c.pLead = undefined; c.pTrail = undefined;
        }
        // The overlay's copy of this cell's envelope: how many seams lie before it, and where
        // its OPEN span starts and ends as a fraction of the band's available arc. Radius-free
        // on purpose -- the seam is a constant width, so the drawn edge is a curve, and the
        // overlay evaluates it through the same seamAt the notes go through.
        // Captured as the RAYS the placement now uses, plus the reference gap, so the overlay
        // reconstructs exactly what the notes were placed with rather than a formula that was
        // true one version ago.
        if (dbgCells) {
          dbgCells.push({ g: c.g, k: c.k, inner: !!c.inner, nB: nB, bandKey: c.bandKey,
                          seams: seamsBefore, f0: fracBefore,
                          f1: fracBefore + frac * open,
                          pLead: c.pLead, pTrail: c.pTrail,
                          ids: c.slots.map(function (sl) { return sl.id; }) });
        }

        // How many notes this cell puts in each row -- what its end margins are made of, see
        // the zero point below. Counted over the notes that are actually there.
        // WEIGHTED, NOT COUNTED, and this is the end-of-animation jump. The end margin below is
        // arc/(2n) -- half the row's own step -- so n decides where every note in the row sits.
        // Counted as "notes present", n includes a departing note for the whole cascade and
        // drops it at rest, in one step, and the margin steps with it: measured on a date range,
        // the last frame and the resting layout differed by up to 301.9 units of TANGENTIAL
        // movement on an interior step of ~600, with radial and size deltas of exactly 0. Half a
        // step sideways, on the last frame, for every note in the row.
        //
        // Alpha instead: a note leaving contributes less and less of a place until it
        // contributes none, and the frame before rest and rest itself are the same number by
        // construction. Same reason weightOf is opacity rather than membership everywhere else
        // in the packing.
        //
        // rowsUsed the same way -- as a sum of per-row occupancy rather than a count of keys,
        // it stops being a staircase too.
        var rowN = Object.create(null);
        c.slots.forEach(function (sl) {
          var w = alpha[sl.id] || 0;
          if (w > 0) rowN[sl.r] = (rowN[sl.r] || 0) + w;
        });
        var rowsUsed = 0;
        Object.keys(rowN).forEach(function (rk) {
          rowsUsed += rowN[rk] > 1 ? 1 : rowN[rk];
        });
        if (!(rowsUsed > 0)) rowsUsed = 1;
        // The cell's OUTERMOST occupied row, which is the one the eye reads as the ring's edge.
        var maxRowR = -1;
        Object.keys(rowN).forEach(function (rk) { if (+rk > maxRowR) maxRowR = +rk; });
        c.slots.forEach(function (sl) {
          if (!present(sl.id)) return;
          // The seam this note's own row pays for, and the wedge edges either side of it.
          //
          // TIMES UNIT, because sl.r is in LATTICE units -- the row pitch there is SP = 1,
          // and positions are scaled by UNIT on the way out (see `scale` below). Feeding a
          // lattice radius to a width measured in graph units made every seam 160x too
          // wide: 0.15 * 160 / 24 is a full radian, so the gaps ate the disc.
          //
          // sl.r and not the pushed radius: a highlight is a display offset and must not
          // change an angle.
          // seamAt AND NOT THE ALLOCATOR'S ANGLE, deliberately. The seam is a constant WIDTH,
          // so its angle shrinks as 1/r -- which is what makes the channel between two wedges
          // PARALLEL-SIDED rather than a widening slot. A constant angle was tried here and it
          // is the wrong geometry: it makes wedge boundaries radial rays, so the channel opens
          // out with radius, which is the "gaps should not be wedge anymore" this was built to
          // fix. See decision 0002 in .ai-context.
          var rs = rowShare ? rowShare[Math.round(sl.r * 1000)] : null;
          // EVERY SEAM GETS ITS OWN HALF-GAP, NOT AN ACCUMULATED ONE.
          //
          // The sweep used to be built at each note's own radius: gap(r) * seamsBefore +
          // avail(r) * fracBefore. Multiply that out and a boundary is
          //
          //     theta(r) = 2*pi*frac + gap(r) * (seams - 0.5 - nB * frac)
          //
          // whose 1/r coefficient IS its distance from the centre of the disc. It vanishes only
          // where a boundary's seam INDEX matches its share of the CIRCLE -- true at the wrap by
          // construction and nowhere else unless every wedge is the same size. Measured on the
          // reporter's vault, two of nine seams pointed at the centre and the rest missed by 32
          // to 101 units, worst just past 04 - Daily Notes, which takes 233 degrees behind a
          // single seam. Reported as "the seam lines are not all radial", and they were not.
          //
          // Now the boundary is a fixed RAY -- the whole accumulation is evaluated once at the
          // band's reference radius -- and each side of it is inset by half a seam at the note's
          // own radius. So every seam is a constant-width channel about a radius, tilted by
          // W/2 and nothing else: the same 29 units everywhere, instead of however uneven the
          // wedges before it happen to be.
          var sm = seamAt(sl.r * UNIT, rs ? rs.nB : nB, isInner ? "i" : "o");
          var a0, a1;
          if (c.pLead !== undefined) {
            a0 = edgeSweep(c, "lead", sl.r * UNIT);
            a1 = edgeSweep(c, "trail", sl.r * UNIT);
          } else if (rs && rs.frac[cIdx] > 0) {
            a0 = sm.gap * rs.seams[cIdx] + sm.avail * rs.before[cIdx] - sm.gap / 2;
            a1 = a0 + sm.avail * rs.frac[cIdx] * open;
          } else {
            a0 = sm.gap * seamsBefore + sm.avail * fracBefore - sm.gap / 2;
            a1 = a0 + sm.avail * frac * open;
          }
          // ROTATED BACK BY HALF A GAP, so the WRAP GAP is centred on 12 o'clock rather
          // than starting there. The allocation places a full gap before the first group
          // (see the leading-offset note above), and sweep 0 is 12 o'clock -- so the gap
          // between the last group and the first spans [0, gap] and 12 o'clock sits on
          // its leading edge, which reads as the disc being rotated half a gap clockwise.
          //
          // Applied here, at the sweep-to-angle conversion, rather than by changing the
          // leading offset: the offset arithmetic is what makes every term continuous
          // when a group fades out, and it is not worth disturbing for a constant
          // rotation. `gap` is this BAND's own already-clamped gap, so each ring rotates
          // by its own half-gap -- one line covers both.
          //
          // A constant, deliberately not scaled by presence: anything presence-weighted
          // here would rotate the ring as notes arrive, which is the class of bug the
          // leading-offset note describes.
          if (probe && probe.watch === sl.id) {
            probe.watched = { k: c.k, g: c.g, u: Math.round(sl.u * 1e5) / 1e5,
                              slotR: Math.round(sl.r), slots: c.slots.length,
                              a0: Math.round(a0 * 1e4) / 1e4, a1: Math.round(a1 * 1e4) / 1e4,
                              span: Math.round(c.span * 1e4) / 1e4,
                              open: Math.round((c.geom > 1e-6 ? c.live / c.geom : 0) * 1e4) / 1e4,
                              geom: Math.round(c.geom * 1e3) / 1e3,
                              live: Math.round(c.live * 1e3) / 1e3,
                              inner: !!c.inner };
          }
          // NO EDGE INSET HERE, and it is worth saying why not, because one was tried.
          //
          // Wedges visibly interleaved along their boundary at 10k, so each wedge was made to
          // give up one dot radius at each end. That fixed the symptom and was the wrong fix:
          // placeCell's half-share margin already holds the end note half a step in, ~80
          // units, which is comfortably more than a dot's 62-unit radius -- there was never a
          // shortage of room. What there was, was dots drawn at 175 units of radius because
          // the size floor had stopped tracking the lattice (see DOT_OF_PITCH). Insetting on
          // top of a sufficient margin just moved the boundary out to twice a normal step and
          // read as a slice missing from the disc.
          // THE END MARGIN: the zero point, plus this band's share of the extra separation,
          // plus how far this end note differs from a typical one.
          //
          // That last term is what keeps the channel centred between the two dots' EDGES rather
          // than their centres -- dot radius runs 4x from a leaf to a hub, so equal centre
          // distances put edges visibly unequal distances from the boundary, which is what a
          // screenshot of the 12 o'clock axis showed. It is a DIFFERENCE and not a radius, so
          // it costs no width on average.
          //
          // Capped together at two thirds of the arc, because a sliver wedge holding one hub
          // note could otherwise ask for more room than it has and invert.
          var arc = a1 - a0;
          var rGraph = Math.max(1e-6, sl.r * UNIT);
          // HALF A PITCH IS THE ZERO POINT: two wedges each holding their end note half a step
          // in from their own edge put those two notes exactly one step apart, which is a
          // boundary nobody can see. Everything past that is the channel, and the channel is
          // what gets scaled -- including the seam, which sits inside it.
          // THE CHANNEL IS A CLEARANCE BETWEEN RIMS, so the margin is that clearance plus the
          // end note's OWN radius.
          //
          // Three attempts got here. The row's half-share of its own arc made the boundary gap
          // equal the interior step, and made the margin a function of the ROW, so every row
          // with a different note count started at a different angle. One margin per band fixed
          // that and still measured to centres -- and dot size follows link weight, so equal
          // centre spacing leaves unequal RIM gaps: 28 to 89 units across one boundary's rows on
          // the 10k vault, a 3.2x variation, which is the ragged seam that was reported.
          //
          // Adding the end note's radius makes the quantity held constant the one a person sees.
          // eA and eB are the sizes of the notes at this row's two ends, serpentine-corrected in
          // placeCell, so each side pays for the note that actually sits there.
          //
          // Divided by this note's own radius, so the clearance is a constant WIDTH and the two
          // wedge edges are parallel to the channel between them -- not a constant angle, which
          // is a radial edge and a channel that widens outward.
          var bk = isInner ? "i" : "o";
          var room = bandOf(bk).room > 1 ? bandOf(bk).room : pitchUnits(bk);
          var clear = CLEAR_OF_ROOM * room * (GAP_BAND[bk] || 1);
          var radOf = function (z) {
            return DOT_OF_PITCH * room * (Math.min(z || 4, NODE_MAX) / NODE_MAX);
          };
          var nRow = rowN[sl.r] > 0.001 ? rowN[sl.r] : 1;
          // THE STEP, but with a CONTINUOUS count. nRow is how many notes are present in this
          // row right now, an integer, so a step built on it is a step function of the frame --
          // and since dotPx sizes a whole band from a percentile of these, every dot in the
          // band moved whenever any row gained or lost a note. Measured: 211% in a single
          // frame, 28 of 122 frames past 5%.
          //
          // The cell's opacity-weighted count over the rows it occupies is the same quantity
          // with the staircase taken out -- a note arriving contributes its alpha rather than
          // suddenly contributing one -- so it slides where nRow ticked. The margin above keeps
          // nRow, which is right: a margin is where a note SITS, and it has to be the half-step
          // of the row the note is actually in.
          // THE CELL'S TIGHTEST ROW, not its average. Both figures are one number per cell, so
          // either keeps size monotone in link weight within the cell -- but the average lets
          // the cell's closest pair be closer than the number that sized it, and the band
          // percentile above cannot see that either. Measured on the 10k fixture with a folder
          // hidden: one overlapping pair at -1 unit, both notes in the same sub-wedge, radius 48
          // each on a 94-unit arc, where the cell's average step was wider.
          //
          // A row's step is arc/n at that row's radius, and the innermost row of a cell is the
          // shortest arc, so the minimum is where the cell is tightest. Bounding by it costs a
          // little size in cells whose rows differ a lot and nothing anywhere else.
          // ONLY FROM A ROW THAT HAS NOTES IN IT, and this guard is the whole bug behind
          // "unfocused tabs load with huge gaps".
          //
          // nRow is the ALPHA-WEIGHTED count of notes in this row, which is right for a margin
          // -- a note arriving contributes its opacity rather than suddenly contributing one --
          // and catastrophic as a divisor. Early in the intro every row holds a fraction of a
          // note, so arc / 0.001 is a thousand times the real step; the pool fills with those,
          // its tenth percentile explodes, and every margin built on it explodes with it. The
          // margins ARE the channels, so the disc draws with enormous gaps.
          //
          // A visible tab animates out of that within a second and nobody sees it. A BACKGROUND
          // tab has requestAnimationFrame throttled to a crawl, so it is left parked on an early
          // frame's geometry -- which is exactly the report: load three vaults, and only the one
          // in the foreground looks right. Measured in the foreground it is still visible as
          // transient spikes, worst gap jumping to 4.55x mid-intro against 1.84x at rest.
          //
          // Half a note is the threshold: a row with less than that in it has nothing to say
          // about spacing, and the rows that do are enough to take a percentile over.
          // ONLY FROM A ROW WITH TWO NOTES IN IT, because a step is a distance BETWEEN notes.
          //
          // arc / nRow is read as "the room a note has", and for nRow = 1 that is not a step at
          // all -- there is no second note in the row to be spaced from. Counting it made a
          // small folder's own narrow wedge the bound on its dot: 14 - Reading List, one note,
          // came out at radius 11-23 against 38-55 for the crowded folders, for a note whose
          // degree is 7. Reported twice, as small folders rendering tiny.
          //
          // A lone note is bounded by its distance to its own wedge edge instead, which edgeCap
          // already does, and its neighbours across that edge have each reserved
          // clear + DOT_OF_PITCH * bandRoom -- so a band-sized dot there cannot reach them.
          // The rows that DO hold two or more notes are what a percentile should be taken over.
          if (nRow > 1.5) {
            var ownStep = arc * rGraph / nRow;
            roomPool[isInner ? "i" : "o"].push(ownStep);
          // ...AND KEPT PER NOTE, as a bound. The band percentile is one number for a whole
          // ring, so a cell packed tighter than the tenth percentile overlaps -- measured by
          // hiding folders one at a time on the 1402-note vault, 16 pairs at -78 units. This is
          // the same continuous quantity as the pool, so bounding by it costs nothing in
          // smoothness, unlike the measured neighbour distance it replaces: that was a minimum
          // over WHICH note is nearest, and which note is nearest changes as the disc moves.
          //
          // Per CELL rather than per row, which is the point. A note's size may depend on how
          // densely its own folder is packed; it may not depend on which row of that folder it
          // landed in, because rows differ in arc and the innermost is always shortest -- that
          // is the gradient that made the most-connected note the smallest one.
            if (cellMin[c.k] === undefined || ownStep < cellMin[c.k]) cellMin[c.k] = ownStep;
          }
          cellOf[sl.id] = c.k;
          var seamArc = sm.gap * rGraph / 2;      // this side's half of the seam, in units
          var keep = EXCESS_KEEP * seamFall(isInner ? "i" : "o");
          var typ = dotTyp(isInner ? "i" : "o");
          // ONE WIDTH PER BAND, not one per row. Sizing each half of the channel from the
          // END NOTE OF THAT ROW is exact per note and wobbly per disc, because the drawn dot
          // is min(dotUnits, dotFit) while the room reserved here was the uncapped dotUnits --
          // so wherever dotFit bites, the channel keeps room for a dot that never arrives.
          // Measured on the demo vault, the room beyond a wedge's own step ran -6 to +28 units
          // on a 160 pitch, with the two sides of one boundary disagreeing by up to 22: gaps
          // that read as notes not reaching the seam, worst where the end notes are fattest.
          //
          // The band-typical dot instead. A genuinely fat end note now sits marginally closer
          // to the channel than a typical one does, which is the trade: a channel of one
          // visible width beats a per-note-exact one that changes every row.
          // THE MARGIN IS THE CLEARANCE, with nothing added and nothing taken away.
          //
          // This used to scale an "excess" -- zero + keep * (excess + seamArc) - seamArc, with
          // keep falling off against the row count -- from when the margin was half a step and
          // the channel was the part above that. There is no excess now: the margin is exactly
          // what the largest dot needs, and the seam is reserved separately by allocateBand and
          // spent by the sweep. Scaling it again took a dot through its own boundary and into
          // the seam, which was reported twice; a floor was then added to stop it, and the floor
          // was the same number the scaling started from.
          // DIVIDED BY THIS NOTE'S OWN RADIUS, so the margin is a constant WIDTH and the two
          // wedge edges are parallel to the channel between them. Converting at the band's
          // reference radius instead makes it a constant angle, which is a radial edge and a
          // channel that widens outward -- the same error as above, one level down.
          // radOf() IS A GUESS AND GUESSING IS THE BUG. The drawn radius is
          // ramp(size) * min(bandRoom, cellRoom) * 0.92, then floored and capped -- a different
          // curve with a floor and two caps -- so a margin built on an estimate of it reserves
          // room for a dot that does not arrive. Tried: rim gaps went from a 3.2x spread to
          // 4.9x, with some channels down to 13 units.
          //
          // The margin therefore holds the CENTRE at a constant width until the radius is
          // decided in one place instead of two. See the note at the top of dotPx.
          // EACH END NOTE'S OWN DOT, not the biggest dot in the band.
          //
          // The reservation was one band-typical width at both ends, so a note smaller than
          // the band's biggest stopped short of its own boundary by the difference -- measured
          // at 26 units on one row of 03, and it is the whole reason two ends of one wedge read
          // as uneven while the numbers reserved for them are identical. The end notes' size
          // ATTRIBUTES have been threaded here as sl.eA and sl.eB since the margin moved to
          // this site; they were never used. Scaled by the same room the band's biggest dot is
          // scaled by, so this is that number times the note's own share of NODE_MAX, and the
          // ink of every end note lands on its wedge's edge instead of somewhere behind it.
          //
          // The dots then TOUCH the seam, so the seam is the whole visible channel and has to
          // be wide enough to be one -- see SEAM_ROWS, raised in the same change.
          var side = function (z) {
            var f = (z || NODE_MAX) / NODE_MAX;
            if (f > 1) f = 1; else if (f < 0.15) f = 0.15;
            return (clear + DOT_OF_PITCH * room * f) / rGraph;
          };
          var mgA = side(sl.eA), mgB = side(sl.eB);
          // `arcCap`, NOT a second `room`. This was `var room = arc * 0.66`, redeclaring the
          // band room from the top of this block -- a different quantity under the same name,
          // and the two were kept apart by nothing but statement order: `side()` closes over
          // `room` and is called on the line ABOVE this one, so it read the band room, and
          // everything below read the arc cap. Moving that call one line later would have
          // changed every seam margin silently. A rename, so the behaviour is identical and
          // the trap is gone.
          var arcCap = arc * 0.66;
          if (mgA + mgB > arcCap) {
            var k = arcCap / (mgA + mgB);
            mgA *= k; mgB *= k;
          }
          // No wrap rotation here any more: it is folded into pLead/pTrail (see edgeSweep),
          // which is what lets one expression serve the notes and the overlay both.
          var t = sweepAngle(a0 + mgA + (arc - mgA - mgB) * sl.u);
          // HOW MUCH ROOM THIS NOTE HAS TO ITS OWN WEDGE EDGE, in graph units. dotPx caps the
          // drawn radius at this, which is the only bound on "a dot must not cross into the
          // seam" that cannot go stale.
          //
          // Sizing the MARGIN to the largest dot was the previous attempt and it has a circular
          // dependency: the margin has to be reserved during placement, the room the dot is
          // sized from is a percentile over the placement, and bandRoom is a module variable
          // assigned at the END of the pass -- so the margin was reserved from the PREVIOUS
          // layout's room. When the room grew between layouts the dot outgrew the margin held
          // for it and crossed the boundary, which is the note in the seam, reported twice.
          //
          // A distance is not circular. mgA and mgB are this wedge's own edges, known here,
          // and both are smooth functions of the plan, so capping by them cannot make a dot
          // breathe the way a cap taken from measured neighbours did.
          var spanArc = arc - mgA - mgB;
          var dEdge = Math.min(mgA + spanArc * sl.u, mgB + spanArc * (1 - sl.u)) * rGraph;
          if (dEdge > 0) edgeCapNext[sl.id] = dEdge;
          // Highlighted notes step outward by HL_PUSH rows. Applied here rather than
          // in the packing so it changes nothing about rows, capacities or wedge
          // angles -- a highlight is a pure display offset, and every stability
          // guarantee about reflows survives it untouched.
          // The previous note in THIS ROW, whichever wedge it belonged to. Both notes take the
          // gap and each keeps the SMALLER of its two sides: a dot has to clear whichever
          // neighbour is nearer, not the average of them.
          // AND BOUNDED BY ITS OWN WEDGE. The step above is measured to whichever note is
          // nearest in the row, and across a boundary that note is on the FAR side of the
          // channel -- so the room it reports includes the channel, and a dot allowed to take
          // the room it has will take the channel with it. Two edge notes each claiming their
          // share leaves a fifth of the seam visible, which is the seam gone.
          //
          // mgA and mgB already are the distance from this wedge's edges to its end notes, so
          // the distance from any note to the nearer edge falls straight out of its position
          // along the arc. Room is capped at twice that: a radius is DOT_OF_PITCH of the room,
          // so twice the edge distance keeps the dot inside its own wedge with a fifth of the
          // distance to spare -- the same proportion an interior note keeps from its
          // neighbour. The channel is then whatever it was reserved to be, whoever is standing
          // at the end of the row.
          //
          // spanArc is the one computed above for dEdge -- identical expression, same scope,
          // and it was simply written twice.
          var dLo = (mgA + spanArc * sl.u) * rGraph;
          var dHi = (mgB + spanArc * (1 - sl.u)) * rGraph;
          var edgeRoom = 2 * Math.min(dLo, dHi);
          if (edgeRoom > 1 && (fit[sl.id] === undefined || edgeRoom < fit[sl.id])) {
            fit[sl.id] = edgeRoom;
          }
          var prev = lastAt[sl.r];
          if (prev) {
            var step = Math.abs(t - prev.t) * rGraph;
            if (step > 1) {
              if (fit[sl.id] === undefined || step < fit[sl.id]) fit[sl.id] = step;
              if (fit[prev.id] === undefined || step < fit[prev.id]) fit[prev.id] = step;
            }
          }
          lastAt[sl.r] = { t: t, id: sl.id };
          if (firstAt[sl.r] === undefined) firstAt[sl.r] = { t: t, id: sl.id };
          // HL_PUSH is NOT scaled by the plan's spacing, though it is quoted in rows. Scaling it
          // was tried and is wrong: the constant was sized as a FRACTION OF THE RADIUS
          // ("0.9 rows on a ~13.3-row disc is 6.8%"), and the radius is the thing the
          // density solve holds still -- it is the row COUNT that shrinks as the lattice
          // spreads. At the density cap the disc is ~5 rows deep, so 0.9 rows would have
          // been 18% of it and a highlighted note would have protruded off the stage.
          // Left in lattice units it stays the 6.8% it was tuned to be.
          var rr = sl.r + (isPushed(sl.id) ? HL_PUSH : 0);
          pos[sl.id] = { x: rr * Math.cos(t), y: rr * Math.sin(t) };
        });
        // Contiguous: the next wedge starts where this one's OPEN part ended. In fractions
        // rather than radians now, so it means the same thing at every radius.
        fracBefore += frac * open;
      });
      // THE PAIR AT 12 O'CLOCK, which nothing measured. Every row is walked once, left to
      // right, so its first and last note are never neighbours in the walk although they are
      // neighbours on the disc -- and each therefore took its room from its one inner side.
      // With dots only ever shrinking that was harmless; letting them GROW made it an overlap,
      // measured as a single pair at -13 units on one date range.
      Object.keys(firstAt).forEach(function (rk) {
        var fst = firstAt[rk], lst = lastAt[rk];
        if (!fst || !lst || fst.id === lst.id) return;
        var d = fst.t - lst.t;
        while (d < 0) d += TWO;
        var step = d * Math.max(1e-6, (+rk) * UNIT);
        if (step > 1) {
          if (fit[fst.id] === undefined || step < fit[fst.id]) fit[fst.id] = step;
          if (fit[lst.id] === undefined || step < fit[lst.id]) fit[lst.id] = step;
        }
      });
    });

    // The hub hole is now EMPTY except for the logo. Unlinked notes used to be
    // sunflower-packed into it; they are a wedge of their own now (see groupOf), so
    // nothing is positioned outside the ring system and every note obeys the same
    // lattice. hubPositions is gone with it.

    // A layout unit is a FIXED distance. It used to be baseSpan*0.52/maxR, which
    // normalised the disc to a constant footprint -- so filtering down to 55 notes
    // spread them across the area 440 had used, and the ring structure dissolved
    // into a scattered cloud. With a fixed unit, fewer notes simply make a smaller
    // disc at the same density.
    var scale = UNIT;
    var out = {};
    graph.forEachNode(function (id) {
      var q = pos[id];
      if (q) out[id] = { x: q.x * scale, y: q.y * scale };
      else if (!strict) out[id] = { x: graph.getNodeAttribute(id, "x"),
                                    y: graph.getNodeAttribute(id, "y") };
    });

    // Committed once per pass, replacing the previous pass wholesale: a note that has left the
    // disc must not leave its old spacing behind for the next one to size against.
    // THE TIGHTEST PAIR IN EACH BAND. dotPx sizes every note in a band against this one
    // figure rather than against its own neighbours, so the ordering by link weight survives
    // -- and taking the minimum rather than an average is what makes that safe: the largest
    // dot in the band is sized to fit the closest pair in it, so no pair anywhere can touch.
    // FROM THE PLAN, NOT FROM THE POSITIONS. Taken from the fit map -- which is measured off
    // the placed notes -- this jittered every frame of a cascade: the set of notes in it
    // changes as notes arrive and leave, and a percentile over a moving set moves for reasons
    // that have nothing to do with any note. Every dot in the band therefore breathed for the
    // length of the animation, which was reported as exactly that.
    //
    // The interior step arc/n is a function of the same interpolated plan quantities that
    // produce the positions -- the cell's live arc, its radius, its notes per row -- so it is
    // as smooth as they are, which is the property #13 established for the spacing and the
    // seam. The fit map keeps its own job: bounding an individual dot, not sizing the band.
    var pool = roomPool;
    // THE PLAN'S OWN FIGURE WINS. It is the one the cascade walks, so taking it here is what
    // makes the final frame and the resting layout the same layout.
    var planRoom = plan.room || null;
    // A LOW PERCENTILE, NOT THE MINIMUM. The minimum is the correct bound and the wrong
    // statistic: it is one pair, and one pair that happens to be tight sets the size for every
    // note in its band. Measured on the 454-note vault, taking the minimum put the inner band's
    // median dot at 25 units with a diameter-to-step of 0.12 while its worst real clearance was
    // 93 -- dots a quarter of the size the room allowed, because of a single outlier.
    //
    // The tenth percentile keeps what matters -- ONE figure per band, so size stays strictly
    // monotone in link weight -- and lets the tightest tenth of pairs come closer to touching
    // than the rest. Overlaps are measured rather than assumed; see the changelog.
    var pick = function (v) {
      if (!v.length) return 0;
      v.sort(function (x, y) { return x - y; });
      return v[Math.floor(v.length * 0.1)];
    };
    // FROM THE LIVE ARCS, not from the plan's reference ones. The plan carries a room figure
    // too, and handing it in was tried first on the theory that a walked value must be smoother
    // than a measured one. It is smoother and it is wrong: plan.room is built from c.band, the
    // cell's REFERENCE width, while the disc is drawn from c.span, its live share -- and under
    // filtering those differ several-fold, so every dot collapsed onto the pixel floor,
    // diameter over step 0.02 to 0.10.
    //
    // The measured pool was never the problem. It is filled from arc/n as placement computes
    // it, which moves exactly as smoothly as the positions do; the breathing came entirely from
    // the per-note cap that used to sit on top of it, and with that gone the worst single-frame
    // size change is 2.2% against 252% before.
    // Measured from this pass -- unless a cascade is walking it, in which case the walked value
    // is the answer and measuring would overwrite it with a per-frame statistic.
    if (!roomNow) {
      bandOf("i").room = pick(pool.i); bandOf("o").room = pick(pool.o);
    }
    Object.keys(cellOf).forEach(function (id) {
      var m = cellMin[cellOf[id]];
      if (m > 1) cellRoomNext[id] = m;
    });
    // Walked if a cascade is walking it, measured otherwise -- the same rule the band room
    // follows two blocks up, for the same reason.
    cellRoom = cellNow || cellRoomNext;
    edgeCap = edgeNow || edgeCapNext;
    if (planRoom) { /* kept on the plan for the probe; the live pool is what draws */ }
    dotFit = fit;
    if (dbgCells) DBG.cells = dbgCells;
    hubPlace(out, plan.r0, scale);
    return out;
  }

  /* ---------------------------------------------------------- the pinned hub */

  // Notes pinned into the hub sit in the hole instead of on their lattice row. This is the
  // ONE exception to "every note obeys the same lattice" -- the note above records that
  // hubPositions was deleted to remove exactly that, and it is re-introduced here
  // deliberately, for notes the reader chose rather than for a category the layout decides.
  //
  // github#12 spiked five arrangements of the middle; this is the one that won. The four
  // that did not are gone rather than left behind a setting: two of them (the hollow brain,
  // the dial) painted into the hole from the DOM, which puts them UNDER sigma's canvases
  // where ~4900 converging edges bury them. Notes in the hub are real nodes on sigma's own
  // node canvas, above the edges, so the problem never arises -- which is most of why this
  // is the concept that survived.
  //
  // Thirteen, which is what the 13 blobs in logo-mask.png suggested and what the hole
  // actually holds: hub notes are DOTS and name themselves on hover, exactly like every
  // other note in the disc.
  var PIN_MAX = 13;
  // The outermost hub ring, as a fraction of the hole.
  //
  // 0.50, and the number is measured against the INNERMOST REAL NOTE rather than against
  // r0 -- what "touching the inner ring" means is the gap between the outer pinned dot's
  // edge and the first note of the disc, and both of those carry a radius the hole does
  // not know about. At 0.62 the ball reached 0.865 of that distance on the demo vault:
  // 8.8px of clearance from a 5.8px dot to a note that has its own radius again, which
  // reads as contact. At 0.50 it reaches 0.70, which is ~19px.
  //
  // Both terms scale together, which is why one constant is enough: shrinking the ring
  // shrinks the slot spacing, and hubSizeMult is derived from that spacing.
  var HUB_R1 = 0.50;

  // A BALL, built from hex rings: 1 in the middle, 6 around it, the rest outside those.
  // Below seven there is no centre -- a middle dot among three or four reads as one of them
  // being late rather than as a core -- so those counts are a plain ring.
  function hubRing(out, count, r, phase) {
    for (var k = 0; k < count; k++) {
      // 12 o'clock, clockwise, matching the wedge order around it.
      var t = Math.PI / 2 - ((k + phase) / count) * Math.PI * 2;
      out.push({ x: r * Math.cos(t), y: r * Math.sin(t) });
    }
  }

  function hubSlots(n, r0) {
    if (n <= 0) return [];
    if (n === 1) return [{ x: 0, y: 0 }];
    var R = r0 * HUB_R1, out = [];
    if (n <= 6) {
      // Tighter for the small counts, so three notes read as a cluster in the middle
      // rather than as three notes stuck to the rim of the hole.
      hubRing(out, n, R * (n <= 4 ? 0.62 : 0.86), 0);
      return out;
    }
    out.push({ x: 0, y: 0 });
    var left = n - 1, inner = Math.min(6, left), outer = left - inner;
    hubRing(out, inner, outer ? R * 0.5 : R * 0.86, 0);
    if (outer) hubRing(out, outer, R, 0.5);      // offset, so the two rings interleave
    return out;
  }

  /**
   * The pinned notes that are actually on screen, in slot order.
   *
   * A pin the current filter has hidden is SKIPPED HERE rather than released. Releasing it
   * was the first version and it is wrong in a way that only shows up later: hiding a
   * folder would permanently drop every pin in it, and unhiding would not bring them back
   * -- a transient filter quietly editing a stored choice. Filters are deliberately not
   * persisted (see .ai-context/decisions/0009), so they must not persist anything else
   * either. Skipping keeps the ball gapless and the size count honest while it is hidden,
   * and the note returns to its slot when the filter lifts.
   */
  function pinnedIds() {
    return state.pinned.filter(function (id) {
      return graph.hasNode(id) && willShow(id);
    });
  }

  // The closest two hub slots get, as a fraction of the hole's radius. Written by hubPlace
  // and read by the node reducer: it is what the dots are sized against, so that thirteen
  // notes in the hub are drawn small enough to be thirteen notes rather than one blob.
  var hubSep = 0;

  function hubPlace(out, r0, scale) {
    var ids = pinnedIds();
    hubSep = 0;
    if (!ids.length) return;
    var slots = hubSlots(ids.length, r0);
    // Measured off the slots themselves rather than derived from the count, because the
    // arrangement changes shape at 2, at 7 and again once the outer ring opens -- a formula
    // in the count would have to know all three and would drift the moment one is retuned.
    if (slots.length < 2) {
      // ALONE GETS THE CAP. Deriving a "spacing" from the hole radius gave one note a
      // smaller dot than three of them -- measured 10.93px against 11.73px -- because a
      // ring of three sits at 0.38 of the hole and its members are further apart than
      // HUB_R1. There is no spacing when there is nothing to space against; the only
      // sensible answer is the largest a hub note is allowed to be.
      hubSep = HUB_SIZE_MAX / HUB_SIZE_K;
    } else {
      var best = Infinity;
      for (var a = 0; a < slots.length; a++) {
        for (var b = a + 1; b < slots.length; b++) {
          var d = Math.hypot(slots[a].x - slots[b].x, slots[a].y - slots[b].y);
          if (d < best) best = d;
        }
      }
      hubSep = best / r0;
    }
    ids.forEach(function (id, k) {
      // The note being dragged follows the pointer instead of its slot -- it has not been
      // dropped yet, and snapping it to a seat mid-gesture is the drag arguing with the hand.
      if (nodeDrag && nodeDrag.id === id) return;
      // WRITTEN UNCONDITIONALLY, not only where the ring left a position behind. A pinned
      // note is excluded from the plan now, so under `strict` it has no entry at all and
      // the old `if (out[id])` guard silently skipped exactly the notes this exists for.
      if (slots[k]) out[id] = { x: slots[k].x * scale, y: slots[k].y * scale };
    });
  }

  // How much bigger a hub note is drawn than it would be on the ring.
  //
  // Proportional to the slot spacing, which is the only thing that keeps the ball readable
  // across the range: one note alone can afford to be large, thirteen cannot, and the step
  // between them is not linear in the count because the arrangement changes shape twice on
  // the way. Clamped at both ends -- 2.8 so a lone pin does not become a planet, 1.15 so a
  // full hub is still visibly heavier than the ring it came from.
  var HUB_SIZE_K = 3.5, HUB_SIZE_MIN = 1.15, HUB_SIZE_MAX = 2.8;
  function hubSizeMult() {
    return Math.max(HUB_SIZE_MIN, Math.min(HUB_SIZE_MAX, HUB_SIZE_K * hubSep));
  }

  function isPinned(id) { return state.pinned.indexOf(id) >= 0; }

  // Pin, unpin, or move a note already pinned. `at` is an insert position for a drop; left
  // out, the note joins at the end.
  //
  // A CAP RATHER THAN A REFUSAL: the hole does not grow, so the oldest pin gives way and
  // the gesture always does something. A refusal at thirteen would read as the drag having
  // missed.
  function pin(id, at) {
    var i = state.pinned.indexOf(id);
    if (i >= 0) state.pinned.splice(i, 1);
    if (at === undefined || at > state.pinned.length) at = state.pinned.length;
    state.pinned.splice(at, 0, id);
    // Evict from the front, never the note just added. The first version wrote the guard
    // into the splice index and could drop the wrong note when the new one had landed at
    // position 0 -- which is reachable, since `at` is an insert position.
    while (state.pinned.length > PIN_MAX) {
      state.pinned.splice(state.pinned[0] === id ? 1 : 0, 1);
    }
    return true;
  }


  function unpin(id) {
    var i = state.pinned.indexOf(id);
    if (i < 0) return false;
    state.pinned.splice(i, 1);
    return true;
  }

  function togglePin(id) {
    if (!unpin(id)) pin(id);
    hubChanged(true);
  }

  // A HOVER MUST NOT SURVIVE THE NOTE MOVING OUT FROM UNDER THE POINTER.
  //
  // Sigma re-evaluates what is hovered only when the pointer MOVES, and it tests against
  // the quadtree. animateTo refreshes with `skipIndexation: true` for the length of the
  // tween -- earned there, since re-indexing every frame is what made the frame rate fall
  // -- so for those ~380ms the index describes where the disc used to be. Move the pointer
  // off a note during that window and sigma tests stale geometry, decides the note is
  // still under the cursor, and then nothing moves again to correct it: the halo stays and
  // the rest of the disc stays dimmed until you jog the mouse. That is the "sometimes",
  // and it is the hazard invariants.md already names -- anything driven by a pointer has
  // not earned the flag.
  //
  // Released rather than re-tested, because there is nothing to re-test against until the
  // pointer moves again. Twice: once now, for the hover the drag itself was holding, and
  // once when the tween has settled and the index is honest again, for one acquired
  // mid-flight. If the pointer really is over the note when it lands, the next move
  // re-enters it, which is what hover means.
  function releaseHover() {
    if (!state.hovered) return;
    hideTip();
    hoverTo(0);
  }

  function hubChanged(animate) {
    releaseHover();
    // A FROZEN PLAN MUST NOT OUTLIVE THE MEMBERSHIP IT WAS FROZEN FOR. pinnedPlan is set for
    // the whole duration of an unrelated big cascade (a folder toggle, a range change -- see
    // pinPlan()) so that cascade's own per-frame work stays on one consistent plan. But
    // ringsLayout() (which applyLayout below calls) prefers pinnedPlan over building fresh
    // whenever it is set, with no notion of "fresh for whom" -- so pinning or unpinning a
    // note WHILE that other cascade is still animating handed this hub change a plan built
    // from membership before the toggle, and the ring geometry for one frame disagreed with
    // the pin that just happened. Clearing it here forces a fresh plan from the CURRENT
    // pinned set; safe to null out from under the other cascade because its own frame loop
    // builds its own local plan per frame and never reads pinnedPlan again once running (see
    // the room/cell/edge endpoint capture, which saves and restores it around itself).
    pinnedPlan = null;
    applyLayout(!!animate, releaseHover);
    placeLogo();
    if (savePinned) savePinned(state.pinned.slice());
  }

  /**
   * The hub as the host handed it over, at mount.
   *
   * FILTERED AGAINST THE GRAPH, which is the rule github#12 asked for and the only one that
   * survives contact with a real vault: the store outlives the build it was written from, so
   * a pinned note can be renamed, deleted, or filtered out of the graph entirely by
   * --ghosts or --templates between one build and the next. A missing id is dropped
   * silently rather than held as an empty slot -- the hub is what is in the middle, and a
   * slot for a note that cannot be drawn is a gap in the ball and a wrong count for the
   * sizes to derive from.
   *
   * Capped on the way in as well. Nothing stops a store written by a future version, or
   * hand-edited, from carrying more than the hole holds.
   *
   * A copy, not the caller's array: the host keeps its own reference to whatever it passed,
   * and pinning would otherwise mutate the plugin's settings object in place.
   */
  function seedPins() {
    var want = deps.pinned;
    if (!want || !want.length || typeof want.length !== "number") return;
    var seen = Object.create(null), out = [];
    for (var i = 0; i < want.length && out.length < PIN_MAX; i++) {
      var id = want[i];
      if (typeof id !== "string" || seen[id] || !graph.hasNode(id)) continue;
      seen[id] = 1;
      out.push(id);
    }
    state.pinned = out;
  }

  /* --- drag and drop ------------------------------------------------------- */

  // Dragging a note into the hole pins it; dragging a pinned note out releases it. The
  // same two outcomes right-click gives, reached the way you would reach for them if
  // nobody had told you about right-click -- which is the whole argument for having both.
  //
  // The node FOLLOWS THE POINTER while dragging rather than a ghost being drawn: the thing
  // being moved is the thing under the hand, and the disc it came from stays put behind it.
  // On release the layout reclaims it -- into a hub slot, or back to its lattice seat --
  // and that snap is what tells you which of the two happened.
  var nodeDrag = null;
  // How far the pointer must travel before a press becomes a drag. Same threshold the
  // ribbon uses, and for the same reason: a click with a shaky hand is still a click.
  var NODE_DRAG_MIN = 4;
  // The id of a note that just crossed NODE_DRAG_MIN, so the clickNode this same release
  // is about to fire does not ALSO open the detail card on top of it. clickNode fires
  // regardless of whether a drag preceded it -- sigma decides click-vs-drag off its own
  // press/release bookkeeping, which never sees the movement our mousemovebody handler
  // consumes with stopPropagation -- so without this, dragging a note into or out of the
  // hub popped its card open at the same time, which reads as the drag having a side
  // effect nobody asked for. Set the moment the THRESHOLD is crossed rather than in
  // drop(), which runs too late to matter: it is on the same "mousemovebody" tick that
  // decides the gesture is a drag at all, well before the eventual release.
  var dragJustMoved = null;

  // Where the hub is, in graph units, and whether a point is in it.
  //
  // TIMES INNER_SCALE, matching where the inner ring's own row 0 actually draws
  // (placeCell: `(base + row*SP) * INNER_SCALE`, base being this same r0) -- not the
  // lattice's nominal r0 alone. Without it this boundary sat further out than the ring
  // genuinely starts, so a small inner-locked folder (few enough notes that its whole
  // band collapses to one row) placed that row INSIDE what this function still called
  // the hole: reported as notes overlapping the centre mark ("the notes touch the
  // brain"), github#35. geomLock.r0 itself is the correct answer to "how big is the
  // hole" (the HOLE=0.3-of-disc invariant is solved against it) -- INNER_SCALE only
  // corrects for the inner ring being drawn pulled in from that boundary, which every
  // caller of the ring's OWN geometry already accounts for and this one had not.
  function inHubHole(gx, gy) {
    if (!geomLock) return false;
    return Math.hypot(gx, gy) / UNIT < geomLock.r0 * INNER_SCALE;
  }

  // The drop target, drawn only while a drag is live. Without it the hole is an invisible
  // target and the gesture is a guess -- the first version had no ring and dropping felt
  // like it either worked or did not for no stated reason.
  /* ---- BEGIN: demo automation + debug API -- stripped from the plugin build, see scripts/build-plugin.mjs (stripDemoAndDebug) ---- */
  /**
   * A purely cosmetic pointer, drawn IN THE PAGE and moved by eval -- never at the OS
   * level. `x, y` are PAGE coordinates, the same ones the driver already dispatches
   * Input.dispatchMouseEvent at, so it need not know this element exists at all beyond
   * calling it with the same numbers.
   *
   * This replaces moving the REAL system cursor (scripts/cursor.ps1, `--cursor`), which
   * worked right up until it was asked to drag a NODE rather than click one. Windows
   * delivers real input for wherever the OS pointer physically sits regardless of which
   * process moved it there, so SetCursorPos was a SECOND, genuinely native mouse-event
   * stream landing in the same window as the CDP-injected one -- and the two disagree on
   * `buttons`: real hardware reports none pressed, while CDP's dispatched drag says 1.
   * bindNodeDrag's own "the button came up outside the window" safety check -- there for
   * the real case of a person's drag actually leaving the tab -- read the native
   * stream's buttons:0 as exactly that, dropped the note mid-glide, and let sigma's
   * default panning take over for the rest of the gesture. Measured: worked every time
   * over CDP alone, failed on every take that also moved the real pointer. A DOM element
   * never touches the OS pointer and generates no second stream, so there is nothing
   * left for that check to misread.
   */
  function demoCursorAt(x, y) {
    var el = $("democursor");
    if (!el) return;
    // Against ROOT, not #vg-canvas -- see the element's own comment in page.html for
    // why: nested in the canvas it was clipped by that element's overflow:hidden for
    // every beat whose target was not literally over the disc (the sidebar, the
    // heatmap band, the ribbon), which is most of the storyboard. Same coordinate
    // conversion the right-click colour menu already uses against ROOT, for the same
    // reason: ROOT's own getBoundingClientRect() reflects whatever sits above it.
    var b = ROOT.getBoundingClientRect();
    el.style.left = (x - b.left) + "px";
    el.style.top = (y - b.top) + "px";
    el.hidden = false;
  }
  function demoCursorHide() {
    var el = $("democursor");
    if (el) el.hidden = true;
  }
  /* ---- END: demo automation + debug API ---- */

  function placeHubDrop() {
    var el = $("hubdrop");
    if (!el || !renderer || !geomLock) return;
    if (!nodeDrag || !nodeDrag.moved) { el.hidden = true; return; }
    var c = renderer.graphToViewport({ x: 0, y: 0 });
    // TIMES INNER_SCALE -- see inHubHole, which this ring is drawn to match: the drop
    // target has to be the same boundary the drop DECISION (inHubHole) uses, or the ring
    // shown on screen promises a drop the actual test disagrees with.
    var edge = renderer.graphToViewport({ x: geomLock.r0 * INNER_SCALE * UNIT, y: 0 });
    var d = Math.hypot(edge.x - c.x, edge.y - c.y) * 2;
    el.style.width = el.style.height = d + "px";
    el.style.left = c.x + "px";
    el.style.top = c.y + "px";
    // Lit while the pointer is actually inside, so the ring answers "will this drop?"
    // rather than only "there is a target somewhere".
    el.setAttribute("data-over", nodeDrag.over ? "1" : "0");
    el.setAttribute("data-drop", nodeDrag.wasPinned && !nodeDrag.over ? "out" : "in");
    el.hidden = false;
  }

  // ONE UPDATE PER FRAME, for anything driven by a high-frequency pointer event. A
  // pointermove/mousemove fires 120+ times a second on a decent mouse, and running real
  // layout work on every one of them makes the handler itself the bottleneck -- the drag
  // lags behind the cursor rather than tracking it. Only the LAST call queued before a
  // frame paints ever runs; earlier ones in the same frame are simply superseded, which is
  // correct for anything answering "where is the pointer now" -- only the latest position
  // ever matters. Shared by buildDateUI's ribbon drag and bindNodeDrag's hub-pin drag,
  // which both used to carry their own separate copy of this same four-line pattern.
  function makeFrameCoalescer() {
    var pend = null, raf = 0;
    var flush = function () {
      raf = 0;
      var f = pend; pend = null;
      if (f) f();
    };
    return function onFrame(fn) {
      pend = fn;
      if (!raf) raf = WIN.requestAnimationFrame(flush);
    };
  }

  function bindNodeDrag() {
    var onFrame = makeFrameCoalescer();
    var captor = renderer.getMouseCaptor && renderer.getMouseCaptor();
    if (!captor) return;

    renderer.on("downNode", function (e) {
      // LEFT BUTTON ONLY. downNode fires for any button, so a right-click was arming a
      // drag as well as pinning: hold the right button down, move a few pixels, and the
      // note came with the pointer. Right-click is a click -- it has one outcome, and the
      // pin animation is the whole of it.
      var o = e.event && e.event.original;
      if (o && o.button !== 0) return;
      nodeDrag = { id: e.node, moved: false, over: false, wasPinned: isPinned(e.node) };
      // A fresh press starts clean -- otherwise a drag that never crosses NODE_DRAG_MIN
      // (a plain click) would leave a PRIOR drag's flag sitting there, ready to swallow
      // this click's own clickNode for the wrong reason.
      dragJustMoved = null;
    });

    captor.on("mousemovebody", function (e) {
      if (!nodeDrag) return;
      // The button can come up outside the window, where no mouseup reaches us -- the
      // next move then carries the note around with nothing held down. `buttons` is the
      // live state of the button rather than a memory of the press, so it catches that.
      if (e.original && e.original.buttons !== undefined && !(e.original.buttons & 1)) {
        drop();
        return;
      }
      // THE CAMERA MUST NOT PAN UNDER THE DRAG, and this has to happen FIRST -- before the
      // threshold test, not after it. The early version returned while the pointer was
      // still inside NODE_DRAG_MIN, so those first few moves reached sigma's own handler
      // and panned the stage: measured, the camera left (0.5, 0.5) for (0.4601, 0.5207)
      // over one drag. The press already belongs to the note by then; the threshold only
      // decides whether the note MOVES, never who owns the gesture.
      if (e.preventSigmaDefault) e.preventSigmaDefault();
      if (e.original) { e.original.preventDefault(); e.original.stopPropagation(); }
      if (!nodeDrag.moved) {
        if (nodeDrag.x0 === undefined) { nodeDrag.x0 = e.x; nodeDrag.y0 = e.y; }
        if (Math.hypot(e.x - nodeDrag.x0, e.y - nodeDrag.y0) < NODE_DRAG_MIN) return;
        nodeDrag.moved = true;
        dragJustMoved = nodeDrag.id;
      }
      // .over IS COMPUTED HERE, SYNCHRONOUSLY, not deferred with the rest below -- drop()
      // is bound straight to mouseup/mouseleave (not through onFrame), so it can run before
      // a queued frame flushes and would otherwise read a stale .over from one frame behind
      // the pointer's actual release point. inHubHole is cheap pure math, not the DOM work
      // this throttle exists for, so there is no cost to keeping it immediate.
      var p = renderer.viewportToGraph(e);
      nodeDrag.over = inHubHole(p.x, p.y);
      // THE POSITION WRITE AND THE DOM-TOUCHING placeHubDrop() ARE DEFERRED to the next
      // paint, same reason and same mechanism as the ribbon drag's onFrame calls above --
      // un-throttled this was measured to fall behind on a real recording, per the comment
      // on demoBigInnerNote picking shorter drag distances specifically to give it less
      // distance to fall behind on. Position is read now, off the event that only exists
      // for this call; only the expensive part waits.
      var dragId = nodeDrag.id;
      onFrame(function () {
        if (!nodeDrag || nodeDrag.id !== dragId) return;    // dropped, or a new drag since
        graph.setNodeAttribute(dragId, "x", p.x);
        graph.setNodeAttribute(dragId, "y", p.y);
        placeHubDrop();
      });
    });

    var drop = function () {
      if (!nodeDrag) return;
      var d = nodeDrag;
      nodeDrag = null;
      placeHubDrop();
      if (!d.moved) return;                    // a plain click -- clickNode already ran
      if (d.over && !d.wasPinned) pin(d.id);
      else if (!d.over && d.wasPinned) unpin(d.id);
      // Either way the layout reclaims the note: into its slot, or back to its seat. A
      // drop that changed nothing still animates home, which is the only way to show that
      // the note has a seat rather than being left wherever it was let go.
      hubChanged(true);
    };
    captor.on("mouseup", drop);
    captor.on("mouseleave", drop);
  }

  /* ------------------------------------------------------------- timeline */

  // Notes ranked oldest-first. THE REVEAL IS ORDERED BY RANK rather than by time on
  // purpose: measured on this vault, 409 of 442 notes fall in the last three months while a
  // handful carry content dates back to 2015, so an intro paced by the calendar would spend
  // 97% of its run on empty years and put everything worth watching in the last half second.
  //
  // The strip below it IS paced by the calendar, because a date axis is what a date filter
  // needs. The two meet in sweepTo, which turns the intro's progress into a position by
  // going through the rank -- so the handle says "everything left of here is on screen",
  // which is the same thing it says under a hand, and the crawl through the empty years is
  // the vault's own shape rather than a mapping artefact.
  var tlRank = Object.create(null), tlDate = [], tlMax = 0;
  // The same dates as tlDate, in ms, so the intro's sweep can turn a progress fraction into
  // a position on the ribbon without parsing a string on every one of its ~270 frames.
  var tlDateMs = [];
  // id -> created, in ms UTC. Absent for an undated note, which is what timeFactor keys on.
  var tlMs = Object.create(null);
  // The whole vault's dates, bucketed, built once. Every one of the three concepts needs
  // the same two things -- how many notes per month, and per year -- and building it once
  // is also what stops three controls disagreeing about where the vault starts.
  var dateSpan = null;
  function buildTimeline() {
    var dated = [];
    graph.forEachNode(function (id, a) { if (a.created) dated.push([id, a.created]); });
    dated.sort(function (x, y) { return x[1] < y[1] ? -1 : x[1] > y[1] ? 1 : 0; });
    tlRank = Object.create(null); tlDate = []; tlDateMs = []; tlMs = Object.create(null);
    dated.forEach(function (pair, i) {
      tlRank[pair[0]] = i + 1;
      tlDate.push(pair[1]);
      // heatParse returns NaN for anything that is not a bare ISO day -- an unrendered
      // Templater placeholder, most often -- and those must not get a position on any of
      // these axes. Written as isNaN rather than as ms === ms, which is the same test and
      // reads like a typo.
      var ms = heatParse(pair[1]);
      if (!Number.isNaN(ms)) tlMs[pair[0]] = ms;
      // Kept parallel to tlDate INCLUDING the unparseable ones, so a rank still indexes the
      // same note in both. NaN is carried and clamped where it is read, not dropped here --
      // dropping it would slide every later rank by one.
      tlDateMs.push(ms);
    });
    tlMax = dated.length;
    buildDateSpan(dated);
  }

  /**
   * The whole vault's dates, bucketed by month and by year.
   *
   * Built once, from the FULL graph and never from what is visible, for the same reason the
   * disc's plan is: a control whose own axis moved when you used it would be unusable. All
   * three concepts read this, which is also what stops them disagreeing about where the
   * vault starts.
   *
   * MONTHS ARE DENSE, not sparse -- every month between the first and last gets an entry,
   * including the empty ones. The author's own vault has a whole year with no notes in it
   * (2021), and a control built from only the months that exist would silently close that
   * gap up and lie about the shape of the history.
   */
  function buildDateSpan(dated) {
    dateSpan = null;
    if (!dated.length) return;
    var lo = heatParse(dated[0][1]), hi = heatParse(dated[dated.length - 1][1]);
    if (Number.isNaN(lo) || Number.isNaN(hi)) return;
    var d0 = new Date(lo), d1 = new Date(hi);
    var y0 = d0.getUTCFullYear(), m0 = d0.getUTCMonth();
    var y1 = d1.getUTCFullYear(), m1 = d1.getUTCMonth();
    var months = [], index = Object.create(null);
    for (var y = y0, m = m0; y < y1 || (y === y1 && m <= m1);) {
      var key = y + "-" + (m < 9 ? "0" : "") + (m + 1);
      index[key] = months.length;
      months.push({ key: key, y: y, m: m, ms: Date.UTC(y, m, 1), n: 0 });
      if (++m > 11) { m = 0; y++; }
    }
    var years = Object.create(null);
    for (var i = 0; i < dated.length; i++) {
      var s = dated[i][1], k = s.slice(0, 7), ix = index[k];
      if (ix !== undefined) months[ix].n++;
      var yy = s.slice(0, 4);
      years[yy] = (years[yy] || 0) + 1;
    }
    var ylist = [];
    for (var yk = y0; yk <= y1; yk++) ylist.push({ y: yk, n: years[String(yk)] || 0 });
    var nMax = 1, tot = 0;
    months.forEach(function (mm) { if (mm.n > nMax) nMax = mm.n; tot += mm.n; });
    // THE HEIGHT REFERENCE IS A PERCENTILE, NOT THE MAXIMUM, and the bars are linear against
    // it. Neither half of that is arbitrary; each fixes what the other choice broke.
    //
    // Against the MAX the scale is hostage to one month. On an eleven-year vault the busiest
    // month holds 631 and the median holds under 40, so every bar but one was a hairline --
    // which is what the sqrt was for, and sqrt then did the opposite damage on a two-year
    // vault where the ratio is only about three: every bar came out between 68% and 100% and
    // the strip read as a solid slab with no shape in it at all.
    //
    // p90 with a floor at a third of the max, linear, gives a readable spread in both. A
    // genuine spike clips at full height, which is honest -- it is the tallest thing there --
    // and costs one month's precision to give every other month some.
    var sorted = months.map(function (mm) { return mm.n; }).sort(function (x, y) { return x - y; });
    var p90 = sorted.length ? sorted[Math.floor(sorted.length * 0.9)] : 1;
    var nRef = Math.max(1, p90, nMax * 0.35);
    var yMax = 1;
    ylist.forEach(function (yy) { if (yy.n > yMax) yMax = yy.n; });
    // AXIS SEGMENTS -- built unconditionally, cheap, and read by ribbonX/ribbonMs only when
    // compactAxis is on. WEIGHTED BY NOTE COUNT, not by calendar time (github#23) -- the
    // first version weighed a populated month by its own real duration and only collapsed
    // literally-empty runs, which undersold the actual ask: a year holding a note or two in
    // most months (so nothing in it ever reads as "empty") still summed to a full year of
    // real-time weight, competing evenly against a year an order of magnitude busier just
    // because both have "every month populated". Measured on the vault this shipped
    // against: 2023 (21 notes, spread thin enough to touch nearly every month) drew WIDER
    // than 2026 (399 notes, the vault's busiest year by far) under that scheme, because 2026
    // is only partway through its own calendar year and 2023 is not.
    //
    // Two levels, both against the SAME percentile-with-floor shape `nRef` above already
    // uses for bar height -- proven on this file already rather than invented fresh:
    //
    // YEAR gets a width between YEAR_FLOOR_MS (about one month) and YEAR_CEIL_MS (about one
    // real year) based on its own note count against `yearRef`, the p90-with-floor
    // reference over every year's count -- so a handful of outlier years cannot both crowd
    // every other year to nothing AND still keep climbing themselves; the ceiling is what
    // makes "compact" years equidistant in the first place; a year at or past yearRef reads
    // as fully wide.
    //
    // WITHIN a year, every one of its months gets an EQUAL share of the year's own weight --
    // deliberately not weighted by the month's own note count. Tried the finer-grained
    // version first (each month's own share of its year, floored so a note-less month
    // still got a sliver) and it read as noise: month-to-month note counts inside one year
    // are not a signal worth spending width on, and equal months make every year's own
    // internal month-tick spacing predictable, which the density weighting between YEARS
    // does not need help from.
    //
    // NOT A NO-OP ON AN EVEN-BY-TIME VAULT ANY MORE. The earlier month-real-duration scheme
    // reduced exactly to the linear formula whenever no month was literally empty; this one
    // does not, because it weighs a YEAR by its content rather than by its calendar length
    // even when every month in it has something -- two years of equal length but unequal
    // note counts now draw unequal widths on purpose. `compactAxis: false` is the only path
    // that still reproduces the plain calendar axis; see ribbonXLinear, untouched.
    var AVG_MONTH_MS = 30.436875 * 86400000; // 365.2425 / 12 days -- the Gregorian mean month
    var YEAR_FLOOR_MS = AVG_MONTH_MS;         // a compacted year reads as about one month wide
    var YEAR_CEIL_MS = 12 * AVG_MONTH_MS;     // a fully-referenced year reads as one real year
    var yCounts = ylist.map(function (yy) { return yy.n; }).sort(function (a, b) { return a - b; });
    var yMax2 = yCounts.length ? yCounts[yCounts.length - 1] : 1;
    var yP90 = yCounts.length ? yCounts[Math.floor(yCounts.length * 0.9)] : 1;
    var yearRef = Math.max(1, yP90, yMax2 * 0.35);
    var segs = [], segW = 0, segOfMonth = new Array(months.length);
    ylist.forEach(function (yy) {
      var yFrac = Math.min(1, yy.n / yearRef);
      var yearWeight = YEAR_FLOOR_MS + yFrac * (YEAR_CEIL_MS - YEAR_FLOOR_MS);
      var idxs = [];
      for (var mi = 0; mi < months.length; mi++) if (months[mi].y === yy.y) idxs.push(mi);
      var mw = yearWeight / idxs.length;
      idxs.forEach(function (mi) {
        segOfMonth[mi] = segs.length;
        segs.push({ i: mi, w0: segW, w1: segW + mw });
        segW += mw;
      });
    });
    dateSpan = {
      months: months, years: ylist, index: index,
      lo: months[0].ms, hi: Date.UTC(y1, m1 + 1, 0),      // last day of the last month
      nMax: nMax, nRef: nRef, yMax: yMax, dated: tot,
      undated: graph.order - tot,
      axis: { segs: segs, totalW: segW || 1, segOfMonth: segOfMonth }
    };
  }

  /**
   * The range as two ISO days. Null ends read as the span's own ends, and it says so rather
   * than saying "All dates".
   *
   * It DID say "All dates" when nothing was capped, which put the same two words in the
   * readout and on the button beside it -- one describing a state and one offering to return
   * to it, indistinguishable. Naming the actual dates is also the more useful answer at rest:
   * it is where the reader learns what eleven years of strip actually spans.
   */
  function rangeLabel() {
    if (!dateSpan) return "";
    var f = state.from === null ? dateSpan.lo : state.from;
    var t = state.to === null ? dateSpan.hi : state.to;
    var iso = function (ms) { return new Date(ms).toISOString().slice(0, 10); };
    return iso(f) + "  \u2192  " + iso(t);
  }

  /**
   * Set the range from anywhere, in the ends' own terms.
   *
   * The brush had this inline in its pointerup handler, including the rule that matters most:
   * an end AT the span's own end means "no bound" rather than "the first note", or brushing
   * the whole strip leaves a filter that excludes everything dated outside the known span.
   * Now the date fields and the year labels go through the same door, so they cannot get that
   * rule subtly different -- or forget it, which is the more likely of the two.
   */
  function setRangeMs(from, to) {
    if (!dateSpan) return;
    if (from !== null && to !== null && from > to) { var sw = from; from = to; to = sw; }
    state.from = (from === null || from <= dateSpan.lo) ? null : from;
    state.to = (to === null || to >= dateSpan.hi) ? null : to;
    applyRange();
  }

  /** The chrome that goes with a range, whichever path put it there. */
  function rangeChrome() {
    var el = $("rangenote");
    if (el) el.textContent = rangeLabel();
    // The two fields show the range's ACTUAL ends, which for an open end is the span's own.
    // Bounded by the span as well, so the picker cannot offer a month the vault has no notes
    // in -- and so a typed date is clamped by the control rather than by us.
    if (dateSpan) {
      var lo = isoDay(dateSpan.lo), hi = isoDay(dateSpan.hi);
      var f = $("from"), t = $("to");
      if (f) { f.min = lo; f.max = hi; f.value = isoDay(state.from === null ? dateSpan.lo : state.from); }
      if (t) { t.min = lo; t.max = hi; t.value = isoDay(state.to === null ? dateSpan.hi : state.to); }
    }
    // Nothing to clear when nothing is capped. A live button that does nothing is a worse
    // answer to "is a filter on?" than a dead one.
    var btn = $("rangeall");
    if (btn) btn.disabled = (state.from === null && state.to === null);
    drawDateUI();
  }

  /**
   * A range change that is a JUMP: the All-dates button, or the debug API. Animated, because
   * a discrete filter change animates everywhere else in this page.
   */
  function applyRange() {
    rangeChrome();
    // A range change is NOT a wedge toggle; the wedge-arc machinery below must not engage on
    // a range that happens to empty a group. Every legend/visibility path passes colToggle.
    cascade();
  }

  /* THE DRAG DOES NOT TOUCH THE DISC AT ALL.
   *
   * Two attempts got this wrong before it got simple. The first put cascade() on every
   * pointermove -- a 1600ms reveal that walks the disc one note at a time, cancelled and
   * restarted 120 times a second, so what you saw was dozens of animations each showing its
   * own first frame. The second replaced that with one cheap layout frame per pointermove,
   * which is what timelineFrame does and is right for a rank cutoff: 452 notes.
   *
   * At 10,000 it is still too much. syncAlpha, the packer and a Sigma refresh are each O(n)
   * and they run between the pointer moving and the next paint, so the drag lags the cursor
   * however cleverly it is scheduled. The honest answer is that the disc is not a thing that
   * can be scrubbed at that size.
   *
   * So a drag only moves the RIBBON: the handles, the pill, the readout and the tooltip,
   * which are one small canvas and two spans. `state` is not written either -- the preview
   * lives on the drag object, so an abandoned drag leaves nothing half-applied and there is
   * exactly one moment when the filter changes. The disc updates once, on release, animated
   * like every other filter change in the page.
   */

  // Today's date, read at load rather than baked in at build time, so the band's own
  // today marker stays correct tomorrow without rebuilding the snapshot.
  var TODAY = (function () {
    var d = new Date(), p = function (n) { return (n < 10 ? "0" : "") + n; };
    return d.getFullYear() + "-" + p(d.getMonth() + 1) + "-" + p(d.getDate());
  })();

  // A day clicked on the heatmap, or one under the pointer. Haloed and -- for the clicked
  // one -- recoloured, and NEVER pushed; see isPushed and nodeStyle.
  //
  // `created` ONLY, and never a file's mtime. The band counts notes ADDED, so clicking one
  // of its squares has to mark exactly the notes that square counted. This was the whole of
  // the argument that killed the sidebar's "Mark today", which read created-or-touched and
  // so lit up notes the square never included: on this vault a folder renumbering moved 111
  // mtimes in a day, none of which was a note written. Two things answering "today"
  // differently in one view is worse than one answering it plainly, and the band is the one
  // that draws the number.
  function isMarkedDay(id) {
    if (!state.markDay && !state.hoverDay && state.hoverYear === null) return false;
    var c = graph.getNodeAttribute(id, "created");
    if (c === state.markDay || c === state.hoverDay) return true;
    // A YEAR IS A PREFIX. `created` is an ISO day, so the year is its first four characters
    // -- no parsing, and it costs one string compare per note per frame.
    return state.hoverYear !== null && !!c && c.slice(0, 4) === state.hoverYear;
  }

  // Haloed: any highlight source at all -- a clicked group, a picked or hovered day, a
  // hovered year. Whether a source also MOVES its notes is a separate question, asked of
  // isPushed: a group owns a contiguous wedge and can move as a block, while a day's notes
  // are scattered through every wedge and cannot.
  function isHighlighted(id) {
    if (isMarkedDay(id)) return true;
    var g = groupOf(id);
    if (state.highlight[g]) return true;
    // HOVERING A LEGEND ROW haloes its notes, and deliberately does NOT push them --
    // isPushed does not ask about these two. A hover is a transient answer to "where is
    // this folder", and a wedge sliding out and back under a moving pointer is a lot of
    // motion to spend on a question that is answered the moment the halo lands. Clicking
    // the row still pushes; that is the difference between asking and choosing.
    if (state.hoverGroup === g) return true;
    var a = graph.getNodeAttributes(id), d = a.dirs || [];
    for (var k = 1; k <= d.length; k++) {
      var pk = pathKey(a, k);
      if (state.highlightSub[pk]) return true;
      if (state.hoverSub[pk]) return true;
    }
    return false;
  }

  /**
   * WHERE ONE CELL'S NOTES ACTUALLY REACH, as a fraction of its band's available arc -- the
   * same fraction space f0/f1 live in, so a drawn edge can be put through them.
   *
   * placeCell holds the end note half a step in (the margin is arc/(2n)), so a wedge's
   * ALLOCATED boundary stands about 3.7 degrees clear of its last dot on this vault. Two
   * neighbouring boundaries then sit as a close pair in the middle of a wide empty channel,
   * which reads as the edges touching each other and bounding nothing, with the seam invisible
   * between them -- reported exactly that way. The seam is the channel between these note
   * edges, and drawing them there is what makes it visible.
   *
   * Back through the same conversion the placement used, at each note's OWN radius: the seam
   * is a constant width, so both its angle and the fraction a given angle corresponds to
   * depend on where the note sits. The dot's half-width goes in as an angle at that radius,
   * so the edge clears the ink rather than the centre.
   */
  function cellNoteFrac(c) {
    if (!renderer || !c || !c.ids || !c.ids.length) return null;
    var q0 = renderer.graphToViewport({ x: 0, y: 0 });
    var q1 = renderer.graphToViewport({ x: UNIT, y: 0 });
    var d0 = Math.hypot(q1.x - q0.x, q1.y - q0.y);
    var perPx = d0 > 1e-3 ? UNIT / d0 : 0;
    var lo = Infinity, hi = -Infinity;
    c.ids.forEach(function (id) {
      if ((alpha[id] || 0) < 0.5) return;
      var at = graph.getNodeAttributes(id);
      var rl = Math.hypot(at.x, at.y) / UNIT;
      if (!(rl > 1e-6)) return;
      var dd = renderer.getNodeDisplayData(id);
      if (!dd || dd.hidden) return;
      var sn = seamAt(rl * UNIT, c.nB, c.inner ? "i" : "o");
      if (!(sn.avail > 1e-9)) return;
      var f = (angleSweep(Math.atan2(at.y, at.x)) + sn.gap / 2 - sn.gap * c.seams) / sn.avail;
      var half = (renderer.scaleSize(dd.size) * perPx) / (rl * UNIT) / sn.avail;
      if (f - half < lo) lo = f - half;
      if (f + half > hi) hi = f + half;
    });
    return lo < hi ? { lo: lo, hi: hi } : null;
  }

  /**
   * The wedge envelope at one lattice radius, in the SCREEN angle convention (degrees, the
   * same one atan2 on a node's x/y gives). Groups are folded from their cells, so a folder
   * split into sub-wedges reports one envelope per sub-wedge run rather than a bounding box
   * across the whole band.
   *
   * `centre` is the midpoint between this wedge's two ANGULAR NEIGHBOURS' facing edges --
   * wedge plus both half-seams -- which is where a single-column note should sit.
   */
  function wedgeEdges(rLattice) {
    var cells = DBG.cells;
    if (!cells || !cells.length) return [];
    var out = [];
    ["i", "o"].forEach(function (bk) {
      var band = cells.filter(function (c) { return (c.inner ? "i" : "o") === bk; });
      if (!band.length) return;
      var r = rLattice || (geomLock
        ? (bk === "i" ? geomLock.r0 + (geomLock.rOuter - geomLock.r0) * INNER_FILL * 0.5
                      : (geomLock.rOuter + geomLock.maxR) / 2)
        : 1);
      var sm = seamAt(r * UNIT, band[0].nB, bk);
      // Straight through edgeSweep -- the placement's own expression. The pre-ray form stays
      // for a capture taken before geomLock existed, where there is no reference radius.
      var sw = function (c, which) {
        if (c.pLead !== undefined) return edgeSweep(c, which === "f0" ? "lead" : "trail", r * UNIT);
        return sm.gap * c.seams + sm.avail * c[which] - sm.gap / 2;
      };
      // Fold adjacent cells of the same group into one wedge, in sweep order.
      var runs = [];
      band.slice().sort(function (x, y) { return x.f0 - y.f0; }).forEach(function (c) {
        var last = runs[runs.length - 1];
        if (last && last.g === c.g) { last.b = c; return; }
        runs.push({ g: c.g, band: bk, a: c, b: c });
      });
      var noteFrac = function (run) {
        var lo = Infinity, hi = -Infinity;
        band.filter(function (c) { return c.g === run.g && c.f0 >= run.a.f0 && c.f1 <= run.b.f1; })
            .forEach(function (c) {
              var e = cellNoteFrac(c);
              if (!e) return;
              if (e.lo < lo) lo = e.lo;
              if (e.hi > hi) hi = e.hi;
            });
        return lo < hi ? { lo: lo, hi: hi } : null;
      };

      runs.forEach(function (run, i) {
        var prev = runs[(i - 1 + runs.length) % runs.length];
        var next = runs[(i + 1) % runs.length];
        var lo = sw(prev.b, "f1"), hi = sw(next.a, "f0");
        if (runs.length < 2) { lo = sw(run.a, "f0") - sm.gap; hi = sw(run.b, "f1") + sm.gap; }
        else { while (hi < lo) hi += 2 * Math.PI; }
        var deg = function (x) { return sweepAngle(x) * 180 / Math.PI; };
        var nf = noteFrac(run);
        out.push({ g: run.g, band: bk, r: r,
                   // The note-hugging edges, in the same fraction space as f0/f1 so the
                   // drawing can put its curve through them. Null when nothing is drawn.
                   nf0: nf ? nf.lo : null, nf1: nf ? nf.hi : null,
                   nStart: nf ? deg(sw({ seams: run.a.seams, f0: nf.lo }, "f0")) : null,
                   nEnd: nf ? deg(sw({ seams: run.a.seams, f0: nf.hi }, "f0")) : null,
                   // The raw terms too: a swing in `start` is either the seam count or the
                   // fraction before it, and the sum alone cannot say which.
                   seams: run.a.seams, f0: run.a.f0, f1: run.b.f1,
                   gap: sm.gap * 180 / Math.PI, avail: sm.avail * 180 / Math.PI,
                   start: deg(sw(run.a, "f0")), end: deg(sw(run.b, "f1")),
                   arc: (sw(run.b, "f1") - sw(run.a, "f0")) * 180 / Math.PI,
                   centre: deg((lo + hi) / 2) });
      });
    });
    return out;
  }

  /** Draw the overlay: every wedge edge as a curve across its band, plus the band radii. */
  function drawWedgeDebug() {
    var cv = DBG.canvas;
    if (!cv) return;
    // THE `hidden` ATTRIBUTE, not a display assignment. Same reason the tooltips in this
    // file use it: the Obsidian directory's linter rejects a static style assignment, and
    // `hidden` says what is meant -- see the [hidden] rule in page.css, which makes it stick
    // whatever else claims `display` on a canvas.
    if (!DBG.on || !renderer || !geomLock) { cv.hidden = true; return; }
    cv.hidden = false;
    var host = $("graph");
    var w = host.clientWidth, h = host.clientHeight, dpr = WIN.devicePixelRatio || 1;
    if (cv.width !== Math.round(w * dpr) || cv.height !== Math.round(h * dpr)) {
      cv.width = Math.round(w * dpr); cv.height = Math.round(h * dpr);
      cv.style.width = w + "px"; cv.style.height = h + "px";
    }
    var g2 = cv.getContext("2d");
    g2.setTransform(dpr, 0, 0, dpr, 0, 0);
    g2.clearRect(0, 0, w, h);
    // THE BANDS AS DRAWN, measured off the dots themselves -- outer edge of the outermost
    // note, inner edge of the innermost -- rather than off geomLock. geomLock's radii are the
    // solver's LATTICE bounds, taken before INNER_SCALE and before the row pitch decides where
    // a row actually lands, so all four circles missed: the outer ring's outer circle floated
    // clear of the notes, its inner circle ran through their middles, and both inner-ring
    // circles sat inside the ring. A debug ring that does not touch what it claims to bound is
    // worse than none -- it invites exactly the reasoning-instead-of-measuring this project
    // keeps paying for. Orphans are excluded: they live in the hub hole, not in a band.
    var perPx = (function () {
      var a0 = renderer.graphToViewport({ x: 0, y: 0 });
      var b0 = renderer.graphToViewport({ x: UNIT, y: 0 });
      var d0 = Math.hypot(b0.x - a0.x, b0.y - a0.y);
      return d0 > 1e-3 ? UNIT / d0 : 0;
    })();
    var seen = { i: null, o: null };
    graph.forEachNode(function (id, a) {
      if ((alpha[id] || 0) < 0.5 || isOrphan(id)) return;
      var dd = renderer.getNodeDisplayData(id);
      if (!dd || dd.hidden) return;
      var rl = Math.hypot(a.x, a.y) / UNIT;
      var dot = renderer.scaleSize(dd.size) * perPx / UNIT;
      var k = bandLock && bandLock[groupOf(id)] ? "i" : "o";
      var bb = seen[k] || (seen[k] = { lo: Infinity, hi: -Infinity });
      if (rl - dot < bb.lo) bb.lo = rl - dot;
      if (rl + dot > bb.hi) bb.hi = rl + dot;
    });
    var thickI = (geomLock.rOuter - geomLock.r0) * INNER_FILL;
    var bandR = { i: [geomLock.r0, geomLock.r0 + thickI], o: [geomLock.rOuter, geomLock.maxR] };
    ["i", "o"].forEach(function (k) {
      if (seen[k] && seen[k].lo < seen[k].hi) bandR[k] = [seen[k].lo, seen[k].hi];
    });
    var vp = function (rl, ang) {
      return renderer.graphToViewport({ x: rl * UNIT * Math.cos(ang), y: rl * UNIT * Math.sin(ang) });
    };
    // The folder's own colour at a given alpha. colorOf answers in whatever CSS form the
    // palette holds (hex or a colour function), so the alpha rides in globalAlpha rather
    // than being spliced into the string.
    var tint = function (g0, a) { return { c: colorOf(g0), a: a }; };
    // The band radii first, underneath: r0, the inner band's fill edge, rOuter, maxR.
    g2.lineWidth = 1;
    g2.strokeStyle = SEAM_YELLOW_45;
    g2.lineWidth = 2;
    g2.setLineDash([4, 4]);
    [bandR.i[0], bandR.i[1], bandR.o[0], bandR.o[1]].forEach(function (rl) {
      g2.beginPath();
      for (var i = 0; i <= 96; i++) {
        var q = vp(rl, i / 96 * 2 * Math.PI);
        if (i) g2.lineTo(q.x, q.y); else g2.moveTo(q.x, q.y);
      }
      g2.stroke();
    });
    g2.setLineDash([]);
    var cells = DBG.cells || [];
    ["i", "o"].forEach(function (bk) {
      var band = cells.filter(function (c) { return (c.inner ? "i" : "o") === bk; });
      if (!band.length) return;
      var lo = bandR[bk][0], hi = bandR[bk][1];
      // A SEAM IS A PARALLEL-SIDED CHANNEL, and this draws it as one.
      //
      // The seam is a constant WIDTH, so its angle goes as 1/r -- that is the whole point of
      // seamAt, and it is what stops the channels between wedges opening out into slots as
      // they run outward (decision 0002). Drawing each edge as a curve of constant FRACTION
      // hid it: the two facing edges both splayed with radius and read as radial rays, with
      // nothing between them. So each boundary is now drawn as what it geometrically is --
      // a radial centre line, and one straight edge either side of it, PARALLEL to it, each
      // offset to the nearest ink on its own side. Parallel lines either side of a radius
      // are the picture of a constant-width channel; anything else here would be a lie about
      // the layout.
      var rMid = (lo + hi) / 2, loG = lo * UNIT, hiG = hi * UNIT;
      var fracOf = function (c, which) {
        var e = cellNoteFrac(c);
        return e ? (which === "f0" ? e.lo : e.hi) : c[which];
      };
      var angAt = function (c, which, rl) {
        var sm = seamAt(rl * UNIT, c.nB, c.inner ? "i" : "o");
        return sweepAngle(sm.gap * c.seams + sm.avail * fracOf(c, which) - sm.gap / 2);
      };
      // Every drawn dot of a cell, in graph units, with the radius of its ink.
      var inkOf = function (c) {
        var out = [];
        var q0 = renderer.graphToViewport({ x: 0, y: 0 });
        var q1 = renderer.graphToViewport({ x: UNIT, y: 0 });
        var d0 = Math.hypot(q1.x - q0.x, q1.y - q0.y);
        var perPx = d0 > 1e-3 ? UNIT / d0 : 0;
        (c.ids || []).forEach(function (id) {
          if ((alpha[id] || 0) < 0.5) return;
          var dd = renderer.getNodeDisplayData(id);
          if (!dd || dd.hidden) return;
          var at = graph.getNodeAttributes(id);
          out.push({ x: at.x, y: at.y, rad: renderer.scaleSize(dd.size) * perPx });
        });
        return out;
      };

      // EACH WEDGE IN ITS OWN FOLDER'S COLOUR, so an edge is attributable at a glance --
      // with twelve wedges in a band, one accent colour for all of them says a boundary
      // moved without saying whose. Sub-wedge boundaries are the same hue, drawn faint:
      // the two are different things, and every measured wedge-jump bug so far has been
      // about which of them moved.

      // ONE CENTRE LINE PER FOLDER, not per cell. Drawn per cell, a folder split into
      // sub-wedges showed one dashed line per sub-wedge -- 05 has two, 04 has three -- and
      // beside a neighbour's single line they read as that neighbour having two centres.
      // Reported as monthly having two, when the extra lines were its neighbours': 05's second
      // sub-wedge, and yearly's own line running the full height of the band while its single
      // note sits in one row. A folder is one wedge to the eye, so it gets one middle.
      //
      // EVALUATED AT BOTH ENDS OF THE BAND AND JOINED. A wedge boundary is a straight line but
      // not a radial one: the seam is a constant width, so its angle goes as W/r and a boundary
      // is theta = theta0 + d/r; the midline between two of them is another straight line with
      // its own offset. Taking the centre angle once in the middle and drawing a RADIUS through
      // it put 06's three notes 0.08, 0.32 and 0.73 degrees off their own centre. Two points,
      // one line: every note of every single-column wedge now measures 0 units off it.
      (function () {
        var runs = [];
        band.slice().sort(function (x, y) { return x.f0 - y.f0; }).forEach(function (c0) {
          var last = runs[runs.length - 1];
          if (last && last.g === c0.g) { last.cells.push(c0); return; }
          runs.push({ g: c0.g, cells: [c0] });
        });
        runs.forEach(function (run) {
          var a0c = run.cells[0], b0c = run.cells[run.cells.length - 1];
          var fMid = (a0c.f0 + b0c.f1) / 2;
          var host0 = a0c;
          run.cells.forEach(function (c0) { if (c0.f0 <= fMid && fMid <= c0.f1) host0 = c0; });
          void b0c;
          // The run's own middle: halfway between its first cell's leading boundary and its
          // last cell's trailing one, both from edgeSweep. Nothing reconstructed.
          var mid = function (rl) {
            if (a0c.pLead !== undefined) {
              return sweepAngle((edgeSweep(a0c, "lead", rl * UNIT)
                                 + edgeSweep(b0c, "trail", rl * UNIT)) / 2);
            }
            var sm0 = seamAt(rl * UNIT, host0.nB, host0.inner ? "i" : "o");
            return sweepAngle(sm0.gap * host0.seams + sm0.avail * fMid - sm0.gap / 2);
          };
          var pts = [];
          for (var qq = 0; qq <= 24; qq++) {
            var rq = lo + (hi - lo) * qq / 24;
            pts.push(vp(rq, mid(rq)));
          }
          // WHITE, not the folder's colour. A narrow wedge is about 15 screen pixels wide at
          // this vault's inner band -- measured off the overlay canvas along one scanline:
          // 07's leading edge, centre and trailing edge landed at x=630, 637.5 and 644.5 with
          // its note at 638. Three lines of one hue inside 15 pixels read as two, which is
          // what "there are two lines in yearly" was looking at. Colour tells them apart where
          // position cannot: folder hues for edges, yellow for the seam, white for the middle.
          g2.strokeStyle = "#fff"; g2.globalAlpha = 0.6; g2.lineWidth = 2;
          g2.setLineDash([5, 5]);
          g2.beginPath();
          pts.forEach(function (pt, qq) { if (qq) g2.lineTo(pt.x, pt.y); else g2.moveTo(pt.x, pt.y); });
          g2.stroke();
          g2.setLineDash([]); g2.globalAlpha = 1;
        });
      })();
      var sorted = band.slice().sort(function (x, y) { return x.f0 - y.f0; });
      sorted.forEach(function (c, i) {
        var next = sorted[(i + 1) % sorted.length];
        if (next === c) return;
        // EVERY LINE IS ITS OWN CURVE, EVALUATED AT BOTH ENDS OF THE BAND AND JOINED.
        //
        // A boundary is the straight line theta = theta0 + d/r: same theta0 for both sides of a
        // seam (they share a wedge fraction), d differing by exactly one seam width -- so the
        // two are parallel and their midline is parallel to both. That much was right.
        //
        // What was wrong was mixing two approximations. The edges were built as perpendicular
        // OFFSETS from a ray through the origin at the angle the boundary has at the band's
        // middle: exact at that radius and drifting from the true boundary at every other,
        // while the wedge's centre line was already a chord between its true angles at the
        // band's two ends. Two different approximations of parallel lines do not stay
        // equidistant, so the centre stopped looking centred towards the inner and outer rows --
        // reported on 06, whose measured centre-to-seam arcs are equal to the unit at every row.
        //
        // One construction for all three: take the angle at lo, take it at hi, join. The lines
        // are chords of the curves they represent, they agree with each other everywhere, and
        // the seam's centre is the midpoint of its two edges by arithmetic at both ends.
        var angOf = function (cell, which, rl) {
          if (cell.pLead !== undefined) {
            return edgeSweep(cell, which === "f0" ? "lead" : "trail", rl * UNIT);
          }
          var sm0 = seamAt(rl * UNIT, cell.nB, cell.inner ? "i" : "o");
          return sm0.gap * cell.seams + sm0.avail * cell[which] - sm0.gap / 2;
        };
        // SAMPLED, NOT A CHORD. A boundary was taken for a straight line, on the grounds that
        // theta0 + d/r is the small-angle form of one. For the seam WIDTH that holds -- d is 58
        // units against a radius of hundreds -- but a boundary's own d is (seams - 0.5) * W,
        // which reaches 493 units at the eighth seam of this vault's inner band: d/r = 0.73,
        // nothing small about it. A line through the curve's two endpoints then cuts several
        // pixels inside it in the middle rows, by a different amount for each of the three
        // lines, which is why 06's centre stopped looking centred while its measured
        // centre-to-seam arcs are equal to the unit at every row. Sampling the same formula the
        // notes are placed with cannot disagree with where they ended up.
        // THE INNER BAND'S SEAM LINES RUN ALL THE WAY IN, to a whisker off the hub's centre.
        // A seam is meant to read as radial, and whether it does is a question about where the
        // lines POINT -- which is only answerable if they are long enough to see converge. They
        // do not converge on a point, and that is the geometry rather than a defect: a boundary
        // is theta = theta0 + d/r, so it swings as the radius falls away, and near the hub the
        // swing is all there is. seamAt's own cap (SEAM_CAP, 45% of the circle) is what keeps
        // it finite. Drawing it makes both facts visible at once.
        var chord = function (fn, style, width, dash, tag, rFrom) {
          if (DBG.trace) DBG.trace.push({ tag: tag || "?", c: c.k, next: next.k,
                                          deg: sweepAngle(fn(DBG.traceR)) * 180 / Math.PI });
          g2.strokeStyle = style.c; g2.globalAlpha = style.a; g2.lineWidth = width;
          if (dash) g2.setLineDash(dash);
          var r0c = rFrom !== undefined ? rFrom : lo;
          g2.beginPath();
          for (var q = 0; q <= 48; q++) {
            var rl = r0c + (hi - r0c) * q / 48;
            var pt = vp(rl, sweepAngle(fn(rl)));
            if (q) g2.lineTo(pt.x, pt.y); else g2.moveTo(pt.x, pt.y);
          }
          g2.stroke();
          if (dash) g2.setLineDash([]);
          g2.globalAlpha = 1;
        };
        var sweepA = function (rl) { return angOf(c, "f1", rl); };
        var sweepB = function (rl) {
          var a = angOf(next, "f0", rl), b = sweepA(rl);
          while (a < b) a += 2 * Math.PI;            // the wrap: B follows A in sweep
          while (a - b > Math.PI) a -= 2 * Math.PI;
          return a;
        };
        var groupBoundary = c.g !== next.g;
        // THE SEAM CENTRE IS DRAWN STRAIGHT AND LONG, not bent inward along its own curve.
        // The question it answers is whether a seam POINTS at the centre of the disc, and the
        // way to answer that is to take the line it actually is across the band and carry it on
        // in a straight line far enough to pass the hub. Sampling the 1/r curve inward instead
        // bends it, which shows the swing but hides the aim; this shows the aim, and by how much
        // each one misses. They are not all radial: every boundary has its own perpendicular
        // offset d = (seams - 0.5) * W, so each misses the centre by its own d.
        (function () {
          var mid = function (rl) { return sweepAngle((sweepA(rl) + sweepB(rl)) / 2); };
          var pOut = { x: hi * UNIT * Math.cos(mid(hi)), y: hi * UNIT * Math.sin(mid(hi)) };
          var pIn = { x: lo * UNIT * Math.cos(mid(lo)), y: lo * UNIT * Math.sin(mid(lo)) };
          var dx = pIn.x - pOut.x, dy = pIn.y - pOut.y, L = Math.hypot(dx, dy);
          if (!(L > 1e-6)) return;
          var reach = Math.hypot(pIn.x, pIn.y);          // enough to carry it past the hub
          var pEnd = { x: pIn.x + dx / L * reach, y: pIn.y + dy / L * reach };
          var q0 = renderer.graphToViewport(pOut), q1 = renderer.graphToViewport(pEnd);
          g2.strokeStyle = SEAM_YELLOW;
          g2.globalAlpha = groupBoundary ? 0.75 : 0.45;
          g2.lineWidth = 2;
          g2.setLineDash([3, 4]);
          g2.beginPath(); g2.moveTo(q0.x, q0.y); g2.lineTo(q1.x, q1.y); g2.stroke();
          g2.setLineDash([]); g2.globalAlpha = 1;
        })();
        chord(sweepA, tint(c.g, groupBoundary ? 0.9 : 0.35), groupBoundary ? 3 : 2,
              null, c.g + " trailing");
        chord(sweepB, tint(next.g, groupBoundary ? 0.9 : 0.35), groupBoundary ? 3 : 2,
              null, next.g + " leading");
        // NO INK MARK. Where a wedge's notes stop short of its boundary was drawn as a short
        // dotted tick per row, and it read as a second centre line: two dashed things inside
        // one wedge, one of them the centre and one of them not. The shortfall is measurable
        // (scratchpad margins2) and does not need to be on the disc while the disc is being
        // judged by eye. Two solid edges and one dashed centre, nothing else.
        // (the folder's centre line is drawn once per FOLDER, after this loop)
      });
      // NO ENVELOPE-CENTRE LINE. The first version of this overlay drew one -- the midpoint
      // between a wedge's two NEIGHBOURS' facing edges, which is a different line from the
      // wedge's own middle and sits about a pixel from it on a narrow wedge. It survived every
      // later rewrite because each patch anchored somewhere else, and it is the "salmon dotted
      // line in the yearly wedge, not in the centre or anything" that took four rounds to find:
      // a third line in the folder's own colour, one pixel off the middle, at 15 pixels of
      // wedge. wedgeEdges() still reports  for the probes that measure against it.
    });
    drawWedgeLegend(g2);
  }

  /**
   * A LEGEND ON THE OVERLAY, and the build it belongs to.
   *
   * Four kinds of line are on this canvas and they have been renamed, recoloured and moved
   * several times in one morning -- so the page now says what its own lines mean, and stamps
   * the build it came from. Half the confusion in that morning was two Chrome windows open on
   * two different builds, which no amount of zooming can tell apart.
   */
  function drawWedgeLegend(g2) {
    var rows = [
      ["solid, folder colour", "wedge edge"],
      ["dashed white", "wedge centre"],
      ["dotted yellow", "seam centre"],
      ["dashed yellow", "band radius"]
    ];
    var pad = 8, lh = 16, sw = 34, x = 12, y = 12;
    g2.font = "11px ui-monospace, monospace";
    g2.textBaseline = "middle";
    var wide = 0;
    rows.forEach(function (r) { wide = Math.max(wide, g2.measureText(r[1]).width); });
    var w = sw + 8 + wide + pad * 2, h = lh * (rows.length + 1) + pad * 2;
    g2.globalAlpha = 0.72; g2.fillStyle = "#000";
    g2.fillRect(x, y, w, h);
    g2.globalAlpha = 1;
    rows.forEach(function (r, i) {
      var yy = y + pad + lh * i + lh / 2;
      g2.strokeStyle = i === 0 ? "#e66767" : i === 1 ? "#fff" : SEAM_YELLOW;
      g2.globalAlpha = i === 0 ? 0.9 : i === 1 ? 0.5 : i === 2 ? 0.75 : 0.45;
      g2.lineWidth = i === 0 ? 1.5 : 1;
      g2.setLineDash(i === 0 ? [] : i === 1 ? [5, 5] : i === 2 ? [3, 4] : [4, 4]);
      g2.beginPath(); g2.moveTo(x + pad, yy); g2.lineTo(x + pad + sw, yy); g2.stroke();
      g2.setLineDash([]);
      g2.globalAlpha = 0.85; g2.fillStyle = "#fff";
      g2.fillText(r[1], x + pad + sw + 8, yy);
    });
    g2.globalAlpha = 0.55; g2.fillStyle = "#fff";
    g2.fillText("built " + (DATA && DATA.generated ? DATA.generated : "?"),
                x + pad, y + pad + lh * rows.length + lh / 2);
    g2.globalAlpha = 1;
  }

  /** Turn the overlay on or off; creates its canvas on first use. */
  function wedgeDebug(v) {
    DBG.on = !!v;
    if (DBG.on && !DBG.canvas) {
      var host = $("graph");
      if (host) {
        var cv = DOC.createElement("canvas");
        // The box is the class's job now; see .vg-wedge-debug in page.css. It was a cssText
        // assignment, which is the one form obsidianmd/no-static-styles-assignment rejects
        // unconditionally -- and it was static, so the rule was right.
        cv.className = "vg-wedge-debug";
        host.appendChild(cv);
        DBG.canvas = cv;
      }
    }
    if (!DBG.on) { DBG.cells = null; if (DBG.canvas) DBG.canvas.hidden = true; }
    if (renderer) renderer.refresh({ skipIndexation: true });
    return DBG.on;
  }

  // Set (or clear) the hovered legend row. afterRender picks the change up through
  // hlSignature and ramps it like any other highlight source.
  //
  // A FULL refresh, NOT `{ skipIndexation: true }`. That flag says "nothing moved, so do
  // not rebuild the spatial index", which is true inside hlWalk's own loop and is NOT true
  // here: this fires whenever the pointer crosses a legend row, which can be at any point
  // during a cascade or a layout tween, when nodes very much have moved. Skipping
  // indexation then leaves Sigma's quadtree describing where the disc used to be.
  //
  // Measured, when this said skipIndexation: the suite went from 17/17 to 9/17 on the demo
  // vault -- aiming at a note resolved nothing ("element at aim CANVAS.sigma-mouse"), the
  // legend reported itself folded while showing 18 subfolder rows, and buildWedgePlan came
  // back null inside the hidden-folder sweep. Three unrelated-looking failures, one stale
  // index. Hover is a per-row event, not a per-frame one, so the re-index costs nothing
  // worth having.
  // `keys` is an array of path keys, or null. Compared as a joined string rather than by
  // identity so that re-entering the same row does not repaint.
  function hoverHighlight(group, keys) {
    group = group || null;
    var next = Object.create(null);
    (keys || []).forEach(function (k) { if (k) next[k] = true; });
    var a = Object.keys(state.hoverSub).sort().join(","),
        b = Object.keys(next).sort().join(",");
    if (state.hoverGroup === group && a === b) return;
    state.hoverGroup = group;
    state.hoverSub = next;
    if (renderer) renderer.refresh();
  }

  // Does this subfolder own a contiguous wedge of its own? Cells are keyed by TINT
  // SLOT, and everything past the third-largest shares the last slot -- so the
  // "N smaller subfolders" are one cell between them, not one cell each.
  //
  // Only a sub with its OWN slot owns a wedge, and only those are pushed. A pooled
  // one's notes are interleaved with its cell-mates at the same angles, so pushing it
  // slides a subset out THROUGH them: the highlight meant to make the selection
  // legible is what creates the overlaps. `03 - Resources/Locations` is the case that
  // settled it -- 3 notes, seventh in the order, sharing the tail slot with six other
  // folders.
  //
  // This was briefly reversed on the argument that a tail folder is still a level-1
  // subfolder, so selecting one doing nothing looked inconsistent in the nav. The
  // overlaps are the worse of the two, and the ring already identifies a pooled
  // selection perfectly well.
  //
  // The exception is real: if the tail slot has exactly ONE occupant, it is that
  // folder's own wedge and it moves like a named one.
  function ownsWedge(folder, sub) {
    var subs = subOrder[folder] || [];
    var k = subs.indexOf(sub || "");
    if (k < 0) return false;
    return k < SUB_NAMED || subs.length === SUB_NAMED + 1;
  }

  // Pushed out radially. Anything that owns a contiguous wedge can move as a block:
  // a group, or a named subfolder whose sub-wedge is its own. What must NOT move is
  // one of the pooled "smaller subfolders", because its notes are interleaved
  // through a wedge shared with the others at the same angles -- so pushing it
  // slides a subset out THROUGH its own cell-mates, and the push meant to make the
  // selection legible is what creates the overlaps. Those are identified by the
  // ring alone.
  function isPushed(id) {
    // NOT a marked day. Its notes are scattered across every folder, so pushing them slides
    // a subset out through their own cell-mates at the same angles -- the same reason a
    // pooled subfolder does not push. It is still haloed and recoloured; see isHighlighted
    // and nodeStyle.
    if (state.highlight[groupOf(id)]) return true;
    // Only the DEPTH-1 folder owns a wedge, so only it can move. A folder nested
    // deeper is a slice of its parent's arc, interleaved with its siblings at the same
    // angles -- pushing it would slide a subset out through them, which is the same
    // reason a pooled subfolder does not move.
    var a = graph.getNodeAttributes(id);
    return !!state.highlightSub[pathKey(a, 1)] && ownsWedge(a.folder, a.sub || "");
  }

  // How many notes' worth of ramp a note gets as the cutoff passes it. Ranks, not
  // days, for the same reason the reveal order is: it keeps the fade even whether the
  // vault gained one note that month or two hundred.
  /**
   * Does this note survive EVERY filter, not just the legend's?
   *
   * `visible()` answers one question -- is its group and its subfolder chain unhidden -- and
   * that was the whole answer for as long as it was the only filter. The timeline cutoff and
   * the date range both live in timeFactor instead, multiplied into opacity, which is what
   * lets them animate without a clause in visible().
   *
   * THE CASCADE PLANS WITH THIS, NOT WITH visible(). Its destination packing has to be the
   * one settle() will actually assign, and settle assigns whatever the packer derives from
   * the SETTLED OPACITIES -- so a note the date range excludes has no seat there. Planning
   * with visible() gave it one, so the animation walked toward a disc that still seated the
   * excluded notes and then re-densified in a single frame at the end.
   *
   * That is the jump, and it only appeared when a change REDUCED the note count: adding notes
   * back means the seats planB reserved are the seats they end up in, so the two agreed by
   * luck in that direction.
   *
   * The epsilon matches present(): a note part-way through a timeline ramp is on screen and
   * belongs in the packing, and only one at rest at zero does not.
   */
  function willShow(id) { return visible(id) && timeFactor(id) > 0.004; }

  var TL_FADE = 8;
  function timeFactor(id) {
    // THE DATE CAP FIRST, because it is a hard bound and the rank cutoff is a ramp: a note
    // outside the range is out whatever the rank cutoff says, and multiplying a ramp by zero
    // would give the same answer more slowly.
    //
    // UNDATED NOTES STAY, which is the same rule the rank cutoff already applies one line
    // down, and it matters more here than it looks: 20% of the notes in the 10-year fixture
    // carry no frontmatter at all. Dropping them from every range would mean a range filter
    // that also silently filters by "has frontmatter", which is not a date question.
    if (state.from !== null || state.to !== null) {
      var ms = tlMs[id];
      if (ms !== undefined) {
        if (state.from !== null && ms < state.from) return 0;
        if (state.to !== null && ms > state.to) return 0;
      }
    }
    if (state.until === null) return 1;
    var rk = tlRank[id];
    if (!rk) return 1;                     // undated notes are always present
    var f = (state.until - rk + 1) / TL_FADE;
    return f <= 0 ? 0 : f >= 1 ? 1 : f;
  }

  /* --------------------------------------------------------- reveal cascade */

  // Every note owns an opacity, and opacity is what the renderer reads -- never
  // visible() directly. A filter change therefore never pops anything in or out:
  // it retargets opacities and the cascade walks them there, one note at a time.
  var alpha = Object.create(null);           // id -> 0..1, the source of truth
  function present(id) { return (alpha[id] || 0) > 0.004; }
  // Opacity is filter AND timeline: the cutoff needs no clause in visible(), it
  // simply holds later notes at zero, and every density decision downstream is
  // already driven by these weights.
  function syncAlpha() {
    graph.forEachNode(function (id) { alpha[id] = visible(id) ? timeFactor(id) : 0; });
  }
  function clearAlpha() { graph.forEachNode(function (id) { alpha[id] = 0; }); }

  // Sigma's parseColor accepts rgba(r,g,b,a), but its channel regex is [0-9]*, so
  // the channels must be INTEGERS -- a CSS4 "rgb(0 0 0 / 50%)" silently fails to
  // parse and the node renders black. Hex in, integer rgba out.
  var rgbCache = Object.create(null);
  function toRgb(hex) {
    var c = rgbCache[hex];
    if (c) return c;
    var h = String(hex).trim();
    if (h.charAt(0) === "#") {
      if (h.length === 4) h = "#" + h.charAt(1) + h.charAt(1) + h.charAt(2) + h.charAt(2) + h.charAt(3) + h.charAt(3);
      c = [parseInt(h.slice(1, 3), 16), parseInt(h.slice(3, 5), 16), parseInt(h.slice(5, 7), 16)];
    } else {
      var m = /(\d+)\D+(\d+)\D+(\d+)/.exec(h);
      c = m ? [+m[1], +m[2], +m[3]] : [128, 128, 128];
    }
    return (rgbCache[hex] = c);
  }
  function withAlpha(color, a) {
    if (a >= 0.999) return color;
    var c = toRgb(color);
    return "rgba(" + c[0] + "," + c[1] + "," + c[2] + "," + a.toFixed(3) + ")";
  }

  // Frames one note takes to arrive, and the window the whole set is staggered
  // across. Frame-counted for the same reason the position tween is: a throttled
  // rAF must not leave the cascade stranded half-faded.
  var FADE_FRAMES = 12;
  // How far a note closes the gap to its target RADIUS each frame. This existed to
  // smooth row-count TICKS, and there are none left to smooth: the row coordinate
  // is continuous now. At 0.3 it only closed 30% of the gap per frame, so while
  // the target kept moving the position ran permanently behind -- and the leftover
  // lag was closed in one go when the animation stopped. Measured on the big
  // toggle, the disc contracted 2133 -> 1731 over 48 frames and then 1731 -> 1653
  // in the last one: the jump at the end of every animation. 1 means follow the
  // target exactly, so the animation simply ends where it already is.
  // ...and 1 was wrong, because the premise above is false: `Math.floor(pp)` makes a
  // note's radius DISCRETE, so a cell's fractional row count crossing an integer
  // teleports its outermost note a full pitch. There were ticks to smooth all along.
  // Measured on the `04 - Daily Notes` hide, worst single-frame step of the outer band:
  //
  //   ease 1 -> 160 units (one whole row)   ease 0.35 -> 56
  //   ease 0.5 -> 80                        ease 0.25 -> 40
  //
  // and the reason 1 was chosen -- the leftover lag closing in one go at the end -- does
  // not happen at any of them: final-frame delta is 0 throughout, and the resting radius
  // is 2138 in every case. That failure was measured over 48 frames; a toggle now runs
  // ~123, which is ample to converge.
  //
  // Unlike taking the radius from the continuous coordinate (tried and reverted the same
  // day), easing only moves notes whose TARGET changed, so the disc stays on its lattice
  // and just the crossing note glides.
  var RADIAL_EASE = 0.25;
  var SPREAD_MAX  = 78;      // longest stagger window, frames
  var SPREAD_PER  = 0.17;    // frames of stagger per note...
  // The floor matters more than it looks. A folder's wedge closes over the whole
  // stagger window, so a SHORT window on a BIG radius means high angular speed:
  // 04 - Daily Notes is 55 notes, which earned a 9.4-frame window, and its wedge
  // in the outer band swung ~6 degrees per frame -- about 210 units of arc for its
  // outermost notes, which reads as jumpy even though nothing is discontinuous.
  // 08 was smooth because 221 notes buy a 37-frame window, and 05 because the
  // inner band's radius makes the same angle a much shorter arc.
  var SPREAD_MIN  = 24;      // ...with a floor, so a mid-sized folder is not rushed
  // Stretches every frame count by this factor, for watching the animation in slow
  // motion. 1 is normal speed. Live: __vg.timeScale = 4 (no rebuild needed), which
  // is how the whole-disc snap at the end of a cascade was finally pinned down.
  // 1.25 = runs at 0.8x speed. Full speed read a shade too quick.
  var TIME_SCALE  = 1.25;
  // WALL-CLOCK, not frame counts. Every animation below runs for the same length of
  // time on every machine. Frame counting made duration a function of refresh rate:
  // the intro was 420 frames, so ~8.7s at 60Hz and ~3.6s at 144Hz on the same vault,
  // and a busy page stretched it further still. Progress is now elapsed/duration, so
  // a slow page draws FEWER intermediate frames instead of running longer.
  //
  // The original reason for frame counting still stands and is still handled: a
  // throttled or stopped rAF must never leave the disc smeared between two layouts.
  // Each animation keeps a timer that force-completes it -- the cascade's stall
  // watchdog, a hard deadline on the other two -- so the final layout is correct even
  // if no further frame ever arrives. That is what makes wall-clock safe here; the
  // earlier wall-clock attempt was reverted because it had no such backstop.
  //
  // TIME_SCALE still multiplies these, so __vg.timeScale = 4 remains a slow-motion
  // knob rather than a no-op.
  var TIMELINE_MS = 4500;   // intro / Refresh -- the vault growing from first note to now
  var CASCADE_MS  = 1600;   // a wedge arriving or leaving, and first paint
  var TWEEN_MS    = 380;    // a plain reflow, no fade
  var NOW = function () { return (window.performance || Date).now(); };
  // Two ways a cascade can fill the ring, and the difference is whether there is
  // already a ring to respect.
  //
  //   trade (fullRing = true)  -- something is on screen. The circle stays FULL
  //     and the wedges trade space: an arriving wedge grows while every other one
  //     shrinks to let it in, and a leaving wedge shrinks while the rest grow
  //     back. Enabling and disabling are exact mirrors of each other, and the
  //     disc only ever changes density, never its outline.
  //
  //   draw (fullRing = false) -- the screen is empty, so there is nothing to
  //     trade with and no ring to keep full. The pie draws itself clockwise from
  //     12 o'clock instead, the occupied arc growing to a full circle. This is
  //     first paint, and "none" then "all".
  var fullRing = false;
  var planKeep = null;       // during a cascade: visible-or-still-on-screen
  var cascadeRun = null;     // in-flight cascade
  var pinnedPlan = null;     // plan held still for the duration of one cascade
  var planMs = 0;            // cost of the last plan build, for measurement
  var lastGapN = { i: 0, o: 0 };   // continuous group count per band, behind the gap total
  // The RENDERED allocation's own numbers, which are the ones that place wedges: the
  // continuous group count it reserved for, and the gap angle it spent. lastGapN above
  // comes from the REFERENCE allocation, which sizes rows and never positions anything --
  // so it can look perfectly smooth while the arc the notes actually sit in jumps.
  var lastStart = null;   // group -> wedge start angle in degrees, this frame
  /** group -> RENDERED wedge arc in degrees, this frame -- the sum of the group's cells'
   *  c.span, which is the allocation the disc is actually drawn from. The probe needs it
   *  because every proxy tried so far lied: the span of the group's own notes reads ~0 for a
   *  single column (its notes share one angle), and lastStart is scaled at the reference
   *  radius. "The wedge should close at constant speed" is a claim about THIS number. */
  var lastArc = null;
  /** group -> "i"|"o", captured with lastArc: neighbour arithmetic on wedge starts is only
   *  meaningful within one band -- the two bands are separate circles, and a metric that
   *  sorted both into one ring reported a column 100 degrees off its own envelope centre. */
  var lastBand = null;
  // Group presences for the GAP reservation, walked between the cascade's two packings
  // and read by the rendered allocation. Null at rest, which is when weight-over-seats
  // is already the right answer -- see allocateBand.
  // How deep the LAST plan reached, in lattice units -- the same measure geomLock.maxR
  // holds for the full vault, so the two divide cleanly. Taken from the plan rather than
  // from node positions: the outermost note sits a little inside the lattice radius it was
  // packed against, so measuring the live extent made a full, unfiltered disc read as 96%
  // of itself and fit() zoomed in slightly on nothing.
  var lastMaxR = 0;
  // HOW MUCH ROOM EACH NOTE ACTUALLY HAS, in graph units: the gap to its nearer neighbour
  // along its own row, as the last layout placed them. Keyed by id.
  //
  // The row pitch is a constant and the ALONG-row pitch is not: a wedge's end margins come out
  // of the same circumference its notes are spread across, so a band carrying many wedges
  // packs its rows tighter than one carrying few. Sizing dots against the row pitch alone drew
  // them too big for their real neighbours -- measured, 126 units of diameter against a
  // 120-unit step, which is dots touching.
  //
  // PER NOTE rather than one figure for the disc, because one figure has to be the tightest
  // spot anywhere and that shrank every dot on the disc to fit the worst cell on it: measured,
  // diameter over spacing fell from 0.79 to 0.24 disc-wide for the sake of a handful of notes.
  // A note's size is now a statement about its own neighbourhood.
  //
  // Measured rather than derived because the spacing depends on the margins, the seam, the
  // wedge count and the presence weights at once, and every attempt in this file to predict
  // one of those from the others has been wrong at least once.
  var dotFit = Object.create(null);
  // The room a BAND has, one figure each, being the tightest pair in it. See dotPx: size has to
  // be a monotone function of link weight, so it cannot carry a per-note term.
  // Per note, its own cell's step. Bounds a dot where its folder is packed tighter than the
  // band's tenth percentile; continuous, so it does not make dots breathe.
  var lastMinArc = 0;   // the angular wedge floor the last allocation applied, for the probe
  /**
   * The band room a cascade hands in, or null at rest.
   *
   * Room is a percentile over the spacing of the notes that are CURRENTLY placed, and it decides
   * dot size -- and, since a margin is a clearance plus the largest dot, it decides POSITIONS
   * too. Recomputed per frame it is a statistic over a set whose membership changes as notes
   * arrive and leave, so it steps, and every note in the band steps with it. Measured on one
   * folder toggle: single frames moving a steady note 717, 952, 1374, 1635 and 1522 units
   * against a median frame of 45.
   *
   * So it is INTERPOLATED between the two endpoint packings rather than derived, which is the
   * rule this file already follows for the spacing, the row count and the gap reservation. It is
   * the one quantity that must be interpolated rather than computed: everything else is a
   * function of the plan, and this is a function of the frame.
   */
  var roomNow = null;
  /**
   * The wedge-arc ramp for fully-toggled SINGLE-CELL groups, or null at rest: group -> a
   * factor walking 1 -> 0 (hide) or 0 -> 1 (show) on the cascade's own clock.
   *
   * "The wedge should close and open at constant speed" is a claim about the RENDERED ARC,
   * and measured on the 3-note inner folder the arc held up to 6.2 degrees above the straight
   * line at 70% of an 11.9-degree close, then crashed. The arc is max(proportional share,
   * min-arc floor) plus the seam, and each driver had its own curve: the share tracks the
   * staggered alpha sum (an S), the floor holds one full note-width until the LAST note fades
   * (min(1, weight)), and the seam presence is the group's max alpha (1 until the very end).
   * One ramp drives all three, and then max(P*ramp, F*ramp) = max(P, F)*ramp -- a straight
   * line by arithmetic, not by tuning.
   *
   * ONLY the arc-side quantities read this: the rendered geom, the floor, the seam presence.
   * Seats, rows, fade delays and dot sizes are untouched -- the reverted curtain (ef6e1c8)
   * and the reverted column machinery (2feff5b) are why nothing that moves a NOTE may be
   * driven by a group-level clock. Single-cell groups only: a note in a one-per-row column
   * sits at its wedge's centre, so a narrowing wedge cannot collide it, and multi-wedge
   * folders are explicitly out of scope until this is judged on the simple case.
   */
  var colWalk = null;
  /**
   * The two endpoint LAYOUTS of the running cascade, or null at rest.
   *
   * INTERPOLATE THE OUTPUT, NOT THE INPUT. The frame loop used to build a plan per frame from
   * interpolated inputs -- a fractional row count, a walked spacing, opacity weights -- and lay
   * that out. Every one of those inputs is continuous, and the layout of them is not: a row
   * count is turned into an integer row by Math.floor, so notes change row in steps. Easing the
   * radius turns each step into a short slide, which is invisible at a 200-unit pitch and is not
   * at a 384-unit one. Measured on one folder toggle, with rowsInner crossing 2.0 partway
   * through: single frames moving a steady note 703, 953, 1378, 1631 and 1515 units against a
   * median frame of 46.
   *
   * Two layouts exist instead, one per endpoint, each a proper packing with integer rows. A frame
   * is a point on the line between them. At ease 0 the disc IS the layout it was resting in, at
   * ease 1 it IS the one settle() assigns, and in between every note moves along its own path at
   * its own steady rate. Nothing is derived per frame, so nothing per frame can step.
   *
   * What it gives up is the property decision 0002 chose: an intermediate frame is no longer
   * itself a valid packing, because a note between two lattice rows sits between them. That
   * decision rejected taking the radius from a FRACTIONAL row coordinate, which puts a note at a
   * radius neither endpoint has and was measured as a smeared disc. This is not that: both ends
   * are exact, and the smear is a note travelling between two places it genuinely belongs.
   */
  var posSrc = null, posDst = null;
  var cellRoom = Object.create(null);
  /**
   * The per-cell room a cascade hands in, or null at rest.
   *
   * Band room is already walked between the two endpoint packings rather than measured per frame
   * -- see roomNow -- because it is a statistic over a moving set and a statistic over a moving
   * set steps. This is the same quantity one level down, and dotPx OVERRIDES the band figure with
   * it whenever a cell is tighter than its band:
   *
   *     if (mine !== undefined && mine > 1 && (!(room > 1) || mine < room)) room = mine;
   *
   * So walking only the band figure fixed only the notes that read the band figure. A small
   * folder is exactly the case where its own cell is the tighter of the two, so small folders
   * kept taking a per-frame measurement and kept stepping. Measured on this vault, worst
   * single-frame size change on a note at FULL opacity: toggling _ Claude (14 notes) 68.4%,
   * 02 - Areas (6 notes) 54.9%, 01 - Projects (5 notes) 39.3% -- against 9-11% for the
   * 138- and 212-note folders, which mostly read the band. Reported as small folders not
   * looking right while large ones did.
   */
  var cellNow = null;
  /**
   * The per-note edge cap a cascade hands in, or null at rest.
   *
   * Third and last of the quantities dotPx reads that are statistics over a moving set, after
   * the band room and the cell room. This one is a DISTANCE -- how far a note is from its own
   * wedge edge -- and it binds hardest where a wedge is narrowest, which is the smallest
   * folders. Walking the other two fixed every folder except the two smallest: 01 - Projects
   * (5 notes) stayed at 33.6% and 06 - Monthly Reviews (3 notes) at 23.7%, worst one-frame size
   * change on a note at full opacity, and in both the stepping note was inside the folder being
   * toggled. Same treatment, same clock.
   */
  var edgeNow = null;
  // Per note, its distance to the nearer edge of its own wedge, in graph units. A dot may not
  // exceed it, or it crosses into the seam.
  var edgeCap = Object.create(null);
  // WHAT THE LAST CASCADE WAS ASKED TO DO. Every question about a jump starts with
  // "did it animate at all, and over how many notes", and inferring that from frame
  // counts is guesswork -- an instant apply and a one-frame animation look identical
  // from outside.
  var lastCascade = { ins: 0, outs: 0, span: 0, path: "none", frames: 0, ms: 0 };

  // Hold the wedge plan still while a cascade runs. visible() is already at its
  // final value the moment a filter changes, so buildWedgePlan would return the
  // very same plan on all ~90 frames -- this just stops paying for it 90 times.
  function pinPlan() {
    var t0 = (window.performance || Date).now();
    var keep = planKeep || visible;
    var shownCount = 0;
    graph.forEachNode(function (id) { if (keep(id)) shownCount++; });
    pinnedPlan = buildWedgePlan(true);
    planMs = (window.performance || Date).now() - t0;
    return pinnedPlan;
  }

  // Reveal (or withdraw) notes one at a time, sweeping CLOCKWISE from 12 o'clock
  // in both directions. Each note's opacity counts toward its wedge's span while
  // it fades, so the wedge fans open by exactly one note's worth per arrival and
  // pushes its clockwise neighbour along -- the space is made by the note taking
  // it, and the motion runs along the circumference rather than out from the hub.
  /**
   * opts, all optional:
   *   fullRing  force the ring mode instead of observing it (the timeline is a density
   *             animation by construction, so for it this is a constant, not an observation)
   *   order     fn(id) -> number, the ARRIVAL ORDER, replacing the clockwise sweep
   *   spread    the arrival window in FRAMES, replacing windowFor(n). span is this plus
   *             FADE_FRAMES, so it sets how much of the animation a single note's fade takes.
   *   totalMs   the whole cascade's wall-clock length, replacing CASCADE_MS
   *   onFrame   fn(progress) per frame, for a caller that has UI to keep in step
   */
  function cascade(done, opts) {
    opts = opts || {};
    stopPlay();                            // a filter change interrupts playback
    if (anim) { WIN.cancelAnimationFrame(anim); anim = null; }
    if (animGuard) { WIN.clearTimeout(animGuard); animGuard = null; }
    if (cascadeRun) {
      WIN.cancelAnimationFrame(cascadeRun.raf);
      WIN.clearTimeout(cascadeRun.guard);
      cascadeRun = null;
    }

    // A null plan is legitimate: "none" hides every note, so there is no
    // geometry left to lay out. The fade still has to run, with positions simply
    // frozen where they are, or the disc would blink out instead of receding.
    // Trade space against whatever is already on screen; only an empty screen
    // draws the pie from nothing.
    fullRing = false;
    graph.forEachNode(function (id) { if (present(id)) fullRing = true; });
    // Observed above, overridden here. The timeline starts from an empty screen, so an honest
    // observation always says "draw mode" -- and draw mode exists for a screen with no ring to
    // trade against, which is the opposite of what growing a vault needs.
    if (opts.fullRing !== undefined) fullRing = !!opts.fullRing;

    // willShow, not visible: membership is "staying, or still fading out", and "staying" has
    // to mean staying under every filter. See willShow.
    planKeep = function (id) { return willShow(id) || present(id); };
    var plan = pinPlan();
    // Arrival order is CLOCKWISE round the circumference. Work out where every
    // note ends up on the FINISHED disc and sort by that sweep angle: notes that
    // share an angle (a column, inner band and main band alike) arrive together,
    // so the frontier reads as a hand sweeping round the clock.
    //
    // Ordering by angle has a second effect that ordering by radius did not:
    // because wedges fill clockwise and the ring is contiguous, everything
    // upstream of the frontier is already complete when a note lands. So a note
    // arrives at its final angle and never moves again -- the only thing that
    // moves is what is still downstream, and that is still invisible.
    // ONE PLAN. This layout is the destination, the tween's target, and what gets assigned at
    // the end -- the same object all three times, so there is nothing for them to disagree about.
    //
    // It used to be computed with the PINNED plan still in force, which made it the pinned
    // packing wearing the destination's opacities rather than the destination packing. settle()
    // then cleared the pin and built its own, and the two differed: measured on one folder
    // toggle, 15 notes moved up to 1517 units TANGENTIALLY at the hand-over while the band
    // extents, the gap angles and every wedge's start angle were identical -- notes seated in a
    // different order by two plans that were supposed to be the same one.
    //
    // So the pin and planKeep come off while it is computed. What comes out is what an unpinned
    // ringsLayout produces once the departing notes are gone, which is the definition settle()
    // was written against and never actually met.
    colWalk = null;   // the previous cascade's ramps die before the resting layouts below run
    var keep = Object.create(null);
    graph.forEachNode(function (id) { keep[id] = alpha[id] || 0; alpha[id] = visible(id) ? timeFactor(id) : 0; });
    var pinWas = pinnedPlan, keepWas = planKeep, roomWas = roomNow;
    pinnedPlan = null; planKeep = null; roomNow = null; cellNow = null; edgeNow = null;
    colWalk = null;
    // TWICE, and the first one is thrown away.
    //
    // Room and position are a fixed point: the margin at a wedge edge is a clearance plus a dot
    // radius, both scaled by the band's room, and room is measured FROM the positions -- so a
    // pass can only write the room it measured after it has already spent the margins. Each pass
    // therefore uses the previous pass's room. At rest that has converged, because frames have
    // been running. Computed once here it uses the SOURCE disc's room, which is the wrong
    // number: the destination has different spacing.
    //
    // Measured: after a toggle settled, forcing a fresh layout moved 70 notes by up to 39 units
    // on 03 - Resources, 04 - Daily Notes and 08 - Meeting Notes, with dot sizes identical to
    // the unit. Margins only, which is precisely the seam -- reported as the seam jumping at the
    // last frame. The second pass is the one that makes finalPos equal what a relayout produces,
    // so settle() assigns a state nothing can later disagree with.
    ringsLayout();
    var finalPos = ringsLayout() || {};
    pinnedPlan = pinWas; planKeep = keepWas; roomNow = roomWas;
    graph.forEachNode(function (id) { alpha[id] = keep[id]; });
    var sweepOf = Object.create(null);
    graph.forEachNode(function (id) {
      var q = finalPos[id];
      sweepOf[id] = q ? angleSweep(Math.atan2(q.y, q.x)) : 0;
    });

    var ins = [], outs = [], to = Object.create(null), from = Object.create(null);
    graph.forEachNode(function (id) {
      // timeFactor, not 1: a note the timeline or the date range excludes must not be
      // revealed by a filter change somewhere else. See the note on `keep` above.
      var want = visible(id) ? timeFactor(id) : 0;
      var now = alpha[id] || 0;
      if (Math.abs(now - want) <= 0.004) return;
      to[id] = want; from[id] = now;
      (want ? ins : outs).push(id);
    });
    if (!ins.length && !outs.length) {
      lastCascade = { ins: 0, outs: 0, span: 0, path: "instant: nothing to move", frames: 0, ms: 0 };
      pinnedPlan = null; roomNow = null; cellNow = null; edgeNow = null; posSrc = posDst = null; applyLayout(true); return;
    }

    // Clockwise in BOTH directions -- notes leave in the same sweep they arrived
    // in, so the animation never runs backwards.
    var clockwise = function (a, b) { return sweepOf[a] - sweepOf[b]; };
    // The arrival order, which is the ONLY thing the timeline needs to do differently: it
    // reveals the vault by DATE, and everything else reveals it clockwise. Same cascade either
    // way, so the timeline gets the row walk, the room walk, the plan-derived gap presence and
    // a settle that assigns the plan -- none of which it had while it ran its own frame loop.
    var rank = typeof opts.order === "function" ? opts.order : null;
    var arrival = rank ? function (a, b) { return rank(a) - rank(b); } : clockwise;
    ins.sort(arrival);
    outs.sort(arrival);

    var windowFor = function (n) {
      if (opts.spread > 0) return opts.spread;   // a caller with its own shape; see playTimeline
      return Math.max(SPREAD_MIN, Math.min(SPREAD_MAX, n * SPREAD_PER)) * TIME_SCALE;
    };
    var delay = Object.create(null);
    [ins, outs].forEach(function (set) {
      var w = windowFor(set.length);
      set.forEach(function (id, i) { delay[id] = set.length < 2 ? 0 : w * i / (set.length - 1); });
    });
    var span = Math.max(windowFor(ins.length), windowFor(outs.length))
             + FADE_FRAMES * TIME_SCALE;
    // WHICH GROUPS GET THE ARC RAMP: fully toggled (every present note leaving, or every
    // arriving note entering an empty group), on a legend toggle only. The single-cell gate
    // is applied where the endpoint plans are in scope, below.
    var tglDir = Object.create(null), tglN = Object.create(null);
    if (opts.colToggle) (function () {
      var startN = Object.create(null), outN = Object.create(null), inN = Object.create(null);
      graph.forEachNode(function (id) {
        if ((alpha[id] || 0) > 0.004) {
          var g0 = groupOf(id);
          startN[g0] = (startN[g0] || 0) + 1;
        }
      });
      outs.forEach(function (id) { var g0 = groupOf(id); outN[g0] = (outN[g0] || 0) + 1; });
      ins.forEach(function (id) { var g0 = groupOf(id); inN[g0] = (inN[g0] || 0) + 1; });
      Object.keys(outN).forEach(function (g0) {
        if (!inN[g0] && outN[g0] === (startN[g0] || 0)) { tglDir[g0] = "out"; tglN[g0] = outN[g0]; }
      });
      Object.keys(inN).forEach(function (g0) {
        if (!outN[g0] && !(startN[g0] || 0)) { tglDir[g0] = "in"; tglN[g0] = inN[g0]; }
      });
    })();

    var moving = ins.concat(outs);
    lastCascade = { ins: ins.length, outs: outs.length, span: Math.round(span * 100) / 100,
                    path: "animated", frames: 0, ms: 0, t0: NOW() };

    var settle = function () {
      if (!lastCascade.exit) lastCascade.exit = "settle() called from outside the loop";
      // Back to weight-over-seats: at rest the seats ARE the group's own notes, so the
      // derived reading is right and a stale override would freeze the gap at whatever
      // the last frame happened to hold.
      if (cascadeRun) {
        WIN.cancelAnimationFrame(cascadeRun.raf);
        WIN.clearTimeout(cascadeRun.guard);
        cascadeRun = null;
      }
      probeSample("pre-settle");
      moving.forEach(function (id) { alpha[id] = to[id]; });
      pinnedPlan = null;
      planKeep = null;
      roomNow = null; cellNow = null; edgeNow = null; posSrc = posDst = null;   // the walk is over; rest measures its own
      colWalk = null;
      // ASSIGN THE PLAN, do not compute another one. applyLayout(false) built a fresh plan
      // here, and a fresh plan is a second opinion: it agreed about the structure -- same band
      // extents, same gaps, same wedge starts -- and disagreed about which seat 15 notes got,
      // which is a 1517-unit tangential jump at the exact moment the animation hands over.
      //
      // finalPos was computed above as what an unpinned layout produces for the destination, so
      // assigning it is the same answer arrived at once instead of twice.
      assignPositions(finalPos);
      // REFRESH THE SIZE STATE TOO, not just position. dotPx() -- what the node reducer calls
      // for every dot, every frame -- reads bandOf().room, cellRoom and edgeCap, and none of
      // those are recomputed here on their own: they are side effects of whichever ringsLayout()
      // call last ran, and that was the FRAME LOOP's, walking roomNow/cellNow/edgeNow from the
      // source packing to the endpoint capture one frame at a time. Positions land correctly
      // because assignPositions above reads finalPos directly; size does not, because nothing
      // re-derives it against the settled alpha -- it is simply left holding whatever the last
      // animated frame's WALKED figure happened to be.
      //
      // github#21: measured up to 157% off on a range change (a note at 26.5px that should be
      // 68.1px), and up to 11.6% on a folder toggle -- both silent, because dr/dtan (position) are
      // exactly right, and every check that ever looked (including the "last frame of a cascade
      // is the resting layout" smoke check) compares the last animated frame against rest, which
      // read the identical stale globals and agreed with itself.
      //
      // The calls below are exactly what an unrelated relayout already does by accident (a
      // resize, a fit, the next toggle -- see __vg.relayout()), just run here instead of waiting
      // for one. roomNow/cellNow/edgeNow are already null (cleared above) and pinnedPlan/planKeep
      // are already null too, so this reads live alpha -- the SAME alpha finalPos was computed
      // against -- and lands on the same plan deterministically. The POSITION output is discarded
      // on purpose: reassigning position from a second call is the exact mistake the comment
      // above this one already fixed once (a 1517-unit jump from two plans disagreeing on seats).
      // Only the room/cellRoom/edgeCap side effects are wanted here.
      //
      // TWICE, same reason finalPos itself is computed twice above: room and position are a
      // fixed point, and a single call still has this frame's MARGINS measured from whatever
      // room the frame loop last left behind -- the very staleness this refresh exists to
      // remove. Measured after adding the first call alone: still 58 of 213 notes off by more
      // than 5%, worst 39.1%, and a second immediate call converged every one of them to 0 --
      // stable under a third call too, confirming it is a fixed point and not still drifting.
      ringsLayout();
      ringsLayout();
      renderer.refresh({ skipIndexation: false });
      probeSample("settled");
      if (done) done();
    };

    // TWO packings, interpolated across the fade rather than tweened before and
    // after it. Crossing REPACK_BELOW re-densifies the disc -- every surviving
    // note genuinely belongs in a different row, at a different radius -- and
    // doing that as its own animation gave three movements in a row: a reflow,
    // the fade, then a repack. Blending the two plans by cascade progress folds
    // the radial change INTO the fade, so the other wedges resize over the whole
    // time the big wedge is arriving or leaving.
    //
    //   planA -- the packing the notes are sitting in right now (pre-toggle)
    //   planB -- the packing they end in (post-toggle)
    //
    // A note missing from one plan simply uses the other: departing notes have no
    // seat in planB, arriving ones have none in planA.
    // Density comes from opacity. Feeding the packer each note's alpha as its
    // weight means rows, reference wedge widths and the hub radius are re-derived
    // every frame from what is actually on screen -- so an arriving note's own
    // wedge grows a row outward as it lands, at the same density as the rest of
    // the ring, while the wedge widens along the circumference at the same time.
    // Radial and circumferential, from one calculation.
    //
    // This replaced blending two finished packings, which could not work: two
    // separately-valid packings differ by a mean of 23.7 degrees, and lerping a
    // note's radius and its fraction-across-the-wedge independently pulled the
    // rows apart -- every note in 03 and 04 changed row, so mid-animation the
    // grid stopped being a grid. A packing derived from weights is a valid grid at
    // every value of those weights.
    var weightOf = function (id) { return alpha[id] || 0; };

    // Row counts at both ends of the move. The count is an integer and it ticks,
    // so instead of deriving it from the live weights (which made it tick at
    // arbitrary moments, reshaping the capacity ruler and flinging notes up to
    // 3191 units in a frame) it is walked from the source count to the
    // destination count by animation progress. At progress 0 that is exactly the
    // packing the notes are resting in; at 1, exactly the one settle() assigns.
    var wasPresent = Object.create(null);
    graph.forEachNode(function (id) { wasPresent[id] = present(id); });

    // WHICH PLAN BASIS settle() will use, decided once, up front.
    //
    // ringsLayout picks between the whole-vault plan and a visible-only rebuild on
    // `shown < order * REPACK_BELOW`. The cascade used to hardcode `true`, so for any
    // filter light enough to stay above that threshold the animation ran on the
    // visible-only packing and settle() then re-planned onto the whole-vault one --
    // a different plan, so the ring changed size AFTER the animation finished.
    //
    // Measured with `04 - Daily Notes` hidden (393 of 450 visible, threshold 248):
    // visible-only gave 16 cells / 64 rows / maxR 13, whole-vault 19 cells / 75 rows /
    // maxR 14, with every major cell differing by exactly one row (6 vs 7). One row is
    // the jump. `08 - Meeting Notes` never showed it because hiding it leaves 229
    // visible, below the threshold, so both sides agreed by luck.
    //
    // visible() is already at its post-toggle value on frame one, so this is exactly
    // the flag settle() will compute, and the contract above -- "at 1, exactly the one
    // settle() assigns" -- holds again.
    var shownAfter = 0;
    graph.forEachNode(function (id) { if (willShow(id)) shownAfter++; });
    var ovAfter = true;   // one basis everywhere; see REPACK_BELOW

    // The lattice spacing at each end of the toggle. Scalars, not per-cell maps: the
    // spacing is global by construction -- it is what makes the packing uniform -- so
    // there is one number per endpoint rather than one per cell.
    var spSrc = 1, spDst = 1;
    // Per band, for the same reason the layout is: one number cannot carry two rings, and a
    // ring whose spacing is interpolated from the other ring's endpoints is a ring that moves
    // when the other one is filtered.
    var spSrcB = { i: 1, o: 1 }, spDstB = { i: 1, o: 1 };
    // The two endpoint packings' band room, walked alongside their spacing. Zero means "this
    // packing had nothing in that band", and the walk below falls back to the other end rather
    // than interpolating toward an empty band.
    var roomSrcB = { i: 0, o: 0 }, roomDstB = { i: 0, o: 0 };
    // Declared HERE, not inside the endpoint-capture block below. `var` is function-scoped, so
    // declaring them in that block left the frame loop reading undeclared names -- a
    // ReferenceError on the first frame, which killed the whole animation rather than degrading
    // it. The two endpoint maps of per-cell room; see cellNow.
    var cellSrc = null, cellDst = null;
    var edgeSrc = null, edgeDst = null;
    var rowsSrc = Object.create(null), rowsDst = Object.create(null);
    var bandSrc = Object.create(null), bandDst = Object.create(null);
    // A group is PRESENT at an end if it has any seated weight there -- one wedge, one
    // gap, regardless of how many notes it keeps. The union of the two ends is walked.
    // ONE planner, called the same way at both ends.
    //
    // The cascade's endpoints have to be *the static planner's own output* for the
    // before and after states, or the animation walks between two packings that
    // nothing else ever renders. Every jump chased on 2026-08-22 was this: the two
    // were called with different arguments and drifted apart one argument at a time --
    // `onlyVisible` hardcoded to true, weights defaulting to 1 instead of the 0/1 that
    // alpha settles to, `planKeep` set by hand at each call site.
    //
    // staticPlan() takes only "which notes are present" and derives the other three
    // from it exactly as ringsLayout does at rest, so planA is what the disc was
    // resting in and planB is what settle() will assign, by construction rather than
    // by coincidence.
    var staticPlan = function (presentFn) {
      var save = planKeep;
      planKeep = presentFn;
      var shown = 0;
      graph.forEachNode(function (id) { if (presentFn(id)) shown++; });
      var p = buildWedgePlan(true,
                             function (id) { return presentFn(id) ? 1 : 0; });
      planKeep = save;
      return p;
    };
    (function () {
      var a = staticPlan(function (id) { return wasPresent[id]; });
      // The single-cell gate: a group splitting into sub-wedges is out of scope, judged on the
      // populated end's plan (the source for a hide, the destination for a show).
      var cellsOfG = function (p0) {
        var m = Object.create(null);
        if (p0) p0.cells.forEach(function (c) { m[c.g] = (m[c.g] || 0) + 1; });
        return m;
      };
      // THE DESTINATION PACKING. willShow, so it is the packing settle() will assign rather
      // than one that still seats whatever the date range or the timeline has excluded.
      var b = staticPlan(function (id) { return willShow(id); });
      var aCells = cellsOfG(a), bCells = cellsOfG(b);
      Object.keys(tglDir).forEach(function (g0) {
        var n0 = tglDir[g0] === "out" ? aCells[g0] : bCells[g0];
        if (n0 !== 1) delete tglDir[g0];
      });
      // Also the depth of each BAND at both ends -- the deepest cell in it, which is
      // what sets the ring's outer radius. A cell that exists at only one end takes
      // this instead of its own missing count, so it matches the ring rather than
      // running its own race. Keyed by band, because the two are packed
      // independently and their depths are unrelated.
      var deepen = function (m, c) {
        var k = c.inner ? "i" : "o";
        if (m[k] === undefined || c.rows > m[k]) m[k] = c.rows;
      };
      // A cell with no weight left at this end is the one arriving or leaving, and it
      // must stay UNDEFINED here so the band-depth fallback above applies to it. That
      // matters now in a way it did not before: planB is built on the same basis
      // settle() uses, and when that basis is the whole vault the departing cell is
      // still in the plan -- at weight 0, so rowsNeeded returns its floor of 1. Left
      // recorded, that walks the leaving wedge from 7 rows to 1 while the ring holds,
      // which is the "contracts faster than the ring, reads as sinking out of the disc"
      // failure this fallback was added to fix. Skipping it restores that.
      var record = function (rows, band) {
        return function (c) {
          if (c.wsum <= 0.0001) return;
          rows[c.k] = c.rows;
          deepen(band, c);
        };
      };
      if (a) a.cells.forEach(record(rowsSrc, bandSrc));
      if (b) b.cells.forEach(record(rowsDst, bandDst));
      // A BAND THAT STARTS EMPTY STARTS AT ONE ROW, so its depth WALKS like every other
      // cascade's instead of arriving at the destination's depth on frame one.
      //
      // Nothing filled bandSrc when the source packing was null, and the fallback below then
      // took the destination depth -- so a disc growing from nothing was laid out at its final
      // row count from the first frame. The notes still eased into place, but the lattice never
      // grew, and the row walk is what gives the movement its serpentine: notes shift along
      // their row and wrap to the next as the count changes. Measured on the intro, pinning the
      // rows this way looks smoother by every number -- worst tangential step p90 26 against 358
      // -- and is wrong for the same reason interpolating the endpoints was wrong: the number
      // improves because the thing being measured has been removed.
      //
      // One row is where a disc of one note rests, so this is the depth the source genuinely
      // has. Only the ARRIVING side is defaulted: a band on its way out has its own fallback
      // above, and the reasoning there -- a wedge that contracts faster than the ring reads as
      // sinking out of the disc -- still holds.
      ["i", "o"].forEach(function (k) {
        if (bandSrc[k] === undefined && bandDst[k] !== undefined) bandSrc[k] = 1;
      });
      // Taken from the endpoint plans themselves, which were built on binary presence --
      // so these are the two densities the disc genuinely rests at, not a sample of
      // whatever alpha happened to be on some frame.
      if (a && a.sp > 0) spSrc = a.sp;
      if (b && b.sp > 0) spDst = b.sp;
      if (a) { spSrcB = { i: a.spInner || a.sp || 1, o: a.sp || 1 }; }
      if (b) { spDstB = { i: b.spInner || b.sp || 1, o: b.sp || 1 }; }
      // THE ROOM EACH ENDPOINT ACTUALLY HAS, taken by laying that endpoint out and reading it
      // back, not from plan.room.
      //
      // plan.room is built from c.band, the cell's REFERENCE arc, and the disc is drawn from
      // c.span, its live share. Under filtering those differ several-fold, and handing the
      // reference figure in collapsed every dot onto the pixel floor -- diameter over step 0.02
      // where the design is 0.786. The only number that matches what gets drawn is the one the
      // layout itself computes, so each endpoint is laid out once, at cascade start, purely to
      // read it.
      //
      // Two extra layouts per cascade, next to the two plan builds staticPlan already does. The
      // room override is off while they run, so each measures its own.
      // WITH THE ALPHAS THAT ENDPOINT HAS, which is the whole of it.
      //
      // ringsLayout reads each cell's weight from the LIVE alpha map, not from the plan it is
      // handed -- geom is opacity-weighted membership. So laying planB out at cascade start gave
      // planB's cells with the PRE-TOGGLE opacities: not the destination, just a different wrong
      // answer. Measured, that put the entire jump at the hand-over: frames were smooth
      // (mean step 5, no step at the start at all) and settle() then moved notes 1517 units
      // tangentially, 15 of them past the threshold.
      //
      // playTimeline already does this dance for its own final positions -- swap the alphas in,
      // lay out, swap them back -- and this is the same need.
      var roomOf = function (pl, alphaFn) {
        if (!pl) return null;
        var outPos = null;
        var keepAlpha = null;
        if (alphaFn) {
          keepAlpha = Object.create(null);
          graph.forEachNode(function (id) { keepAlpha[id] = alpha[id]; alpha[id] = alphaFn(id); });
        }
        var keepI = bandOf("i").room, keepO = bandOf("o").room;
        var keepFit = dotFit, keepCell = cellRoom, keepEdge = edgeCap;
        var keepPin = pinnedPlan, keepKeep = planKeep;
        var saved = roomNow, savedCell = cellNow, savedEdge = edgeNow;
        roomNow = null; cellNow = null; edgeNow = null; edgeNow = null;
        outPos = ringsLayout(pl, true);
        var got = { i: bandOf("i").room, o: bandOf("o").room, pos: outPos,
                    cells: cellRoom, edges: edgeCap };
        roomNow = saved; cellNow = savedCell; edgeNow = savedEdge;
        // Everything that pass wrote as a side effect is put back: it was a measurement, not a
        // frame, and the disc must not be able to tell it happened.
        bandOf("i").room = keepI; bandOf("o").room = keepO;
        dotFit = keepFit; cellRoom = keepCell; edgeCap = keepEdge;
        pinnedPlan = keepPin; planKeep = keepKeep;
        if (keepAlpha) graph.forEachNode(function (id) { alpha[id] = keepAlpha[id]; });
        return got;
      };
      // A is where the disc IS, so its alphas are the ones in force. B is where it is GOING,
      // which is the same expression settle() lands on.
      var rA = roomOf(a, null);
      var rB = roomOf(b, function (id) { return willShow(id) ? timeFactor(id) : 0; });
      if (rA) roomSrcB = { i: rA.i || 0, o: rA.o || 0 };
      if (rB) roomDstB = { i: rB.i || 0, o: rB.o || 0 };
      cellSrc = (rA && rA.cells) || null; cellDst = (rB && rB.cells) || null;
      edgeSrc = (rA && rA.edges) || null; edgeDst = (rB && rB.edges) || null;
      // WHERE THE NOTES ARE, and where the one plan puts them. The source is read off the graph
      // rather than reconstructed from planA: the disc on screen is the truth about where a note
      // is starting from, and a packing built to describe it can only ever be an approximation
      // of it -- which is where the jump at the START of an animation came from.
      posSrc = Object.create(null);
      graph.forEachNode(function (id) {
        posSrc[id] = { x: graph.getNodeAttribute(id, "x"), y: graph.getNodeAttribute(id, "y") };
      });
      posDst = finalPos;
    })();

    // The guard is a WATCHDOG on stalled frames, not a deadline. It used to be a
    // fixed setTimeout at roughly span/60 seconds, which assumes 60fps -- and this
    // page does not always manage that with 442 nodes and 1409 edges. Below about
    // 30fps the timeout fired part-way through and settle() snapped the disc to
    // its final layout: a jump at the end of every animation, on exactly the
    // machines where the animation mattered. It now only fires if no frame has
    // arrived for a while, so a slow page simply animates slower.
    var STALL_MS = 400;
    var watchdog = function () {
      if (cascadeRun && NOW() - cascadeRun.tick < STALL_MS) {
        cascadeRun.guard = WIN.setTimeout(watchdog, STALL_MS);
        return;
      }
      settle();
    };
    // `span` is measured in frames, and every stagger delay and fade window below is
    // expressed in the same unit. Rather than convert all of them, map one frame onto
    // however many milliseconds it must take for the whole span to land on CASCADE_MS.
    // The proportions -- which note starts when, how long each fade lasts -- are
    // untouched; only the clock changes.
    // Wall clock is set here, not by span: span is a nominal frame count and this divides the
    // total by it, so a cascade lasts the same time whatever its shape. The timeline wants its
    // own total (TIMELINE_MS), which is the second of the only two things it needs differently.
    var msPerFrame = (opts.totalMs > 0 ? opts.totalMs : CASCADE_MS * TIME_SCALE) / Math.max(1, span);
    // Real time drives the progress, but a single frame may never advance more than
    // 1/MIN_FRAMES of the whole span. `04 - Daily Notes` is the toggle that made this
    // necessary: 55 notes plus a repack drops the frame rate far enough that pure
    // wall-clock progress moved the disc in visible leaps. Clamped, a slow page takes
    // longer than CASCADE_MS instead of jumping -- the fixed duration holds whenever
    // there are enough frames to draw it, which is the only time it is worth having.
    var MIN_FRAMES = 20;
    var maxAdv = Math.max(1, span) / MIN_FRAMES;
    var frame = 0, tPrev = NOW(), tailFrames = 0;
    cascadeRun = { raf: 0, tick: NOW(), guard: WIN.setTimeout(watchdog, STALL_MS) };
    (function step() {
      var tn = NOW();
      var adv = (tn - tPrev) / msPerFrame;
      tPrev = tn;
      if (adv > maxAdv) adv = maxAdv;
      frame += adv;
      if (cascadeRun) cascadeRun.tick = tn;
      var busy = false;
      for (var i = 0; i < moving.length; i++) {
        var id = moving[i];
        var q = (frame - delay[id]) / (FADE_FRAMES * TIME_SCALE);
        q = q < 0 ? 0 : q > 1 ? 1 : q;
        alpha[id] = from[id] + (to[id] - from[id]) * (q * q * (3 - 2 * q));   // smoothstep
        if (q < 1) busy = true;
      }

      // Progress across the WHOLE cascade, not per note: this is what carries
      // the repack, so it has to finish exactly when the last note does.
      var pr = Math.min(1, frame / Math.max(1, span));
      var ease = pr * pr * (3 - 2 * pr);
      if (opts.onFrame) opts.onFrame(pr);

      // THE GAP RESERVATION IS NOT WALKED ANY MORE -- see allocateBand's groupPres. It used to
      // be interpolated here, from 1 to 0 across the cascade's own clock, and that clock is not
      // the one the notes fade on: a note's opacity finishes on a per-note fade clock, far
      // sooner. The reservation outlived the notes it was reserving for, and released what was
      // left of itself in the single frame their cell was culled. The plan measures each group's
      // presence from the same weights it packs with instead, so there is one clock.

      // ONE allocation per frame, from a plan whose geometry is interpolated
      // between the two packings. Angles are therefore assigned once, in a single
      // contiguous sweep, and cannot overlap; the radial re-densification arrives
      // as the blended rows, spread across the whole fade.
      // Membership is the union of staying and still-on-screen (planKeep), and
      // the weights do the densifying.
    // A RAMPED GROUP'S FADES FILL THE WHOLE CASCADE. The wedge's arc walks the full span, and
    // the default stagger window is far shorter for a small group -- so its notes were gone by
    // a third in, the ramped wedge closed EMPTY for the rest, and when the last note's cull
    // took the cell out of the plan the arc fell off a cliff: measured 6.92 degrees in one
    // frame on the single-note folder, 84x the mean step. Spread across span - FADE, the last
    // fade lands at the cascade's end, where the arc is ~0 and the cull costs nothing. Order:
    // inner to outer on a hide (the column winks out from the hub), outer to inner on a show
    // (it materialises from the rim) -- the two are time-mirrors, like the arc itself.
    (function () {
      var stretch = Math.max(1, span - FADE_FRAMES * TIME_SCALE);
      Object.keys(tglDir).forEach(function (g0) {
        var set = (tglDir[g0] === "out" ? outs : ins).filter(function (id) {
          return groupOf(id) === g0;
        });
        if (!set.length) return;
        set.sort(function (p, q) {
          var ap = graph.getNodeAttributes(p), aq = graph.getNodeAttributes(q);
          var d = Math.hypot(ap.x, ap.y) - Math.hypot(aq.x, aq.y);
          return tglDir[g0] === "out" ? d : -d;   // hide: inner first; show: outer first
        });
        // HIDES ONLY. The cliff this fixes is a hide phenomenon -- the last note's fade
        // culls the cell and the arc falls off whatever it was still holding -- and a show
        // has no equivalent: its cell exists from the first frame. Stretching the arrivals
        // too left two of a three-note column standing in a part-open wedge for 28 frames,
        // against 1 on develop. So a show keeps the natural stagger and only its ORDER is
        // ours: outer first, so the column materialises from the rim inward.
        if (tglDir[g0] === "out") {
          set.forEach(function (id, i) {
            delay[id] = set.length < 2 ? stretch : stretch * i / (set.length - 1);
          });
        } else {
          var base0 = set.map(function (id) { return delay[id] || 0; }).sort(function (x, y) { return x - y; });
          set.forEach(function (id, i) { delay[id] = base0[i]; });
        }
      });
    })();

      var rowsAt = function (c) {
        var s = rowsSrc[c.k], d = rowsDst[c.k];
        if (s === undefined && d === undefined) return 0;   // cell knows best
        // A cell missing from ONE end is the one arriving or leaving. It takes the
        // depth its BAND has at that end, so it carries the same number of rows as
        // the rest of the ring for the whole toggle and only its arc closes.
        //
        // Two earlier versions were both wrong in opposite directions. Mirroring the
        // other end (`s = d`, `d = s`) pinned its count, so the disc re-densified
        // around a wedge that held its radii and faded at full size. Walking to 0
        // fixed that but overshot: the wedge then contracted FASTER than the ring
        // -- measured, 1494 against the survivors' 1814 at 75% -- so it read as
        // sinking out of the disc rather than shrinking with it. The band's own
        // depth is the value that keeps the ring's outer edge continuous across the
        // toggling wedge from the first frame to the last.
        var bk = c.inner ? "i" : "o";
        if (s === undefined) s = bandSrc[bk] !== undefined ? bandSrc[bk] : d;
        if (d === undefined) d = bandDst[bk] !== undefined ? bandDst[bk] : s;
        return s + (d - s) * ease;
      };
      // Same clock as the rows and the gap reservation above: at ease 0 this is the
      // packing the disc is resting in and at 1 it is the one settle() assigns.
      var roomWalk = function (k) {
        var sv = roomSrcB[k], dv = roomDstB[k];
        if (!(sv > 1)) return dv;          // band was empty at this end
        if (!(dv > 1)) return sv;
        return sv + (dv - sv) * ease;
      };
      // The band's DEPTH, walked between the two endpoint packings -- the same clock and the
      // same endpoints the per-cell row count uses. A band missing from one end takes the other
      // end's depth rather than interpolating toward nothing, which is the rule bandSrc already
      // follows.
      var depthWalk = function (k) {
        var a2 = bandSrc[k], b2 = bandDst[k];
        if (a2 === undefined && b2 === undefined) return 0;
        if (a2 === undefined) a2 = b2;
        if (b2 === undefined) b2 = a2;
        return a2 + (b2 - a2) * ease;
      };
      var spNow = {
        i: spSrcB.i + (spDstB.i - spSrcB.i) * ease,
        o: spSrcB.o + (spDstB.o - spSrcB.o) * ease,
        depth: { i: depthWalk("i"), o: depthWalk("o") },
      };
      // The walked room, handed to ringsLayout through the same channel the spacing uses. Set
      // before the plan is built so the margins inside this frame's layout read it.
      roomNow = { i: roomWalk("i"), o: roomWalk("o") };
      // The arc ramps, on the same clock as everything else the frame walks.
      colWalk = Object.create(null);
      Object.keys(tglDir).forEach(function (g0) {
        // pr, not ease: ease is the smoothstep the NOTES ride, and an arc linear in ease is
        // S-shaped in time -- measured as a symmetric +-1.2 degree residual on an 11.9-degree
        // wedge. Constant speed is a statement about the clock on the wall.
        colWalk[g0] = { f: tglDir[g0] === "out" ? 1 - pr : pr, n: tglN[g0] || 1 };
      });
      // AND THE PER-CELL ROOM, on the same clock. A note in only one endpoint takes that
      // endpoint's figure rather than interpolating toward a cell that does not exist there,
      // which is the same fallback the band walk uses for an empty band.
      if (cellSrc || cellDst) {
        var cn = Object.create(null);
        var put = function (id) {
          if (cn[id] !== undefined) return;
          var a0 = cellSrc ? cellSrc[id] : undefined, b0 = cellDst ? cellDst[id] : undefined;
          if (a0 === undefined && b0 === undefined) return;
          if (a0 === undefined) a0 = b0;
          if (b0 === undefined) b0 = a0;
          cn[id] = a0 + (b0 - a0) * ease;
        };
        if (cellSrc) Object.keys(cellSrc).forEach(put);
        if (cellDst) Object.keys(cellDst).forEach(put);
        cellNow = cn;
      }
      if (edgeSrc || edgeDst) {
        var en = Object.create(null);
        var putE = function (id) {
          if (en[id] !== undefined) return;
          var a1 = edgeSrc ? edgeSrc[id] : undefined, b1 = edgeDst ? edgeDst[id] : undefined;
          if (a1 === undefined && b1 === undefined) return;
          if (a1 === undefined) a1 = b1;
          if (b1 === undefined) b1 = a1;
          en[id] = a1 + (b1 - a1) * ease;
        };
        if (edgeSrc) Object.keys(edgeSrc).forEach(putE);
        if (edgeDst) Object.keys(edgeDst).forEach(putE);
        edgeNow = en;
      }
      // PLANNED, EVERY FRAME, BY THE ONE PLANNER -- not interpolated between two layouts.
      //
      // Interpolating the endpoint layouts was tried and measured beautifully: no step at the
      // start, mean frame 0, nothing at settle, 128 frames. It is also wrong, and the measurement
      // could not see it. A frame became a point on a straight polar line between two lattices,
      // so the lattice itself stopped existing in between -- and with it the serpentine, which is
      // the whole reason a row's last note and the next row's first sit at the same end. Notes
      // crossed each other mid-flight instead of following the boustrophedon.
      //
      // A plan per frame keeps every frame a valid packing, which is what makes the movement read
      // as the disc rearranging rather than as dots sliding about. What must NOT be per-frame is
      // the note SIZE: it comes from a percentile over the notes currently placed, so computing
      // it per frame makes it fluctuate with the weights. That one quantity is interpolated --
      // see roomNow.
      var plan = buildWedgePlan(ovAfter, weightOf, rowsAt, spNow);
      var targets = plan ? ringsLayout(plan, true) : null;
      // CONVERGE BEFORE SETTLING. Easing closes only RADIAL_EASE of each note's gap
      // per frame, so when progress hits 1 a small remainder is still outstanding --
      // and settle() assigns exact targets, closing it in a single frame. That is the
      // "small jump at the end, like a skipped frame", and it is the cost the doc
      // attributes to easing.
      //
      // So the loop keeps running past pr = 1 while any note is still more than half a
      // unit from its target, and the ease ramps to 1 across that tail. The targets are
      // static by then, so the remainder decays fast: from a full row it is under half
      // a unit within a handful of frames, and settle() becomes a no-op rather than a
      // correction. Ramping only in the TAIL matters -- ramping across the whole
      // animation would restore the very tick the easing exists to smooth, which for
      // 04 lands around 90% of the way through.
      var ez = pr < 1 ? RADIAL_EASE
                      : Math.min(1, RADIAL_EASE + tailFrames * 0.15);
      var resid = 0;
      if (targets) graph.forEachNode(function (id) {
        var q = targets[id];
        if (!q) return;
        // The ANGLE is taken exactly -- it is the circumferential motion, and
        // easing it would put the wedge out of step with the ring again. Only the
        // RADIUS is eased, because that is what steps: a row count is an integer,
        // so it ticks rather than glides, and easing turns each tick into a short
        // slide of about one row instead of a jump.
        var x = graph.getNodeAttribute(id, "x"), y = graph.getNodeAttribute(id, "y");
        var h = Math.atan2(q.y, q.x);
        var rNow = Math.hypot(x, y), rWant = Math.hypot(q.x, q.y);
        var gap = rWant - rNow;
        if (gap < 0 ? -gap > resid : gap > resid) resid = gap < 0 ? -gap : gap;
        var r = rNow + gap * ez;
        graph.mergeNodeAttributes(id, { x: r * Math.cos(h), y: r * Math.sin(h) });
      });
      if (pr >= 1) tailFrames++;
      // skipIndexation while animating: the quadtree is only needed for hit
      // testing, and settle() rebuilds it. Re-indexing 442 nodes and 1409 edges
      // every frame was the reason the frame rate fell far enough for the old
      // fixed guard to fire mid-animation.
      probeSample("cascade");
      lastCascade.frames++;
      lastCascade.ms = Math.round(NOW() - lastCascade.t0);
      renderer.refresh({ skipIndexation: true });
      // `resid > 0.5` is the new clause: never hand over to settle() while a note is
      // still visibly short of its target. Bounded by the ramp above, so this adds a
      // few frames, not an open-ended tail.
      // Only while something is recording: this is one object per frame in the hot loop,
      // and the frame count and elapsed time above already answer "did it animate".
      if (probe) lastCascade.last = { adv: Math.round(adv * 1000) / 1000, frame: Math.round(frame * 100) / 100,
                           span: Math.round(span * 100) / 100, pr: Math.round(pr * 1000) / 1000,
                           busy: busy, resid: Math.round(resid * 100) / 100,
                           msPerFrame: Math.round(msPerFrame * 1000) / 1000,
                           moving: moving.length, run: !!cascadeRun };
      if (busy || pr < 1 || resid > 0.5) cascadeRun.raf = WIN.requestAnimationFrame(step);
      else { lastCascade.exit = "converged"; settle(); }
    })();
  }

  /* ------------------------------------------------------------- animation */

  // FRAME-BY-FRAME PROBE, ON BOTH AXES. Reasoning about which band can move the other has
  // been wrong twice, so measure it: this samples each band's radial extent on every
  // animated frame, and the report gives the biggest single-frame step per band. A
  // "jump" is a large step in one frame; a smooth animation is many small ones.
  //
  // THE TANGENTIAL HALF WAS MISSING FOR MONTHS, and a whole class of jump with it. Every
  // number here was a radius, so a change that left radii alone and slid every wedge round
  // the circle measured as perfectly smooth -- which is exactly what a change in the gap
  // reservation does. The date sliders jumped visibly while "a range change animates instead
  // of snapping" passed, because the only thing it could see was the radius.
  //
  // Tangential displacement is reported in GRAPH UNITS, not degrees: a degree at the rim is
  // four times the movement it is at the hub, and what a person sees is the distance. Only
  // notes present in both frames count -- a note fading in has no previous position, and
  // charging it for arriving would report a jump on every frame of every animation.
  var probe = null;
  function probeSample(tag) {
    if (!probe) return;
    var iMin = Infinity, iMax = 0, oMin = Infinity, oMax = 0, iN = 0, oN = 0;
    var prev = probe.prevAng, now = Object.create(null);
    var prevR = probe.prevR, nowR = Object.create(null);
    var tanStep = 0, tanId = null, tanOver = 0, tanSum = 0, tanN = 0;
    // PER-NOTE RADIAL STEP, which is what the tangential half has always measured and the
    // radial half never did. Both band extents below are a MAX OVER A SET, and the set churns
    // as notes arrive and leave -- so when the furthest note winks out the maximum is handed
    // to the next one in and the sample steps by the distance between them, reporting a jump
    // in a disc that did not move. Worse in the other direction: a stale note parked outside
    // every visible one pinned the extent flat across an entire cascade (path 0 over 154
    // frames), so a moving disc measured as perfectly still. A note's own change in radius has
    // neither failure, and "the disc jumped" is a statement about notes, not about a maximum.
    var radStep = 0, radId = null, radSum = 0, radN = 0;
    graph.forEachNode(function (id, a) {
      var r = Math.hypot(a.x, a.y);
      // Tangential step: the angle moved, times the radius it moved at.
      if (present(id)) {
        var th = Math.atan2(a.y, a.x);
        now[id] = th;
        nowR[id] = r;
        if (prevR && prevR[id] !== undefined) {
          var dr = Math.abs(r - prevR[id]);
          if (dr > radStep) { radStep = dr; radId = id; }
          radSum += dr; radN++;
        }
        if (prev && prev[id] !== undefined) {
          var d = th - prev[id];
          // Shortest way round, or a note crossing the 12 o'clock seam reports a
          // near-full-circle jump every time it wraps.
          while (d > Math.PI) d -= 2 * Math.PI;
          while (d < -Math.PI) d += 2 * Math.PI;
          var moved = Math.abs(d) * r;
          if (moved > tanStep) { tanStep = moved; tanId = id; }
          if (moved > 160) tanOver++;         // further than one row, in one frame
          tanSum += moved; tanN++;
        }
      }
      // github#17, now fixed: this used to be unfiltered, and the comment here said the honest
      // fix needed a set fixed across the whole probe rather than a filter. That is what
      // probe.set is -- see the note where it is captured.
      // Only notes the probe was started with, and only while they are still drawn. See the
      // note on probe.set for why it is neither "everything" nor "whatever is present now".
      // THE SET, AND ONLY THE SET -- no second filter on current opacity. Adding one looked
      // harmless and reintroduced the collapse this replaced: notes near a range edge carry a
      // fractional timeFactor from the heat ramp, so they cross any opacity threshold at
      // different moments and drop in and out of a max. Measured with the filter on, the demo
      // vault reported a single frame of 3406 units against a path of 4169 while the worst
      // NOTE moved 149 -- the extent was not moving, its membership was. The set is fixed
      // precisely so membership cannot move.
      if (probe.set && !probe.set[id]) return;
      if (bandLock && bandLock[groupOf(id)]) {
        iN++; if (r < iMin) iMin = r; if (r > iMax) iMax = r;
      } else {
        oN++; if (r < oMin) oMin = r; if (r > oMax) oMax = r;
      }
    });
    probe.prevAng = now;
    probe.prevR = nowR;
    if (probe.watch) probe.watchSeries.push(probe.watched || null);
    probe.samples.push({
      tag: tag, ms: Math.round(NOW() - probe.t0), gapI: lastGapN.i, gapO: lastGapN.o,
      ngI: bandOf("i").nG, ngO: bandOf("o").nG,
      gapDegI: bandOf("i").gapDeg, gapDegO: bandOf("o").gapDeg,
      radStep: Math.round(radStep), radId: radId,
      radMean: Math.round(radN ? radSum / radN : 0),
      tanStep: Math.round(tanStep), tanId: tanId, tanOver: tanOver,
      tanMean: Math.round(tanN ? tanSum / tanN : 0),
      // Where each group's wedge STARTS. "The gap jumped" is precisely this series
      // moving in a step rather than a ramp, and it is what a person sees: every
      // wedge boundary shifting round the disc at once.
      starts: lastStart,
      arcs: lastArc,
      bands: lastBand,
      innerN: iN, innerMin: Math.round(iMin === Infinity ? 0 : iMin), innerMax: Math.round(iMax),
      outerN: oN, outerMin: Math.round(oMin === Infinity ? 0 : oMin), outerMax: Math.round(oMax)
    });
  }

  var anim = null, animGuard = null;   // in-flight tween + its force-complete timer

  function assignPositions(targets) {
    graph.forEachNode(function (id) {
      var t = targets[id];
      if (t) graph.mergeNodeAttributes(id, { x: t.x, y: t.y });
    });
  }

  // Tween positions so a reflow reads as the pie rearranging rather than
  // teleporting. Runs for TWEEN_MS regardless of frame rate; the guard timer below
  // force-completes the move, so a throttled or stopped rAF cannot leave nodes
  // smeared between their old and new positions -- which is what sank the first
  // wall-clock attempt, before there was a backstop.

  function animateTo(targets, done) {
    if (anim) { WIN.cancelAnimationFrame(anim); anim = null; }
    if (animGuard) WIN.clearTimeout(animGuard);

    // In Rings, interpolate in POLAR space about the disc centre so nodes sweep
    // along arcs -- the pie reads as rotating into its new arrangement rather
    // than every dot cutting a straight line across the middle. Force keeps
    // cartesian interpolation, since that layout is not centred on the origin.
    var polar = state.layout === "rings";
    var from = {};
    graph.forEachNode(function (id, a) {
      var t = targets[id] || { x: a.x, y: a.y };
      if (!polar) { from[id] = { x: a.x, y: a.y, tx: t.x, ty: t.y }; return; }
      var r0_ = Math.hypot(a.x, a.y), r1_ = Math.hypot(t.x, t.y);
      var h0 = Math.atan2(a.y, a.x), h1 = Math.atan2(t.y, t.x);
      var d = h1 - h0;                       // take the short way round
      while (d > Math.PI) d -= 2 * Math.PI;
      while (d < -Math.PI) d += 2 * Math.PI;
      from[id] = { r: r0_, h: h0, dr: r1_ - r0_, dh: d };
    });

    var settle = function () {
      if (anim) { WIN.cancelAnimationFrame(anim); anim = null; }
      if (animGuard) { WIN.clearTimeout(animGuard); animGuard = null; }
      assignPositions(targets);
      renderer.refresh({ skipIndexation: false });
      if (done) done();
    };
    var dur = TWEEN_MS * TIME_SCALE;
    // WATCHDOG, not a deadline. A fixed `WIN.setTimeout(settle, dur + margin)` fires
    // part-way through whenever the page cannot render fast enough to finish in time,
    // and settle() then snaps the disc to its final layout -- a jump at the end of
    // every animation, on exactly the machines where the animation matters. This only
    // fires when no FRAME has arrived for STALL_MS, so a slow page animates slowly and
    // a stopped rAF still lands.
    var lastFrame = NOW();
    var TWEEN_STALL = 400;
    var tweenDog = function () {
      if (anim && NOW() - lastFrame < TWEEN_STALL) { animGuard = WIN.setTimeout(tweenDog, TWEEN_STALL); return; }
      settle();
    };
    animGuard = WIN.setTimeout(tweenDog, TWEEN_STALL);

    // Advance by real time so the tween is the same length everywhere, but never by
    // more than 1/MIN_FRAMES of it in a single frame. Below that frame rate the
    // animation stretches instead of teleporting: fixed duration is only worth having
    // while there are enough frames to draw it smoothly.
    var MIN_FRAMES = 20;
    var p = 0, tPrev = NOW();
    (function step() {
      var tn = NOW();
      lastFrame = tn;
      var adv = (tn - tPrev) / dur;
      tPrev = tn;
      if (adv > 1 / MIN_FRAMES) adv = 1 / MIN_FRAMES;
      p = Math.min(1, p + adv);
      var e = p < 0.5 ? 4 * p * p * p : 1 - Math.pow(-2 * p + 2, 3) / 2;
      graph.forEachNode(function (id) {
        var f = from[id];
        if (!f) return;
        if (polar) {
          var r = f.r + f.dr * e, h = f.h + f.dh * e;
          graph.mergeNodeAttributes(id, { x: r * Math.cos(h), y: r * Math.sin(h) });
        } else {
          graph.mergeNodeAttributes(id, {
            x: f.x + (f.tx - f.x) * e,
            y: f.y + (f.ty - f.y) * e
          });
        }
      });
      probeSample("tween");
      renderer.refresh({ skipIndexation: false });
      if (p < 1) { anim = WIN.requestAnimationFrame(step); }
      else { settle(); }
    })();
  }



  function applyLayout(animate, done) {
    var targets = ringsLayout();
    if (!targets) { if (done) done(); return; }
    if (animate) animateTo(targets, done);
    else {
      assignPositions(targets);
      renderer.refresh({ skipIndexation: false });
      if (done) done();
    }
  }

  /* ---------------------------------------------------------------- render */

  var renderer, neighbourCache = null;

  function neighboursOf(id) {
    // From the adjacency, not graph.neighbors(): in a budgeted vault the graph is missing
    // every trimmed link (see syncLazyEdges), so asking it would undercount everywhere -- and
    // the halo, focusSet and the detail panel all need the note's real neighbourhood.
    if (!neighbourCache) neighbourCache = {};
    if (!neighbourCache[id]) {
      neighbourCache[id] = (adj[id] || []).map(function (e) { return e.o; });
    }
    return neighbourCache[id];
  }

  /**
   * In a budgeted vault, top the graph up with the focused note's COMPLETE web -- and take
   * exactly that top-up back when the focus moves on. The resting budget is never touched.
   *
   * Called wherever the focus (hovered-or-selected, the same resolution focusSet uses) can
   * change: enterNode, select(), and the hover tween's landing frame. That last one matters:
   * state.hovered is only released when the tween reaches zero, precisely so the fade-out gets
   * drawn -- so the extra links must survive until the same moment, or they would vanish while
   * the note under the pointer is still fading. The extras are tracked by name rather than
   * re-derived: deciding what to remove by re-asking the budget would couple this to the
   * selection rule, and a tracked list cannot disagree with what was actually added.
   */
  var lazyShown = null, lazyAdded = [];
  function syncLazyEdges() {
    if (!lazyEdges) return;
    var want = state.hovered || state.selected || null;
    if (want === lazyShown) return;
    lazyAdded.forEach(function (pr) {
      if (graph.hasEdge(pr[0], pr[1])) graph.dropEdge(pr[0], pr[1]);
    });
    lazyAdded = [];
    if (want) (adj[want] || []).forEach(function (e) {
      if (!graph.hasEdge(want, e.o)) {
        graph.addUndirectedEdge(want, e.o, edgeAttrsOf(e.w));
        lazyAdded.push([want, e.o]);
      }
    });
    lazyShown = want;
  }

  // The key for a folder at depth k in a note's chain: "PARA/a", "PARA/a/b", ...
  // Depth 1 is the old `folder + "/" + sub`, so every existing key still matches.
  function pathKey(a, k) {
    return a.folder + "/" + (a.dirs || []).slice(0, k).join("/");
  }

  function visible(id) {
    var a = graph.getNodeAttributes(id);
    if (isHidden(groupOf(id))) return false;
    // Subfolder filtering only bites while the folders are what we group by.
    // Every ANCESTOR is checked, at whatever depth the vault happens to nest, so
    // hiding a folder hides its whole subtree without needing to know how deep it is.
    if (state.dim === "folder") {
      var d = a.dirs || [];
      if (!d.length && state.hiddenSub[a.folder + "/"]) return false;
      for (var k = 1; k <= d.length; k++) if (state.hiddenSub[pathKey(a, k)]) return false;
    }
    return true;
  }

  /* ------------------------------------------------------------ hover tween */

  // Hovering a note used to SNAP: everything else went to --dim on one frame and the
  // label appeared with it. That is the same discrete step this whole project exists to
  // remove from the layout, and it reads the same way -- as the disc flinching rather
  // than responding. `hoverT` is how far the treatment has arrived, and every part of it
  // (the dim, the lift, the edge highlight, the label) is a function of it.
  //
  // Short, because this fires on every note the pointer crosses: anything slower reads
  // as lag rather than as motion. Measured against the tween that already exists for a
  // click (TWEEN_MS 380) -- a third of that is about right for something this frequent.
  var HOVER_MS = 150;
  // How much the hovered note lifts. Modest on purpose: the dot has to stay on its
  // lattice row, so this is the one place a note may change size without moving.
  var HOVER_GROW = 0.45;
  var hoverT = 0, hoverAim = 0, hoverRaf = 0, hoverPrev = 0;

  // sRGB, not OKLab. This runs per node per frame while a hover arrives, and the
  // endpoints are a group hue and the neutral dim -- a perceptual path between them buys
  // nothing visible and costs two cbrt per channel. Cached on a hundredth of t, which is
  // finer than the eye and coarse enough that the cache actually hits.
  var mixCache = Object.create(null);
  function mixHex(from, to, t) {
    if (t <= 0) return from;
    if (t >= 1) return to;
    var key = from + to + t.toFixed(2);
    var hit = mixCache[key];
    if (hit) return hit;
    var a = toRgb(from), b = toRgb(to), out = "#";
    for (var i = 0; i < 3; i++) {
      var v = Math.round(a[i] + (b[i] - a[i]) * t).toString(16);
      out += v.length < 2 ? "0" + v : v;
    }
    return (mixCache[key] = out);
  }

  // How far the hover treatment should be applied for this frame. A CLICK selection has
  // no tween and must not be animated by whatever the pointer is doing, so it is always
  // fully applied; only a hover ramps.
  // codanna: pinned at 1 while the hovered note IS the selected one. Selection already
  // holds the focus treatment at full; letting the tween drive it ramped the node's
  // size, the web's alpha and the dim toward zero on leave and snapped them back when
  // the tween released state.hovered -- a flick on every mouse-out of the active note.
  function hoverAmount() {
    return state.hovered && state.hovered !== state.selected ? hoverT : 1;
  }

  function hoverTo(aim) {
    hoverAim = aim;
    if (hoverRaf) return;            // already walking -- it will pick up the new aim
    hoverPrev = NOW();
    (function step() {
      var now = NOW(), dt = now - hoverPrev;
      hoverPrev = now;
      // Clamped like every other animation here: a stalled frame stretches the tween
      // rather than leaping it, so a backgrounded tab resumes mid-fade instead of
      // arriving already finished.
      var adv = Math.min(dt, HOVER_MS) / (HOVER_MS * TIME_SCALE);
      hoverT += hoverAim > hoverT ? adv : -adv;
      if (hoverT > 1) hoverT = 1;
      if (hoverT < 0) hoverT = 0;
      var landed = hoverT === hoverAim;
      // The hovered id is released only HERE, at zero. Clearing it in leaveNode would
      // empty focusSet() on that frame and snap the whole disc back to full colour --
      // the fade-out would never be drawn.
      if (landed && hoverT === 0) { state.hovered = null; syncLazyEdges(); }
      renderer.refresh({ skipIndexation: true });   // nothing moved; only colour and size
      if (landed) { hoverRaf = 0; return; }
      hoverRaf = WIN.requestAnimationFrame(step);
    })();
  }

  /* -------------------------------------------------------- highlight ramp */

  // How far each note has arrived at being highlighted. PER NOTE, not one global scalar,
  // and that is not over-engineering: highlights are additive. Two groups can be lit at
  // once, a subfolder can be lit inside an already-lit group, and a heatmap hover swaps
  // one whole set for another. A single ramp would pop the second set in at whatever
  // value the first had left it, and could not fade one set out while another fades in.
  // `alpha` already establishes the pattern -- a per-note value that something walks.
  var hl = Object.create(null);            // id -> 0..1
  var hlRaf = 0, hlPrev = 0, hlSig = "";
  // Deliberately TWEEN_MS, the same duration as the radial push in animateTo. Becoming
  // highlighted is ONE event that moves a note and grows it, so the two have to land
  // together; at different durations the dot arrives and then keeps swelling.
  // How much bigger a highlighted note gets, ON TOP of the 0.3 the halo ring already
  // needed to sit outside the dot. Small on purpose -- the ring and the radial push
  // already say "this one", so this is confirmation, not the signal.
  var HL_GROW = 0.2;

  // A cheap fingerprint of WHAT IS HIGHLIGHTED, so the per-note sweep below runs only
  // when the answer can have changed. Called every frame from afterRender; sweeping 450
  // nodes there instead would be pointless work on every frame of every animation.
  function hlSignature() {
    return Object.keys(state.highlight).join(",") + "|" +
           Object.keys(state.highlightSub).join(",") + "|" +
           (state.markDay || "") + "|" + (state.hoverDay || "") + "|" +
           // Both hover sources belong here for the same reason everything else does:
           // this is what decides whether the per-note sweep runs at all, so a source
           // missing from it is a source whose highlight silently never ramps.
           (state.hoverGroup || "") + "|" + Object.keys(state.hoverSub).join(",") + "|" +
           // The hovered YEAR, for exactly that reason: it haloes notes, so it has to be
           // able to change the signature or the ramp never starts.
           (state.hoverYear || "");
  }

  function hlWalk() {
    if (hlRaf) return;                     // already walking; it re-reads the aims itself
    hlPrev = NOW();
    (function step() {
      var now = NOW(), dt = now - hlPrev;
      hlPrev = now;
      // Clamped like every other animation here, so a stalled frame stretches the ramp
      // rather than leaping it.
      var adv = Math.min(dt, TWEEN_MS) / (TWEEN_MS * TIME_SCALE);
      var moving = false;
      graph.forEachNode(function (id) {
        var aim = isHighlighted(id) ? 1 : 0, v = hl[id] || 0;
        if (v === aim) return;
        v += aim > v ? adv : -adv;
        if (v > 1) v = 1;
        if (v < 0) v = 0;
        hl[id] = v;
        if (v !== aim) moving = true;
      });
      renderer.refresh({ skipIndexation: true });   // size and colour only; nothing moved
      if (!moving) { hlRaf = 0; return; }
      hlRaf = WIN.requestAnimationFrame(step);
    })();
  }

  // Called from afterRender: no call site has to remember to start the ramp, which
  // matters because the highlight set is written from five different places (three legend
  // handlers, and the heatmap's click and hover).
  function hlSync() {
    var sig = hlSignature();
    if (sig === hlSig) return;
    hlSig = sig;
    hlWalk();
  }

  // MEMOIZED BY FOCUS ID, not recomputed on every call. edgeReducer calls this once PER
  // EDGE on every render pass (it colours/dims each edge against the current focus), and
  // drawFocusWeb calls it again on top of that -- both rebuilding the same object from
  // scratch each time, on a graph that can hold thousands of edges. The result depends on
  // nothing but `f` and neighboursOf(f): neighboursOf reads `adj`, the note's always-
  // complete adjacency, not the lazy-edge-budgeted graph (see neighboursOf's own comment),
  // so it cannot go stale under a given f the way something reading live budgeted edges
  // could. Caching by `f` alone -- not "once per frame" -- means it also survives
  // unchanged across a whole hover, not just a single frame of it.
  var focusSetCache = { key: undefined, set: null };
  function focusSet() {
    var f = state.hovered || state.selected;
    if (focusSetCache.key === f) return focusSetCache.set;
    var set = null;
    if (f) {
      set = Object.create(null);
      set[f] = true;
      neighboursOf(f).forEach(function (n) { set[n] = true; });
    }
    focusSetCache.key = f;
    focusSetCache.set = set;
    return set;
  }

  // Shared by drawFocusWeb and checkFocusWeb, so the diagnostic can never drift from what
  // is actually drawn: the viewport-projected endpoints and the quadratic control point
  // for one edge (control point = chord midpoint + curvature x the chord normal, matching
  // curvatureFor's own sign and magnitude). null for a hidden edge.
  function edgeCurveGeom(e, s, t) {
    var ed = renderer.getEdgeDisplayData(e);
    if (!ed || ed.hidden) return null;
    var ps = renderer.graphToViewport(graph.getNodeAttributes(s));
    var pt = renderer.graphToViewport(graph.getNodeAttributes(t));
    var dx = pt.x - ps.x, dy = pt.y - ps.y, k = ed.type === "curve" ? (ed.curvature || 0) : 0;
    return { ed: ed, ps: ps, pt: pt, k: k, cp: { x: (ps.x + pt.x) / 2 + dy * k, y: (ps.y + pt.y) / 2 - dx * k } };
  }

  // THE FOCUS WEB, DRAWN ONCE MORE ABOVE THE DIM NOTES. Sigma paints every edge on its
  // bottom layer and every node above that, so the edges edgeReducer lights on hover or
  // click ran under the notes they crossed -- each dim disc in the way cut a grey gap out
  // of a blue line, and on a well-connected hub the web read as dashed (issue #2). The
  // approach here follows the diagnosis and geometry already diffed on the fork branch
  // linked from that issue (bartolli/vault-graph@21a618c), reimplemented against this
  // file's current state rather than applied verbatim.
  //
  // `hovers` sits above `nodes`, so stroking the focus edges here and re-drawing the
  // focus neighbours' discs over them stacks dim notes < web < lit notes -- on this one
  // canvas, and BELOW the label pill drawHover paints next. NOT by marking the neighbours
  // `highlighted`: that lifts them onto hoverNodes, which sits above the pill too, and the
  // lit discs ate the focus note's own name. Geometry is the curve program's own: control
  // point = chord midpoint + curvature x the chord normal (matches curvatureFor's sign and
  // magnitude); thickness = max(minEdgeThickness, scaleSize(size)); colour = the edge
  // reducer's, already lit or dimmed. Alpha follows the hover ramp so the web arrives with
  // the dim instead of popping in over it.
  function drawFocusWeb(ctx, data, settings) {
    var f = state.hovered || state.selected;
    if (!f || data.key !== f || state.query) return;
    var ht = hoverAmount();
    if (ht <= 0) return;
    // Every edge with BOTH ends in the focus set is lit by the edge reducer -- the
    // neighbour-to-neighbour ones too, not only those touching the focus note -- so the
    // overlay walks the whole set, each edge once.
    var set = focusSet(), seen = Object.create(null);
    ctx.save();
    ctx.globalAlpha = ht;
    ctx.lineCap = "round";
    Object.keys(set).forEach(function (n) {
      graph.forEachEdge(n, function (e, attrs, s, t) {
        if (seen[e]) return;
        seen[e] = true;
        if (!set[s] || !set[t]) return;
        var geo = edgeCurveGeom(e, s, t);
        if (!geo) return;
        ctx.beginPath();
        ctx.moveTo(geo.ps.x, geo.ps.y);
        if (geo.k) ctx.quadraticCurveTo(geo.cp.x, geo.cp.y, geo.pt.x, geo.pt.y);
        else ctx.lineTo(geo.pt.x, geo.pt.y);
        ctx.lineWidth = Math.max(settings.minEdgeThickness, renderer.scaleSize(geo.ed.size || 1));
        ctx.strokeStyle = geo.ed.color;
        ctx.stroke();
      });
    });
    // The lit neighbours, once more on top of the web. Plain discs only: a halo-typed
    // note keeps its ring, and stays under the web rather than being flattened here.
    Object.keys(set).forEach(function (n) {
      if (n === f) return;
      var nd = renderer.getNodeDisplayData(n);
      if (!nd || nd.hidden || nd.type === "halo") return;
      var p = renderer.graphToViewport(graph.getNodeAttributes(n));
      ctx.beginPath();
      ctx.arc(p.x, p.y, renderer.scaleSize(nd.size), 0, 2 * Math.PI);
      ctx.fillStyle = nd.color;
      ctx.fill();
    });
    ctx.restore();
  }

  // Replaces Sigma's built-in hover label, whose pill is hardcoded to #FFF.
  // Geometry matches its label drawer: text at x + size + 3, y + labelSize/3.
  function drawHover(ctx, data, settings) {
    drawFocusWeb(ctx, data, settings);
    if (typeof data.label !== "string" || !data.label) return;
    var n = settings.labelSize;
    ctx.font = settings.labelWeight + " " + n + "px " + settings.labelFont;
    var w = ctx.measureText(data.label).width;
    var x0 = data.x + data.size, x1 = x0 + w + 9;
    var h = n + 9, y0 = data.y - h / 2, y1 = data.y + h / 2, r = 5;

    ctx.beginPath();
    ctx.moveTo(x0 + r, y0);
    ctx.lineTo(x1 - r, y0); ctx.quadraticCurveTo(x1, y0, x1, y0 + r);
    ctx.lineTo(x1, y1 - r); ctx.quadraticCurveTo(x1, y1, x1 - r, y1);
    ctx.lineTo(x0 + r, y1); ctx.quadraticCurveTo(x0, y1, x0, y1 - r);
    ctx.lineTo(x0, y0 + r); ctx.quadraticCurveTo(x0, y0, x0 + r, y0);
    ctx.closePath();
    ctx.fillStyle = THEME.hoverBg;
    ctx.fill();
    ctx.lineWidth = 1;
    ctx.strokeStyle = THEME.hoverBorder;
    ctx.stroke();

    ctx.fillStyle = THEME.text;
    ctx.fillText(data.label, data.x + data.size + 5, data.y + n / 3);
  }

  // The node's styling, independent of how far it has faded in. Visibility is
  // NOT decided here any more -- the reducer gates on opacity instead.
  function nodeStyle(id, a) {
        var r = Object.assign({}, a);
        r.color = nodeColor(id);
        // EVERYTHING about being highlighted is a function of hl[id], including whether
        // the treatment is applied at all -- not of isHighlighted(id). On the way out the
        // predicate is already false while the ramp is still above zero, so keying the
        // halo on the predicate would drop the ring on the first frame of the fade and
        // leave only a shrinking dot.
        var hv = hl[id] || 0;
        // THE PICKED DAY'S NOTES TAKE THE NEUTRAL FILL. This was gated on the sidebar's
        // "Mark today" flag, and that button is gone -- clicking the band's today column
        // marks the same notes by the same predicate, so the band is the control now and
        // the treatment has to come with it, or the band would do a weaker version of the
        // job it inherited.
        //
        // state.markDay ONLY, not isMarkedDay(). That predicate also answers for hoverDay
        // and for a hovered year, and recolouring on hover is far too loud a thing to do
        // while a pointer crosses the band -- a whole year of notes changing fill as the
        // cursor passes a label. A hover haloes; a CLICK, which is a choice, recolours.
        //
        // The colour is the extreme of the neutral axis, deliberately not one of the ten
        // group hues, so it can never be misread as a folder.
        if (state.markDay && graph.getNodeAttribute(id, "created") === state.markDay) {
          r.color = mixHex(r.color, THEME.today, hv);
          r.zIndex = 3;
        }
        // The halo is the same for both highlight sources. It is drawn in the extreme of
        // the neutral axis for the same reason the today colour is: it must not be
        // mistakable for one of the ten group hues.
        if (haloOn && hv > 0.004) {
          r.type = "halo";
          // Mixed from the note's own colour rather than faded with an alpha: the ring
          // then emerges from the dot instead of ghosting over it, and it stays a solid
          // hex, which is the only thing Sigma's colour parser is reliable about.
          r.haloColor = mixHex(nodeColor(id), THEME.today, hv);
          // 0.3 is the room the ring needs outside the dot; HL_GROW is the note actually
          // getting bigger. Both ramp together, so the dot swells and the ring arrives
          // around it as one movement.
          r.size = (r.size || a.size) * (1 + (0.3 + HL_GROW) * hv);
          r.zIndex = 4;
        }

        // Search wins: the whole result set is labelled, and nothing else is.
        if (state.query) {
          if (a.label.toLowerCase().indexOf(state.query) < 0) {
            r.color = THEME.dim; r.label = ""; r.zIndex = 0; return r;
          }
          r.zIndex = 2; r.highlighted = true; r.forceLabel = true; return r;
        }

        var focusNode = state.hovered || state.selected;
        var focus = focusSet();
        var ht = hoverAmount();
        if (focus && !focus[id]) {
          r.color = mixHex(r.color || nodeColor(id), THEME.dim, ht);
          r.label = ""; r.zIndex = 0; return r;
        }
        if (focus) r.zIndex = 2;

        if (id === focusNode) {
          r.size = (r.size || a.size) * (1 + HOVER_GROW * ht);
          // The name waits until the dim is most of the way there. Otherwise the label
          // box lands on top of notes that are still at full colour, and for the first
          // few frames it is unreadable.
          if (ht > 0.5) { r.highlighted = true; r.forceLabel = true; }
          return r;
        }
        // Nothing is permanently labelled. Notes are named on hover, on click and
        // in search results -- never by Sigma's viewport label grid, which assumes
        // nodes are spread out and is false by construction here: every hub is
        // packed into the centre, so they all compete for one grid cell and which
        // one wins is arbitrary.
        r.label = "";
        return r;
  }

  /* ------------------------------------------------------------------ logo */

  // The logo lives in the hub hole, sized as a fraction of it. Measured: the hole
  // is 90 units of radius on a 1016px stage at ratio 1.08, i.e. 180px across, so
  // 0.5 of that is ~114px on screen (it was 0.72 / ~163px, trimmed 30%). The
  // 192px asset is deliberately larger than that: at 2x DPR the same 114px box
  // wants ~228 device pixels.
  //
  // It is placed by projecting the disc centre and one point on the hole's edge
  // through the camera, rather than by assuming the stage centre: the centre is in
  // fact fixed (panning and rotation are off, and the pinned bbox is symmetric
  // about the origin, so camera 0.5,0.5 IS the disc centre) but the hole's radius
  // in PIXELS scales with the camera ratio, and hard-coding either would break on
  // zoom.
  var LOGO_OF_HOLE = 0.5;
  // A FIXED SIZE, in screen pixels. It used to be purely a fraction of the hub hole, which
  // meant it moved whenever the hole did -- and once the hub radius became something the
  // band balancer solves for, the logo started changing size with the folder layout, which
  // has nothing to do with it. Measured on the same vault across two balancer targets:
  // 160px, then 130px.
  //
  // LOGO_OF_HOLE stays as a CEILING rather than the rule. Zooming out shrinks the hole in
  // pixels, and a genuinely fixed size would then spill the mark out of the hub and under
  // the innermost notes. So: this size, or as much of it as the hole can hold.
  var LOGO_PX = 128;

  // Paint for the mask, taken from the ring itself: for each of RING_BUCKETS slices of
  // the circle, the colour of the OUTERMOST note sitting at that angle. So the logo
  // carries the same hues in the same directions as the wedges around it, and follows
  // every change to them -- palette, theme, filters, a group hidden and its neighbours
  // growing into the space -- with no second copy of the scheme to maintain.
  //
  // Sampled from positions rather than read off the plan on purpose: the plan knows
  // group spans, but what is actually on the rim at a given angle is a subfolder tint,
  // and the tints are most of what makes the disc look like itself.
  //
  // nodeColor, not the rendered display colour: hover and search deliberately dim
  // everything else, and the logo should not grey out when you search.
  var RING_BUCKETS = 144;         // 2.5 degrees each
  // Half-width of the colour blur, in buckets. 5 spreads each boundary over 11 buckets
  // (~27.5 degrees), which is wide enough that the mark reads as a wash of the disc's
  // palette rather than as wedges with soft joins. It was 2 (~12.5 degrees) first,
  // which still showed the wedge structure.
  var LOGO_BLEND_BUCKETS = 5;
  // The mean-coloured core that hides the conic gradient's centre singularity: solid
  // to CORE_SOLID% of the mark's half-width, gone by CORE_FADE%. Kept small -- past
  // ~30% the arcs are wide enough to blend on their own, and a bigger core just washes
  // the hue variation out of the middle of the mark.
  var CORE_SOLID = 9, CORE_FADE = 34;
  var lastGradient = "", lastGradientInner = "";
  var logoMaskReady = false, logoMaskImg = null;
  // Where the inner-band layer starts giving way to the outer one, as a fraction of
  // the mark's half-width. Opaque to the first stop, gone by the second -- so the
  // inner palette occupies a 40% core, not the 64% it started at, which let it take
  // over most of the mark.
  var LOGO_INNER_FADE = "16%, 40%";

  // One colour per angular bucket, shared by the CSS gradient and the PNG export so
  // the two cannot drift apart.
  function ringColors() {
    // Sample ONE band. The outer ring is what reads as "the disc" -- it is the big
    // one, it is what surrounds the mark, and its wedges are the subfolder tints worth
    // borrowing. The inner ring is six small groups packed close to the hub, so
    // including it crowded the mark with hues from an annulus barely wider than the
    // logo itself and fought the outer ring for the same angles.
    //
    // Except when the outer ring is empty -- filter down to only inner-band groups and
    // it becomes the disc, so it supplies the paint. Band membership comes from
    // bandLock, which is fixed at load, rather than from a radius test.
    var o = bandColors(false), i = bandColors(true);
    if (!o) return i || new Array(RING_BUCKETS);      // nothing on screen
    if (!i) return o;
    // Hand over CONTINUOUSLY rather than switching. `outer || inner` meant that the
    // frame the last outer note went invisible, the whole mark repainted from the
    // outer palette to the inner one in one step -- and in two-ring mode the core
    // layer was dropped at the same instant, so it jumped twice. Everything else in
    // this layout is driven by weights for exactly this reason; the logo was the last
    // discrete thing left.
    var t = outerPresence();
    if (t >= 0.999) return o;
    if (t <= 0.001) return i;
    return mixColorArrays(i, o, t);                  // t = 1 is pure outer
  }

  // How much of the outer band is on screen, as a 0..1 ramp. A ramp rather than a
  // boolean so the handover rides along with the cascade that causes it: alphas slide
  // continuously, so this does too.
  //
  // Measured as a SHARE of the band, not an absolute count. It was 12 notes' worth of
  // alpha, which on a 418-note outer band meant the ramp covered the last **2.9%** of
  // the fade -- continuous on paper, still a snap on screen. As a share, the knee sits
  // at a fixed fraction of the band however big the vault grows.
  //
  // The knee is what keeps ordinary filtering from tinting the mark: above it t is
  // pinned at 1, so hiding up to three quarters of the outer band changes nothing.
  // Below it the mark leans toward the inner palette, which is honest -- by then the
  // inner ring really is most of what is on screen.
  // ...and SMOOTHSTEPPED, not linear. A linear ramp has a kink where it meets the
  // knee -- the rate of colour change goes from nothing to full in one frame, which is
  // itself a visible event even though the value is continuous. Smoothstep is flat at
  // both ends, so the handover eases in and eases out.
  //
  // 0.5 rather than 0.25 because the ramp has to be long enough to read as a fade:
  // measured at 0.25 the last two tenths of the fade moved 92 and 93 (channel sum)
  // against a median of ~35, so the change was still piling up at the end.
  var BAND_HANDOVER = 0.5;          // share of the outer band at which the ramp starts
  function outerPresence() {
    var s = 0, n = 0;
    graph.forEachNode(function (id) {
      if (bandLock && bandLock[groupOf(id)]) return;  // inner band
      n++; s += alpha[id] || 0;
    });
    if (!n) return 0;
    var t = Math.max(0, Math.min(1, (s / n) / BAND_HANDOVER));
    return t * t * (3 - 2 * t);
  }

  function mixColorArrays(a, b, t) {
    var out = new Array(RING_BUCKETS);
    for (var i = 0; i < RING_BUCKETS; i++) {
      var x = toRgb(a[i] || "#888"), y = toRgb(b[i] || "#888");
      out[i] = "rgb(" + Math.round(x[0] + (y[0] - x[0]) * t) + "," +
                        Math.round(x[1] + (y[1] - x[1]) * t) + "," +
                        Math.round(x[2] + (y[2] - x[2]) * t) + ")";
    }
    return out;
  }

  // Raw sample of ONE band, gaps filled, or null if that band has nothing on screen.
  function bandColors(wantInner) {
    var col = new Array(RING_BUCKETS), rad = new Array(RING_BUCKETS), any = false;
    graph.forEachNode(function (id, a) {
      // present(), NOT a 0.5 cutoff. This was the last discrete step in the chain and
      // it defeated the smooth handover entirely: with a 0.5 threshold a band stops
      // being sampled the moment its last note passes half-faded, so the base snapped
      // from blended to pure-inner in one frame -- the jump between the final two
      // frames of the cascade, whatever `t` happened to be doing.
      //
      // Worth recording how this hid: every measurement of the handover set alphas to
      // exactly 0 or 1, so the threshold was never crossed and the curves looked
      // clean. A real cascade holds fractional alphas for most of its length. Test the
      // in-between states, not just the endpoints.
      if (!present(id)) return;
      if ((bandLock ? !!bandLock[groupOf(id)] : false) !== wantInner) return;
      // A PINNED NOTE IS NOT ON THE RIM, whatever its coordinates say. The r > 1e-6 guard
      // below only catches one sitting exactly at the centre; every other hub slot has a
      // direction, so a pinned note was voting for the colour of the wedge it happens to
      // point at -- from inside the hole, where it is nowhere near the outermost note.
      if (isPinned(id)) return;
      var r = Math.hypot(a.x, a.y);
      if (!(r > 1e-6)) return;                       // hub notes have no direction
      var k = Math.floor(angleSweep(Math.atan2(a.y, a.x)) / (2 * Math.PI) * RING_BUCKETS);
      k = ((k % RING_BUCKETS) + RING_BUCKETS) % RING_BUCKETS;
      if (rad[k] === undefined || r > rad[k]) { rad[k] = r; col[k] = nodeColor(id); any = true; }
    });
    if (!any) return null;

    // Gaps -- the space between wedges, and any angle with nothing on it -- inherit
    // the last colour seen going clockwise, so a wedge's hue runs right up to its
    // neighbour's and the mark has no holes in it. With one band sampled these are
    // wider than before, which the blur then turns into long soft transitions.
    var first = -1;
    for (var i = 0; i < RING_BUCKETS; i++) if (col[i]) { first = i; break; }
    var carry = col[first];
    for (var n = 0; n < RING_BUCKETS; n++) {
      var j = (first + n) % RING_BUCKETS;
      if (col[j]) carry = col[j]; else col[j] = carry;
    }
    return col;
  }

  // Softened version of the same array: a circular box blur in RGB, so a wedge's hue
  // fades into its neighbour's instead of meeting it at a hard line. Hard edges were
  // the first version and read as a pie chart pasted inside the mark -- the disc's own
  // boundaries are crisp because they are separated by gaps and sit at a distance,
  // whereas here a dozen of them are crammed into ~130px with nothing between them.
  //
  // Blurred rather than eased per boundary because it is one pass over 144 buckets
  // that handles every boundary, the seam at 0/360, and runs too narrow to hold a
  // transition, without special cases for any of them.
  function ringColorsSmooth(src) {
    var col = src || ringColors();
    if (!col || !col[0]) return col || new Array(RING_BUCKETS);
    var n = RING_BUCKETS, w = LOGO_BLEND_BUCKETS, out = new Array(n);
    for (var i = 0; i < n; i++) {
      var r = 0, g = 0, b = 0, k = 0;
      for (var d = -w; d <= w; d++) {
        var c = toRgb(col[((i + d) % n + n) % n]);
        r += c[0]; g += c[1]; b += c[2]; k++;
      }
      out[i] = "rgb(" + Math.round(r / k) + "," + Math.round(g / k) + "," + Math.round(b / k) + ")";
    }
    return out;
  }

  function ringGradient(src) {
    var col = ringColorsSmooth(src);
    if (!col || !col[0]) return "";

    // ONE position per bucket, at its centre, rather than a start/end pair. A pair
    // pins the colour flat across the whole bucket and steps at the join; a single
    // position lets the browser interpolate from each centre to the next, which is
    // the other half of the softening.
    var step = 360 / RING_BUCKETS, stops = [];
    for (var i = 0; i < RING_BUCKETS; i++) {
      var prev = col[(i - 1 + RING_BUCKETS) % RING_BUCKETS];
      var next = col[(i + 1) % RING_BUCKETS];
      // Skip only the INTERIOR of a flat run -- both of its ends have to be stated.
      // Dropping every repeat instead left one stop at the start of a run and the
      // next at the start of the following one, so the browser ramped across the
      // whole wedge: measured, a colour change spread over 47.5 degrees, which turns
      // every wedge into a smear rather than a plateau with soft joins.
      if (col[i] === prev && col[i] === next) continue;
      stops.push(col[i] + " " + ((i + 0.5) * step).toFixed(2) + "deg");
    }
    // Close the circle explicitly. The first and last bucket centres sit half a step
    // in from 0 and 360, so without these the seam is held flat at each end; the
    // blur already makes the two nearly equal, and this makes them exactly so.
    var seam = (function () {
      var a = toRgb(col[RING_BUCKETS - 1]), b = toRgb(col[0]);
      return "rgb(" + Math.round((a[0] + b[0]) / 2) + "," + Math.round((a[1] + b[1]) / 2) +
             "," + Math.round((a[2] + b[2]) / 2) + ")";
    })();
    stops.unshift(seam + " 0deg");
    stops.push(seam + " 360deg");
    // `from 0deg` is 12 o'clock running clockwise, which is exactly the sweep the disc
    // is laid out in -- so the mark's hues line up with the wedges behind it.
    var conic = "conic-gradient(from 0deg at 50% 50%, " + stops.join(", ") + ")";

    // A CORE of the palette's own mean, laid OVER the conic gradient.
    //
    // A conic gradient has a singularity at its centre: every angular stop converges
    // on one point, so a transition that is 29px wide out at the rim is 0px wide in
    // the middle. That is why the mark had a hard seam straight down the fissure no
    // matter how much angular blending was added -- the blend is measured in degrees
    // and degrees are worth nothing at r=0.
    //
    // Angle cannot help here, so the centre stops using it: it fades to the average of
    // all the buckets, which is the one colour that cannot clash with whatever meets
    // there. Radial, so the fix is strongest exactly where the problem is and gone by
    // the time the arcs are wide enough to blend on their own.
    var m = [0, 0, 0], k = 0;
    for (var q = 0; q < RING_BUCKETS; q++) {
      var c = toRgb(col[q]); m[0] += c[0]; m[1] += c[1]; m[2] += c[2]; k++;
    }
    k = Math.max(1, k);
    var mean = "rgba(" + Math.round(m[0] / k) + "," + Math.round(m[1] / k) + "," +
               Math.round(m[2] / k) + ",";
    var core = "radial-gradient(circle at 50% 50%, " +
      mean + "1) 0%, " + mean + "0.92) " + CORE_SOLID + "%, " +
      mean + "0) " + CORE_FADE + "%)";
    // Core first = core on top.
    return core + ", " + conic;
  }

  function placeLogo() {
    var el = $("logo");
    if (!el || !logoMaskReady || !renderer || !geomLock) return;
    // THE MARK YIELDS TO THE NOTES. One pin is enough: a brain with a note sitting on top
    // of it is worse than either alone, and the hub can hold one thing at a time. Unpin
    // the last and it comes back.
    //
    // Ghosting it instead, so it stays as a container behind the notes, was spiked and
    // dropped -- the mark is painted UNDER sigma's canvases and the hub is where every
    // edge converges, so anything faint enough to read as a container is not visible at
    // all. Measured at 0.26 and again at 0.55; neither survived the edge tangle.
    // FADED, NOT HIDDEN, and the rest of the function still runs. Setting `hidden` popped
    // the mark out on the frame the first pin landed, while the note it yielded to was
    // still crossing the disc -- the one hard cut in an otherwise tweened change. Opacity
    // is transitioned in page.css; the mark keeps being sized and positioned underneath so
    // it has nowhere to jump to when it comes back.
    //
    // Ghosting it PERMANENTLY, so it stays as a container behind the notes, is a different
    // idea and was spiked and dropped: the mark is painted UNDER sigma's canvases and the
    // hub is where every edge converges, so anything faint enough to read as a container is
    // not visible at all. Measured at 0.26 and again at 0.55; neither survived the tangle.
    // pinnedIds(), NOT state.pinned -- a filter can hide every pin without releasing any
    // of them (see pinnedIds), and hubPlace draws nothing for a hub in that state. Yielding
    // on the raw list left the mark faded with an empty hole under it: hiding the one
    // folder a pin lived in emptied the hub and the mark never came back to fill it.
    var yielded = pinnedIds().length > 0;
    el.style.opacity = yielded ? "0" : "";
    // Re-paint only when the ring's colours actually changed. This runs from
    // afterRender, so it fires on every frame of a cascade -- and assigning the same
    // background string 90 times would be 90 style recalculations for nothing.
    // The base layer is ringColors() in both modes -- the outer band, handing over to
    // the inner one as the outer empties. The core layer, in two-ring mode, is the
    // inner band on its own.
    var two = state.logoTwoRing;
    var g = ringGradient();
    var inner = two ? bandColors(true) : null;
    // Shown whenever the inner band has anything on screen. It used to also require a
    // non-empty outer band, on the grounds that the core would otherwise duplicate the
    // base -- but that dropped the layer on the exact frame the base finished handing
    // over to the inner palette, which is a second jump on top of the first. Letting
    // it duplicate is harmless: both layers converge on the same colours, so the mark
    // simply becomes uniformly inner-coloured with nothing popping.
    var gi = (two && inner) ? ringGradient(inner) : "";
    if (g && g !== lastGradient) { lastGradient = g; el.style.background = g; }
    var eli = $("logoInner");
    if (eli) {
      if (gi) {
        if (gi !== lastGradientInner) { lastGradientInner = gi; eli.style.background = gi; }
      } else if (lastGradientInner) { lastGradientInner = ""; }
      eli.hidden = !gi;
      eli.style.opacity = yielded ? "0" : "";   // fades with the layer beneath it
    }
    var c = renderer.graphToViewport({ x: 0, y: 0 });
    // TIMES INNER_SCALE -- see inHubHole. LOGO_OF_HOLE already caps the mark well inside
    // this radius, but the radius itself has to be where the ring actually starts, not
    // the lattice's nominal r0 -- otherwise the logo is sized against a hole bigger than
    // the one really there and can end up smaller than it should be relative to a ring
    // that has, in practice, drawn closer in than this measured.
    var edge = renderer.graphToViewport({ x: geomLock.r0 * INNER_SCALE * UNIT, y: 0 });
    var holePx = Math.hypot(edge.x - c.x, edge.y - c.y);
    var size = Math.max(24, Math.min(LOGO_PX, holePx * 2 * LOGO_OF_HOLE));
    el.style.width = size + "px";
    el.style.height = size + "px";
    el.style.left = c.x + "px";
    el.style.top = c.y + "px";
    el.hidden = false;
    if (eli && gi) {
      eli.style.width = size + "px";
      eli.style.height = size + "px";
      eli.style.left = c.x + "px";
      eli.style.top = c.y + "px";
    }
  }

  /* ------------------------------------------------------------ node sizes */

  // Node sizes are SCREEN pixels and are fixed at graph build; the disc's pixel
  // radius is not, because the pinned bbox is mapped to the stage, so a shorter
  // window draws a smaller disc with the same size dots. The NODE_MAX cap of 11 was
  // tuned against a measured ~28px row pitch on a 1068x1270 stage -- and on a 1080p
  // screen the stage is barely 720px tall, where the same disc packs its rows 22.6px
  // apart. Measured there: 3 touching pairs, worst overlap 9.8px. The cap was doing
  // its job; the thing it was calibrated against had moved.
  //
  // So the sizes are scaled by the pitch actually on screen, which restores the
  // tuned radius-to-pitch relationship at any window size. Capped at 1 so a large
  // screen is untouched (measured 30.3px there, i.e. already above the reference),
  // and floored so dots cannot vanish. Zooming in raises the pitch and returns the
  // scale to 1, which is the right behaviour: zoom is how you get detail back.
  // A FLOOR ON THE SCALE IS A FLOOR ON OVERLAP, and that is what it turned into.
  //
  // Scaling every size by pitch/REF_PITCH keeps the tuned radius-to-pitch relationship, so
  // dots never outgrow the lattice -- but only while the scale is allowed to fall. The floor
  // was there so dots could not vanish, and it was set against vaults whose rows land 22-30px
  // apart. A 10,000-note disc fitted to the stage puts its rows 4.5px apart, where the honest
  // scale is 0.16 and the floor holds it at 0.45 -- so dots draw 2.8x too big for the lattice.
  // Measured there: the biggest dot 175 units of RADIUS against a 160-unit row pitch, 349
  // across, and the MEDIAN dot 192 across. At those sizes overlap is not a tuning problem,
  // it is arithmetic, and it was also what made wedges interleave along their boundaries.
  //
  // So the two ends are handled separately, because they want opposite things. The BIGGEST dot
  // is capped at a fraction of the pitch, which is the relationship that has to hold or notes
  // collide. The SMALLEST is floored in pixels, which is the one that has to hold or notes
  // disappear. In between it is linear, so the degree ordering survives wherever there is room
  // to show it -- and where there is not, the range simply flattens rather than overflowing.
  //
  // DOT_OF_PITCH is 11/28: the cap and reference pitch that were already tuned together, now
  // expressed as the ratio they always were. At the reference pitch this reproduces today's
  // sizes exactly; below it, it keeps doing what the multiplier was meant to.
  var REF_PITCH = 28;
  // NO FLOOR AND NO CEILING ON A MULTIPLIER, because the multiplier is gone. github#13
  // raised the ceiling from 1 to 2.4 for a good reason -- a filtered disc genuinely has more
  // room per note, and clamping at 1 kept the median dot at 4.2px while its neighbours' room
  // nearly tripled -- but a clamp at either end is a knee, and a knee is what dots growing as
  // you zoom in and then shrinking again actually is. Measured across a 21x zoom, diameter
  // over pitch: 0.786 flat below the old knee, then 0.55, 0.367, 0.092 above it.
  //
  // Both ends are now separate numbers with separate jobs, and neither is a clamp on the
  // other: the biggest dot is a fraction of the pitch (or notes collide), the smallest is a
  // floor in PIXELS (or notes vanish), and the range between them is linear. The room a
  // filtered disc gains is picked up per note by dotFit, which measures it rather than
  // inferring it from a ratio of counts.
  var DOT_OF_PITCH = 11 / 28;   // biggest dot RADIUS, as a fraction of the lattice pitch
  var DOT_MIN_PX = 1.5;         // and the smallest is still a dot
  // How far a dot may grow with a spreading lattice before it stops following it. 1.6 of a
  // normal row's worth: enough that a filtered disc reads as bigger dots, short of the point
  // where two of them are most of the space between their neighbours.
  // Up from 1.6, which was set when a filtered band's dots became blobs -- and most of that
  // was the inner band wearing the OUTER band's ramp, since there was only one. With a ramp per
  // band, a dot can follow its own lattice all the way to the spacing cap, which is what keeps
  // the proportion the design is stated in. Measured before: a 96-of-454 range spread the outer
  // pitch to 2.5 while the cap held dots at 1.6, so diameter over step came out 0.49 at the
  // largest dot against a design of 0.786 -- 0.786 * (1.6 / 2.5) exactly. Reported as gaps
  // between the seam and the edge notes, which is what a sparse ring of undersized dots is.
  var DOT_MAX_SPREAD = DENSITY_MAX;
  // How far a dot may outgrow its band's pitch on the strength of the room it measures. The
  // shrink direction is unbounded on purpose -- a crowded note gives up whatever it must.
  //
  // THE SAME FACTOR THE LATTICE MAY SPREAD BY, and it has to be, because a ring of fixed
  // diameter holding few notes spreads TANGENTIALLY without limit while its radial pitch is
  // capped at DENSITY_MAX. Measured on a 22-of-454 range: pitch 416 units, step 1046 -- 2.5x --
  // so a 1.6 ceiling drew dots at diameter/step 0.26 against a design of 0.786, and the ring
  // read as nine small dots adrift on a circle. Set to the spacing cap, the growth a dot is
  // allowed and the spread the lattice is allowed are one number instead of two that disagree.
  var DOT_ROOM_MAX = DENSITY_MAX;
  // display px = ramp.m * attr size + ramp.b, never below ramp.lo, per band. See BAND.
  var sizeScale = 1;      // kept for the probe and the report; = the outer ramp at NODE_MAX

  // Whether the border program actually loaded. If the bundle ever ships without it,
  // highlighting still works -- the radial push carries it on its own -- rather than
  // asking Sigma for a node type it cannot draw.
  var haloOn = !!RENDERING.createNodeBorderProgram;

  function measureSizeScale() {
    if (!renderer) return sizeScale;
    // One row of spacing is lastSP LATTICE units, i.e. lastSP * UNIT graph units. That
    // distinction is the fix: measuring one lattice unit answered a question about the
    // camera, and the question here is how much room a note has next to its neighbour.
    var a = renderer.graphToViewport({ x: 0, y: 0 });
    var b = renderer.graphToViewport({ x: UNIT * (bandOf("o").sp || 1), y: 0 });
    var pitch = Math.hypot(b.x - a.x, b.y - a.y);
    if (!(pitch > 0)) return sizeScale;
    // TIMES THE CAMERA RATIO, because sigma now scales what we hand it by 1/ratio (see
    // zoomToSizeRatioFunction). pitch * ratio is the px-per-row at ratio 1 -- a property of the
    // stage and the normalisation box, not of the zoom -- so `hi` below comes out the same
    // number at every zoom and the dot keeps its proportion to the lattice. Without this the
    // two scalings compound and the dots grow as 1/ratio squared.
    var cam = renderer.getCamera().getState().ratio || 1;
    pitch *= cam;
    // The two ends, then the line between them.
    //
    // NO PIXEL CEILING ON THE TOP END, and taking one out is what fixed the strangest report
    // of the lot: dots that grew as you zoomed in and then shrank again. NODE_MAX is 11 and
    // was a PIXEL cap, tuned when `size` meant pixels -- but the rule now is that a dot is a
    // fraction of the row pitch, and the two disagree the moment the pitch passes 28px. Below
    // the knee the dots tracked the lattice; above it they froze at 11px while the lattice kept
    // growing, so they read as swelling and then dwindling. Measured, diameter over pitch:
    // 0.786, 0.786, 0.786, then 0.55, 0.367, 0.092 as the camera closed in.
    //
    // Unbounded is the honest answer: zoomed far enough in, one note SHOULD fill the view.
    // NODE_MAX stays as the top of the attribute range the line below maps from.
    // CAPPED IN ABSOLUTE TERMS TOO. DOT_OF_PITCH of the live pitch is the relationship that
    // has to hold -- a dot against the space it has -- but a heavily filtered band spreads its
    // pitch by up to DENSITY_MAX, and a dot riding that is a blob. The ratio is
    // preserved right up to the cap and then held, which reads as a sparse ring of ordinary
    // dots rather than a handful of balloons.
    // ONE RAMP PER BAND, because there is one pitch per band and a dot is a fraction of its
    // OWN pitch. Calibrated from lastSP alone, the whole disc was sized by the OUTER ring:
    // measured on a 96-of-454 date range, the outer pitch was 400 units and the inner 206, and
    // the same dots went into both -- worst pair clearance 239 in the outer band against 29 in
    // the inner. Reported exactly that way round, gaps at the outer seams and notes touching on
    // the inner ring, which is one bug seen from both ends.
    //
    // The floor is the one thing that IS about pixels on screen -- a dot has to stay visible --
    // so it is converted the other way, from px to the size units sigma will divide by ratio.
    var rampFor = function (units) {
      var bb = renderer.graphToViewport({ x: units, y: 0 });
      var pit = Math.hypot(bb.x - a.x, bb.y - a.y) * cam;
      var hi = DOT_OF_PITCH * pit;
      var hiCap = DOT_OF_PITCH * UNIT * DOT_MAX_SPREAD * cam;
      if (hi > hiCap) hi = hiCap;
      var lo = Math.min(hi, DOT_MIN_PX * cam);
      return { m: (hi - lo) / Math.max(1e-6, NODE_MAX - NODE_MIN),
               b: lo - (hi - lo) / Math.max(1e-6, NODE_MAX - NODE_MIN) * NODE_MIN,
               lo: lo, hi: hi };
    };
    var ro = rampFor(UNIT * (bandOf("o").sp || 1) * bandScale("o"));
    var ri = rampFor(UNIT * (bandOf("i").sp || 1) * bandScale("i"));
    bandOf("o").ramp = ro; bandOf("i").ramp = ri;
    return ro.hi / NODE_MAX;   // what the old multiplier would have been at the top end
  }

  // A dot's radius in display pixels. One place, so the renderer, the label placer and any
  // measurement all agree about how big a dot is.
  //
  // AND CAPPED BY THE ROOM THIS NOTE HAS. The mapping above sizes against the row pitch; a
  // note whose row neighbours sit closer than that gets scaled down to keep the same
  // proportion to the gap it actually has. Both directions matter and the nearer one wins,
  // which is what dotFit already holds.
  function dotPx(size, id) {
    // Which band this note is in, so both the ramp and the room it is measured against are
    // the ones that belong to it.
    var isIn = id !== undefined && bandLock && !!bandLock[groupOf(id)];
    var rp = bandOf(isIn ? "i" : "o").ramp;
    var v = rp.m * (size || 4) + rp.b;
    var scale = 1;
    if (id !== undefined) {
      // THE BAND'S ROOM, NOT THIS NOTE'S. Sizing each note against its own neighbours makes
      // size a function of position as well as of link weight, and the two disagree: notes are
      // laid down in weight order from the inside out, so the innermost note is the most
      // connected one -- and the innermost row has the shortest arc, therefore the least room,
      // therefore the smallest dot. The most connected note came out smaller than its
      // neighbours and the graph read backwards, which is what a graph must not do.
      //
      // One figure per band, the tightest pair in it, makes size strictly monotone in link
      // weight: the ordering is the whole point of the encoding and nothing local may perturb
      // it. Taking the MINIMUM is what keeps that safe -- the biggest dot in the band is sized
      // to fit the closest pair in it, so nothing can touch. The price is that one crowded pair
      // sets the size for its whole band, which is the honest trade for an encoding that can be
      // read.
      var room = bandOf(isIn ? "i" : "o").room;
      var mine = cellRoom[id];
      // A RAMPED CELL'S ROOM RAMPS WITH IT. cellRoom is walked between the two endpoint
      // packings, which know nothing about the arc ramp -- so a wedge held narrow by the ramp
      // kept handing its notes the room of a wedge that is not there, and two of a three-note
      // column overlapped by 24 units at 83% of a show. The tangential step inside a cell is
      // proportional to the cell's arc, so the same factor applies to both. Through cellRoom
      // rather than as a size multiplier: this is the per-CELL cap, which is stable, and not
      // the per-NOTE one, which is the thing that made every dot in the vault breathe.
      if (colWalk) {
        var cwd = colWalk[groupOf(id)];
        // Falling back to the BAND's room when the cell has none of its own: cellRoom is only
        // filled for cells whose measured step exceeds a unit, so a small folder's cell is
        // usually absent from it -- and ramping an absent number is a no-op, which is exactly
        // what the first attempt at this measured (28 overlapping frames, unchanged).
        if (cwd !== undefined) mine = (mine === undefined ? room : mine) * cwd.f;
      }
      if (mine !== undefined && mine > 1 && (!(room > 1) || mine < room)) room = mine;
      // AND A HAIR UNDER, because both figures above are AVERAGES over a row and the notes in a
      // row are not evenly spaced. Position within a row is weight-based -- a note's own share
      // of its row's weight -- so a light note beside a heavy one sits closer than the row's
      // mean step, and no average can see that. The exact bound is each note's own local gap,
      // which is dotFit, and dotFit is a minimum over WHICH neighbour is nearest: it changes as
      // the disc moves, and using it made every dot in the vault breathe (252% in one frame).
      //
      // 8% off an average is cheaper than that, and it is enough: the residual was one pair at
      // -5 units on a ~159 step, 3%. Anything materially worse than local weight variation would
      // not be covered by this and should not be -- it would be a real defect.
      room *= 0.92;
      // AND NOTHING PER NOTE. A cap taken from this note's own measured room was tried, to stop
      // the tightest pairs touching, and it is the thing that made dots breathe: dotFit is
      // measured off live positions, and a note's NEAREST NEIGHBOUR is not a fixed neighbour --
      // it changes as the disc moves and as notes arrive, so a minimum over it jumps for
      // reasons that have nothing to do with the note. Measured directly, with the cap the
      // worst single-frame size change was 252% and 72 of 122 frames moved more than 5%;
      // without it, 2.2% and none. It also broke the ordering it was bolted onto, which is the
      // property the encoding exists for. Both requirements point the same way.
      var pit = pitchUnits(isIn ? "i" : "o");
      // BOTH WAYS. This only ever shrank a dot, which is half a rule: the ramp sizes a note
      // against the RADIAL pitch, dotFit measures the room it has ALONG its row, and those two
      // are the same number only when a row is exactly full. They usually are not. Measured on
      // a 96-of-454 range: pitch 400 units, step 640, so every note had 1.6x the room its size
      // was drawn from -- diameter over step 0.49 against a design of 0.786, which reads as a
      // sparse ring with gaps at every seam and was reported as exactly that. Letting the same
      // ratio grow a dot as well as shrink it restores the proportion the design is stated in,
      // in every filter state rather than only in the full one.
      //
      // Bounded, because room is unbounded: beside a hole a note would otherwise inflate to
      // fill it, which trades a gap for a balloon and hides the hole rather than fixing it.
      if (room !== undefined && pit > 1e-9) {
        var f = room / pit;
        if (f > DOT_ROOM_MAX) f = DOT_ROOM_MAX;
        v *= f;
        scale = f;
      }
    }
    // AND THE FLOOR SCALES WITH THE ROOM. This was the last overlap in the vault and it was one
    // line: the room factor multiplied the value and not the floor, so wherever a note had LESS
    // room than a pitch the dot shrank and the floor did not, and the floor won.
    //
    // It only shows in a small window, which is why it survived every measurement here until a
    // check ran in a tiled one. The floor is the one quantity in the ramp denominated in
    // PIXELS -- a dot has to stay visible -- and a fixed number of pixels is a large number of
    // graph units on a small stage. Measured at 1280x635 with a folder hidden: six overlapping
    // pairs, every single dot in them at radius exactly 45 units, which is 1.5px there, against
    // arcs of 45 to 88.
    //
    // A dot that cannot be both visible and separate is drawn separate. Sub-pixel is faint and
    // honest; overlapping is a smear that says two notes are one.
    var lo = (rp.lo || DOT_MIN_PX) * scale;
    if (v < lo) v = lo;
    // AND NEVER PAST ITS OWN WEDGE EDGE. Last, so it outranks the pixel floor too: a dot that
    // cannot be both visible and inside its wedge is drawn inside it. The cap is in graph
    // units and v is in the size units sigma divides by the camera ratio, so it converts
    // through the same pitch pair the ramp was built from.
    var capU = edgeCap[id];
    if (capU !== undefined && capU > 0) {
      var pitU = pitchUnits(isIn ? "i" : "o");
      var hiU = DOT_OF_PITCH * pitU;                 // what v = ramp top means, in graph units
      if (hiU > 1e-6) {
        var capV = rp.m * NODE_MAX + rp.b;
        var vMax = capV * (capU / hiU);
        if (v > vMax) v = vMax;
      }
    }
    return v;
  }

  // Returns true if it moved enough to be worth a repaint. The threshold is what
  // keeps this from oscillating when it is driven from render-time events.
  function syncSizeScale() {
    var next = measureSizeScale();
    if (Math.abs(next - sizeScale) < 0.01) return false;
    sizeScale = next;
    return true;
  }

  function refreshSizeScale() {
    if (syncSizeScale() && renderer) renderer.refresh();
  }

  /* -------------------------------------------------------- edge curvature */

  // Bow a link AWAY from the disc centre rather than letting it chord straight
  // across. Measured on this vault only 9% of links stay inside one folder, so
  // most of the 1419 of them join notes in different wedges -- and two rim notes
  // on opposite sides are joined by a line through the hub. At this count that is
  // a flat grey wash over the middle of the disc, which is exactly where the
  // inner ring lives.
  //
  // The bow is scaled by how close the chord would have passed to the centre, so
  // a link between neighbours in one wedge stays almost straight and only the
  // long cross-disc ones sweep round. Uniform curvature was tried first and reads
  // worse: it bends the short local links, which are the ones whose straightness
  // carries the "these two notes are near each other" signal.
  var CURVE_MIN = 0.05, CURVE_MAX = 0.55;

  function discR() {
    return geomLock && geomLock.maxR ? geomLock.maxR * UNIT : 1;
  }

  function curvatureFor(s, t) {
    var dx = t.x - s.x, dy = t.y - s.y;
    var len = Math.sqrt(dx * dx + dy * dy);
    if (!len) return CURVE_MIN;
    // Perpendicular distance from the disc centre to the chord's LINE: a link
    // aimed through the hub scores ~0, one hugging the rim scores ~R.
    var h = Math.abs(s.x * t.y - s.y * t.x) / len;
    var near = 1 - Math.min(1, h / (discR() * 0.5));
    // Squared, so the ramp stays near CURVE_MIN for most links and only the ones
    // genuinely crossing the hub get the full bow.
    var mag = CURVE_MIN + (CURVE_MAX - CURVE_MIN) * near * near;
    // Sign picks the side to bow toward. Sigma bows to the +90deg side of
    // source->target, so test that normal against the chord's midpoint: if they
    // agree the bow points away from the centre, which is the one we want.
    var out = (-dy / len) * (s.x + t.x) / 2 + (dx / len) * (s.y + t.y) / 2;
    return out >= 0 ? mag : -mag;
  }

  function makeRenderer() {
    renderer = new SigmaCls(graph, $("graph"), {
      allowInvalidContainer: true,
      renderLabels: true,
      labelRenderedSizeThreshold: 11,
      labelDensity: 0.2,
      labelGridCellSize: 150,
      labelFont: 'ui-sans-serif, "Segoe UI", system-ui, sans-serif',
      labelSize: 11,
      labelWeight: "500",
      // Sigma defaults these to black-on-anything and a hardcoded #FFF hover pill,
      // which is illegible / jarring on the dark surface.
      labelColor: { color: THEME.text },
      defaultDrawNodeHover: drawHover,
      zIndex: true,
      // SIZES SCALE WITH THE LATTICE, NOT WITH ITS SQUARE ROOT.
      //
      // Sigma's default zoomToSizeRatioFunction is Math.sqrt, so a node's drawn radius goes as
      // 1/sqrt(ratio) while its POSITION goes as 1/ratio. The two therefore diverge on every
      // zoom, and no choice of size can hold a dot at a fixed fraction of the gap to its
      // neighbour. Measured, drawn diameter over row pitch: 0.76 at rest, 1.44 three notches
      // in, 3.51 at the far end -- dots that end up swallowing their neighbours, which is
      // exactly what "they still touch when zooming in" was.
      //
      // Identity makes the multiplier 1/ratio, the same law the positions follow, so the
      // relationship between a dot and the space it has is the one thing that does NOT change
      // as the camera moves. The sizes fed in are then pinned to the lattice once (see
      // measureSizeScale) rather than re-derived per frame.
      zoomToSizeRatioFunction: function (x) { return x; },
      minCameraRatio: 0.02,
      maxCameraRatio: 12,
      // PANNING IS ON, and the centre lock that used to fight it is gone.
      //
      // The disc was pinned to the middle of the stage on the reasoning that it is the whole
      // point of the view, with a camera listener that put x and y back to 0.5 after every
      // update. That is defensible while the only camera gesture is zoom -- but it also
      // makes zoom-toward-pointer a lie, since the camera is dragged back the moment it
      // moves, so zooming in on one wedge walks it off the far edge instead. Panning plus a
      // reset is the ordinary answer, and it costs nothing that a reset does not give back.
      //
      // Rotation stays off: the wedge labels and the heatmap's day rows both assume up is up.
      //
      // The initial value is the SETTING rather than a literal: the host may have persisted
      // it off, and starting on and correcting afterwards would let one drag through before
      // the lock arrived.
      enableCameraPanning: panEnabled,
      enableCameraRotation: false,
      enableCameraZooming: true,
      // ONE WHEEL NOTCH WAS 70%. Sigma's default zoomingRatio is 1.7, so every notch
      // multiplied or divided the ratio by that -- three notches and the disc has gone from
      // filling the stage to a sixth of it. 1.2 is about 32 notches across the whole
      // 0.02..12 range, which is a scroll rather than a teleport.
      //
      // The animation is shortened with it. 250ms per notch is fine at 70% and lags visibly
      // at 20%, because the next notch arrives before the last one has landed.
      zoomingRatio: 1.2,
      zoomDuration: 120,
      defaultEdgeType: "line",
      // Both programs are registered up front so the toggle is a per-edge `type`
      // in the reducer rather than a renderer rebuild. Sigma merges these with its
      // own defaults, so "line" survives. NB the programs live on
      // Sigma.rendering, NOT on Sigma -- getting that wrong makes a program
      // silently never appear, with no error anywhere.
      edgeProgramClasses: RENDERING.EdgeCurveProgram
        ? { curve: RENDERING.EdgeCurveProgram } : {},
      // A ring around highlighted notes. borders[0] is the OUTER band and the
      // {fill:true} entry is the CORE -- the reverse of what the option order
      // suggests, which is worth stating because getting it backwards silently
      // draws a solid blob in the halo colour.
      nodeProgramClasses: RENDERING.createNodeBorderProgram ? {
        halo: RENDERING.createNodeBorderProgram({
          borders: [
            { size: { value: 0.26 }, color: { attribute: "haloColor" } },
            { size: { fill: true }, color: { attribute: "color" } },
          ],
        }),
      } : {},
      enableEdgeEvents: false,
      // Opacity is applied here, once, rather than at each of nodeStyle's five
      // exits. Everything below 0.004 is genuinely hidden, so a fully faded note
      // costs nothing to render and drops out of hit-testing.
      nodeReducer: function (id, a) {
        var al = alpha[id] || 0;
        if (al <= 0.004) { var h = Object.assign({}, a); h.hidden = true; return h; }
        var r = nodeStyle(id, a);
        if (al < 0.999) {
          // Fade AND grow: opacity alone reads as a colour change, while a note
          // that also scales up reads as arriving. Labels wait until it is
          // mostly there, so the cascade is not a wall of half-legible text.
          r.color = withAlpha(r.color || a.color, al);
          r.size = (r.size || a.size) * (0.45 + 0.55 * al);
          if (al < 0.62) { r.label = ""; r.forceLabel = false; r.highlighted = false; }
        }
        // A RAMPED WEDGE'S NOTES SHRINK WITH IT. The reporter's ideal case is the one-note
        // folder: "the wedge continuously gets smaller, the note as well, and it vanishes".
        // Without this the dots keep their resting size inside a wedge that is closing under
        // them, and two of a three-note column interpenetrate by 35 units for 36 frames --
        // measured on the show, where the stretched fades leave two notes standing in a
        // half-open wedge. Size, not position: the group clock may scale a dot, never move a
        // note -- that distinction is what the two reverts were about.
        if (colWalk) {
          var cwr = colWalk[groupOf(id)];
          if (cwr !== undefined) {
            r.size = Math.max(0.05, (r.size === undefined ? base : r.size) * cwr.f);
          }
        }
        // Applied last, so it carries the arrival ramp above as well as the resting size --
        // a fading note should grow toward the size it will actually hold. The ramp is a
        // RATIO against the attribute size, so it survives the switch from a multiplier to a
        // mapping: work out what fraction the ramp asked for, then take that fraction of the
        // size this dot is entitled to.
        var base = a.size || 4;
        r.size = dotPx(base, id) * ((r.size === undefined ? base : r.size) / base);
        // A pinned note is bigger, because it stands where the mark did. Applied after the
        // dotPx mapping, so it scales the size this dot is actually entitled to. Colour is
        // untouched -- it keeps its group's hue, which is how the hub goes on being
        // coloured by the ring with no sampler at all.
        //
        // NO forced label: it names itself on hover like every other note in the disc.
        // Forcing them on was tried and was a mistake -- it made the hub illegible past
        // three, and the pile-up then got mistaken for a capacity limit.
        if (isPinned(id)) {
          r.size = (r.size || a.size) * hubSizeMult();
          r.zIndex = 3;
        }
        return r;
      },
      edgeReducer: function (id, a) {
        var r = Object.assign({}, a);
        var x = graph.extremities(id);
        var al = Math.min(alpha[x[0]] || 0, alpha[x[1]] || 0);
        if (al <= 0.004) { r.hidden = true; return r; }
        if (state.curveEdges && RENDERING.EdgeCurveProgram) {
          r.type = "curve";
          r.curvature = curvatureFor(graph.getNodeAttributes(x[0]),
                                     graph.getNodeAttributes(x[1]));
        }
        r.color = THEME.edge;
        // codanna: the pixel clamp (see EDGE_MAX_PX above). Applied to the resting size
        // here and re-applied after the focus branch may lift r.size.
        var edgeCap = EDGE_MAX_PX * ((renderer && renderer.getCamera().getState().ratio) || 1);
        if ((r.size || 1) > edgeCap) r.size = edgeCap;
        var focus = focusSet();
        if (state.query) { r.color = THEME.dim; return r; }
        if (focus) {
          // In step with the nodes, off the same hoverT -- the web separating from the
          // rest is most of what makes a hover legible, so it cannot lag behind it.
          var ht = hoverAmount(), base = a.size || 1;
          if (focus[x[0]] && focus[x[1]]) {
            r.color = mixHex(THEME.edge, THEME.edgeHi, ht);
            r.size = Math.min(base + (1.4 - base) * ht, edgeCap);   /* codanna: pixel clamp */
            r.zIndex = 2;
          } else {
            r.color = mixHex(THEME.edge, THEME.dim, ht);
            r.zIndex = 0;
          }
        }
        // Squared, so links lag their notes: the dots land first and the web
        // draws itself in behind them rather than everything arriving at once.
        if (al < 0.999) r.color = withAlpha(r.color, al * al);
        return r;
      }
    });

    // The centre lock is gone -- see enableCameraPanning above. What it was bundled with is
    // not: a camera change moves the hub hole in screen pixels, so the logo has to be
    // re-placed and the row pitch re-measured whatever moved the camera.
    (function () {
      var cam = renderer.getCamera();
      // codanna: the edge pixel clamp (EDGE_MAX_PX) reads the camera ratio inside the
      // edge reducer, and reducers only re-run on refresh -- so while the clamp binds,
      // a zoom must schedule one. rAF-throttled, skipIndexation (nothing moved), and
      // skipped entirely while the camera stays in the region where the clamp cannot
      // bind, which is every resting zoom level.
      var edgeCapWas = false, edgeCapRaf = 0;
      cam.on("updated", function (st) {
        placeLogo(); refreshSizeScale();
        var binds = edgeCapBinds(st.ratio);
        if ((binds || edgeCapWas) && !edgeCapRaf) {
          edgeCapRaf = WIN.requestAnimationFrame(function () {
            edgeCapRaf = 0;
            renderer.refresh({ skipIndexation: true });
          });
        }
        edgeCapWas = binds;
      });
    })();

    // A window resize changes the disc's pixel radius without touching the camera,
    // so it needs its own hook. Debounced, because a drag-resize fires continuously
    // and each change costs a full refresh.
    var rzTimer = null;
    // OBSERVE THE CONTAINER. A window resize listener is right for a page that fills the
    // window and wrong for a view inside an app: dragging an Obsidian sidebar resizes this
    // element without resizing the window, and closing a pane resizes it the other way.
    // Watching the element covers both, and still fires on a window resize, because the
    // container is sized from the window in the standalone.
    var onResize = function () {
      if (rzTimer) WIN.clearTimeout(rzTimer);
      rzTimer = WIN.setTimeout(function () { rzTimer = null; refreshSizeScale(); placeLogo(); }, 120);
    };
    if (window.ResizeObserver) new ResizeObserver(onResize).observe(root);
    else window.addEventListener("resize", onResize);

    // afterRender covers the cases a camera event does not: the first paint, a
    // container resize (Sigma re-reads its dimensions and renders without the camera
    // changing at all), and -- the one that actually bit -- the END of fit()'s 380ms
    // camera animation. Measuring the row pitch straight after calling fit() reads
    // the pre-animation ratio, which pinned sizeScale to its floor and drew every
    // dot at 4.95px instead of 8.9px.
    //
    // Safe against a repaint loop because syncSizeScale only reports a change worth
    // more than 0.01: change -> refresh -> afterRender -> no change -> stop.
    renderer.on("afterRender", function () {
      if (DBG.on) drawWedgeDebug();
      placeLogo(); refreshSizeScale(); heatDraw(); hlSync();
      placeHubDrop();                            // the hub's drop ring, while dragging
    });

    renderer.on("enterNode", function (e) {
      state.hovered = e.node;
      syncLazyEdges();
      showTip(e.node); hoverTo(1);
    });
    // The tip goes at once -- it tracks the pointer, so leaving it behind would leave it
    // pointing at nothing -- but the DISC fades back out, which is why state.hovered is
    // released by the tween at zero rather than here.
    renderer.on("leaveNode", function () { hideTip(); hoverTo(0); });
    renderer.on("clickNode", function (e) {
      // Consumed, not just read -- clickNode fires on this same release either way, and
      // once used it must not go on suppressing some LATER, unrelated click on the note
      // this one happened to drag. See dragJustMoved's own comment for why it exists.
      if (dragJustMoved === e.node) { dragJustMoved = null; return; }
      select(e.node);
    });
    renderer.on("clickStage", function () { select(null); });
    // Right-click is the pin gesture, and dragging into the hole is the other way to
    // reach it. preventDefault on the original event, or the browser's context menu
    // covers the thing that just moved.
    renderer.on("rightClickNode", function (e) {
      if (e.event && e.event.original) e.event.original.preventDefault();
      togglePin(e.node);
    });
    // NO clear-all on a stage right-click. It was here as "a way out that does not need you
    // to remember what you pinned", and it is a trap: the hub IS empty stage between its
    // dots, so right-clicking in the middle -- the most natural place to right-click when
    // you are looking at the hub -- threw the whole thing away. An undoable gesture would
    // be one thing; this was a destructive one sitting on top of the feature it destroys.
    // Unpinning is per note, the same way pinning is.
    bindNodeDrag();

    // DOUBLE CLICK RESETS THE VIEW, on the stage and on a note alike.
    //
    // preventSigmaDefault() is what makes it a reset rather than a reset AND sigma's own
    // double-click zoom: the captor emits the event and then checks that flag before doing
    // its own thing, synchronously, so setting it here is seen. Without it the two fight and
    // the camera lands somewhere neither asked for.
    //
    // A note gets the same treatment as the stage on purpose. "Double click zooms to this
    // note" is a defensible other answer, but then double click means two things depending
    // on a 6px target, and one of them is not what the tooltip says.
    var onDoubleClick = function (e) {
      if (e && e.preventSigmaDefault) e.preventSigmaDefault();
      fit();
    };
    renderer.on("doubleClickStage", onDoubleClick);
    renderer.on("doubleClickNode", onDoubleClick);
    // The wedge overlay, on by default in a --dev build. Here rather than at construction:
    // it needs geomLock and a first layout to have something to draw, and both exist by the
    // time the handlers are wired.
    if (wantWedgeDebug()) wedgeDebug(true);
  }

  /* ------------------------------------------------------- group labels */


  /* ------------------------------------------------------------ tooltip */

  function showTip(id) {
    var a = graph.getNodeAttributes(id), t = $("tip");
    var p = renderer.graphToViewport({ x: a.x, y: a.y });
    setHTML(t, '<div class="t">' + esc(a.label) + '</div>' +
      '<div class="m">' + esc(groupOf(id)) + ' &middot; ' + a.deg + ' edge' + (a.deg === 1 ? "" : "s") +   /* codanna: vocabulary */
      '<br>' + esc(a.ntype) + ' &middot; ' + esc(a.folder) +
      (a.sub ? ' / ' + esc(a.sub) : '') +
      '</div>');
    t.hidden = false;
    // #canvas, NOT #stage: graphToViewport returns coordinates relative to
    // sigma's container, and the heatmap band means #stage's origin is now
    // above it. Measuring against #stage put every tooltip the height of the
    // band too low, and clamped it against the wrong bottom edge.
    var box = t.getBoundingClientRect(), st = $("canvas").getBoundingClientRect();
    var x = Math.min(p.x + 14, st.width - box.width - 8);
    var y = Math.min(Math.max(p.y - box.height - 10, 8), st.height - box.height - 8);
    t.style.left = x + "px"; t.style.top = y + "px";
  }
  function hideTip() { $("tip").hidden = true; }

  /* ------------------------------------------------------- detail panel */

  // codanna: the symbol's signature, syntax-highlighted when the build inlined a
  // highlight.js grammar for its language (window.hljs, core + only the languages
  // present in the index -- see graph.mjs). No grammar, or hljs failing on a
  // malformed fragment, falls back to the escaped plain string.
  function sigHtml(sig, lang) {
    var hl = WIN.hljs;
    if (hl && lang && hl.getLanguage(lang)) {
      try { return hl.highlight(String(sig), { language: lang }).value; }
      catch (e) { /* fall through to plain */ }
    }
    return esc(sig);
  }

  // codanna: hop history for the relationship lists. Clicking a connected symbol walks
  // the graph and quickly loses the starting point, so hops -- and only hops -- accumulate
  // a trail, rendered as a back arrow plus crumbs at the top of the card. A fresh
  // selection (disc click, search hit) starts a new trail; closing the card ends it.
  var navTrail = [], navHop = false;
  function goTo(id) {
    if (state.selected && state.selected !== id) {
      navTrail.push(state.selected);
      if (navTrail.length > 30) navTrail.shift();
    }
    navHop = true;
    select(id); centerOn(id);
  }
  // i indexes navTrail; the target and everything after it leave the trail, so backing
  // up twice does not re-collect the place just left.
  function navBackTo(i) {
    var id = navTrail[i];
    navTrail.length = i;
    if (!graph.hasNode(id)) { select(null); return; }
    navHop = true;
    select(id); centerOn(id);
  }

  // codanna: keyboard bindings. Alt+ArrowLeft and Backspace step back along the hop
  // trail -- both preventDefault when they act, because the browser answers them with
  // history navigation, and on a file:// page that is leaving the graph. Escape walks
  // down the stack: blur the input being typed in, else close the settings panel, else
  // the detail card (the context menu's own capture listener already owns Escape while
  // it is open). "/" focuses search. Guards: keys inside inputs belong to the input,
  // and with two views in one document only the focused one acts.
  if (DOC && typeof DOC.addEventListener === "function") {
    DOC.addEventListener("keydown", function (ev) {
      var ae = DOC.activeElement;
      if (ae && ae !== DOC.body && !ROOT.contains(ae)) return;
      var t = ev.target;
      var typing = t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" ||
                         t.tagName === "SELECT" || t.isContentEditable);
      if (typing) {
        if (ev.key === "Escape" && t.blur) t.blur();
        return;
      }
      var ctx = $("ctxmenu");
      if (ctx && !ctx.hidden) return;
      if (ev.key === "Escape") {
        var sp = $("settings"), gear = $("gear");
        if (sp && !sp.hidden && gear && !gear.hidden) { gear.click(); ev.preventDefault(); return; }
        if (state.selected) { select(null); ev.preventDefault(); }
        return;
      }
      if ((ev.key === "ArrowLeft" && ev.altKey) ||
          (ev.key === "Backspace" && !ev.altKey && !ev.ctrlKey && !ev.metaKey)) {
        if (navTrail.length) { navBackTo(navTrail.length - 1); ev.preventDefault(); }
        return;
      }
      if (ev.key === "/" && !ev.ctrlKey && !ev.metaKey && !ev.altKey) {
        var qq = $("q");
        if (qq) { qq.focus(); ev.preventDefault(); }
      }
    });
  }

  function select(id) {
    if (!navHop && (!id || id !== state.selected)) navTrail.length = 0;   /* codanna: new trail on fresh selection */
    navHop = false;
    state.selected = id;
    syncLazyEdges();
    var d = $("detail");
    if (!id) { d.hidden = true; renderer.refresh(); return; }

    var a = graph.getNodeAttributes(id);
    var nb = neighboursOf(id).slice().sort(function (p, q) {
      return graph.getNodeAttribute(q, "deg") - graph.getNodeAttribute(p, "deg");
    });
    /* codanna: the hop trail (see goTo above). First crumb, ellipsis, last two when long. */
    var crumbs = "";
    if (navTrail.length) {
      var crumbBtn = function (i) {
        var lb = graph.hasNode(navTrail[i]) ? graph.getNodeAttribute(navTrail[i], "label") : "?";
        return '<button class="crumb" data-tr="' + i + '" title="Back to ' + esc(lb) + '">' + esc(lb) + '</button>';
      };
      var parts = [];
      if (navTrail.length <= 3) { for (var ti = 0; ti < navTrail.length; ti++) parts.push(crumbBtn(ti)); }
      else parts.push(crumbBtn(0), '<span class="dots">&hellip;</span>',
                      crumbBtn(navTrail.length - 2), crumbBtn(navTrail.length - 1));
      crumbs = '<div class="crumbs"><button class="nvb" data-tr="' + (navTrail.length - 1) +
        '" title="Back (Alt+Left or Backspace)">&#8592;</button>' + parts.join('<span class="sep">&rsaquo;</span>') + '</div>';
    }

    var h = '<button class="x" title="Close">&times;</button>' +
      crumbs +
      '<h2>' + esc(a.label) + '</h2>' +
      '<div class="meta">' +
        '<span><b style="color:' + colorOf(groupOf(id)) + '">&#9632;</b> ' + esc(groupOf(id)) + '</span>' +
        '<span>' + a.deg + ' edge' + (a.deg === 1 ? "" : "s") + '</span>' +   /* codanna: vocabulary */
        (a.words ? '<span>' + a.words + ' line' + (a.words === 1 ? "" : "s") + '</span>' : "") +
        (a.created ? '<span>' + esc(a.created) + '</span>' : "") +
      '</div>' +
      '<div>' + (a.tags || []).slice(0, 8).map(function (t) {
        return '<span class="chip">#' + esc(t) + '</span>';
      }).join("") + '</div>' +
      '<div class="chip" style="border-style:dashed">' + esc(a.folder) +
        (a.sub ? ' / ' + esc(a.sub) : '') + ' / ' + esc(a.ntype) + '</div>' +
      /* codanna: a symbol has no note to open; its location is the id (file:start-end). */
      '<div class="chip" style="border-style:dashed;font-family:ui-monospace,SFMono-Regular,Menlo,Consolas,monospace">' + esc(a.path) + '</div>' +
      (a.sig ? '<pre class="sig">' + sigHtml(a.sig, a.lang) + '</pre>' : "") +
      '<div class="actions">' +
        // The label stays "Pin to hub" either way -- the .btn convention above is that a
        // toggle shows its state by filling in, not by changing what it says, and the
        // pin icon itself is already on/off (see pinSvg). Same gesture as right-click.
        '<button class="btn pin" data-pin="' + id + '" aria-pressed="' + isPinned(id) + '" title="' +
          (isPinned(id) ? "Unpin from hub" : "Pin to hub") + '">' + pinSvg(isPinned(id)) +
          ' Pin to hub</button>' +
      '</div>';

    /* codanna: connected symbols grouped by relation and direction, from the adjacency's
       histogram (see the adj builder above -- each entry's r is already this node's view). */
    if (nb.length) {
      var REL_ORDER = ["Calls", "Uses", "Implements", "Extends", "Defines"];
      var REL_LABEL = { Calls: ["Calls", "Called by"], Uses: ["Uses", "Used by"],
                        Implements: ["Implements", "Implemented by"], Extends: ["Extends", "Extended by"],
                        Defines: ["Defines", "Defined in"] };
      var relOf = Object.create(null);
      (adj[id] || []).forEach(function (e) { relOf[e.o] = e.r; });
      var relGroups = Object.create(null), plain = [], headings = [];
      nb.forEach(function (n) {
        var rels = relOf[n], hit = false;
        if (rels) {
          Object.keys(rels).forEach(function (rel) {
            var c = rels[rel], lab = REL_LABEL[rel] || [rel, rel + " (in)"];
            if (c[0]) { (relGroups[lab[0]] = relGroups[lab[0]] || []).push({ n: n, k: c[0] }); hit = true; }
            if (c[1]) { (relGroups[lab[1]] = relGroups[lab[1]] || []).push({ n: n, k: c[1] }); hit = true; }
          });
        }
        if (!hit) plain.push({ n: n, k: 1 });
      });
      REL_ORDER.forEach(function (rel) { var lab = REL_LABEL[rel]; headings.push(lab[0], lab[1]); });
      Object.keys(relGroups).forEach(function (k) { if (headings.indexOf(k) < 0) headings.push(k); });
      var row = function (it) {
        return '<li><button data-go="' + it.n + '">' + esc(graph.getNodeAttribute(it.n, "label")) +
               (it.k > 1 ? ' <span class="rk">&times;' + it.k + '</span>' : '') +
               ' <span style="color:var(--text-3)">' + graph.getNodeAttribute(it.n, "deg") + '</span></button></li>';
      };
      headings.forEach(function (hd) {
        var list = relGroups[hd]; if (!list || !list.length) return;
        h += '<div class="nb">' + esc(hd) + ' (' + list.length + ')</div><ul>' + list.slice(0, 60).map(row).join("") + '</ul>';
      });
      if (plain.length) h += '<div class="nb">Connected (' + plain.length + ')</div><ul>' + plain.slice(0, 60).map(row).join("") + '</ul>';
    } else {
      h += '<div class="nb">No edges</div>';
    }

    setHTML(d, h);
    d.hidden = false;
    d.querySelector(".x").onclick = function () { select(null); };
    // Re-selecting the same id rebuilds the card so the pressed state and icon follow
    // the toggle -- togglePin itself already re-lays-out the ring and repaints the mark.
    d.querySelector(".pin").onclick = function () { togglePin(id); select(id); };
    Array.prototype.forEach.call(d.querySelectorAll("[data-go]"), function (b) {
      b.onclick = function () { goTo(b.getAttribute("data-go")); };   /* codanna: hop, keeps the trail */
    });
    Array.prototype.forEach.call(d.querySelectorAll("[data-tr]"), function (b) {
      b.onclick = function () { navBackTo(+b.getAttribute("data-tr")); };
    });
    renderer.refresh();
  }

  // The camera lives in framed-graph space, not graph space, so read the node's
  // display coords rather than its raw x/y.
  function centerOn(id) {
    var d = renderer.getNodeDisplayData(id);
    if (!d) return;
    renderer.getCamera().animate({ x: d.x, y: d.y, ratio: 0.22 }, { duration: 420 });
  }

  /* ---------------------------------------------------------------- UI */

  // buildSettings lives inside buildTools's own closure (the gear/settings-panel
  // wiring), not out here beside buildLegend -- so buildLegend cannot call it directly
  // without eslint (correctly) flagging it as undefined, and would throw at runtime if
  // it tried. buildTools() sets this once, during the one-time init sequence that runs
  // well before anything could click a twisty, so the settings panel's own subfolder
  // rows can be kept in sync with the legend's without hoisting either function out of
  // the closure it belongs in.
  var refreshSettingsPanel = null;

  function buildLegend() {
    // EVERY ROW BELOW IS ABOUT TO BE REPLACED, and the one under the pointer goes with
    // them -- so its mouseleave will never fire and its halo would be left on with
    // nothing hovered. Clearing here is the only place that covers all of the callers
    // (a click, a colour pick, a filter) rather than each of them remembering to.
    hoverHighlight(null, null);

    var names = order[state.dim] || [];
    $("gcount").textContent = "(" + names.length + ")";

    // subCount ("folder/sub" -> note count) is module-scope now -- see its declaration
    // beside subOrder. What's still built fresh on every render is the folder TREE, as
    // "prefix -> { childName: count }" at every depth. Built by walking each note's own
    // `dirs` chain, so the legend's nesting comes from the vault rather than from any
    // assumed number of levels: a folder five deep renders the same way as one a single
    // level down.
    var kids = Object.create(null);
    if (state.dim === "folder") {
      graph.forEachNode(function (_id, a) {
        var d = a.dirs || [];
        for (var i = 0; i < d.length; i++) {
          var pk = a.folder + "/" + d.slice(0, i).join("/");
          if (!kids[pk]) kids[pk] = Object.create(null);
          kids[pk][d[i]] = (kids[pk][d[i]] || 0) + 1;
        }
      });
    }

    // One eye, two states. Drawn as inline SVG rather than an emoji or a font glyph
    // so it is the same 14px shape on every machine and inherits currentColor.
    var eyeBtn = function (attrs, on, what) {
      return '<button class="eye" ' + attrs + ' aria-pressed="' + on + '" title="' +
             (on ? "Hide " : "Show ") + esc(what) + '">' + eyeSvg(on) + '</button>';
    };
    // twBtn is module-scope now -- see its declaration beside eyeSvg.

    // Everything BELOW a sub-wedge row, recursively, at whatever depth the vault goes.
    // These levels take no tint and cut no wedge -- they inherit their parent's swatch
    // colour, because that is the truth: the pie does not distinguish them, and the
    // legend must not claim otherwise. What they do get is their own eye and their own
    // highlight, which is what makes a person's 1-on-1s selectable.
    var subtree = function (prefix, depth, col) {
      var m = kids[prefix];
      if (!m || !state.pathOpen[prefix]) return "";
      return Object.keys(m).sort(function (a, b) {
        return m[b] - m[a] || a.localeCompare(b);
      }).map(function (nm) {
        var pk = prefix + "/" + nm;
        var on = !state.hiddenSub[pk];
        var hlk = !!state.highlightSub[pk];
        return '<div class="lgr sub' + Math.min(depth, 4) + '">' +
          twBtn(kids[pk] ? 'data-twp="' + esc(pk) + '"' : null, !!state.pathOpen[pk]) +
          eyeBtn('data-epath="' + esc(pk) + '"', on, nm) +
          '<button class="lgs" data-hpath="' + esc(pk) + '" data-hl="' +
            (hlk ? "on" : "off") + '" aria-pressed="' + on +
            '" title="Highlight ' + esc(nm) + '">' +
          '<span class="sw" style="background:' + col + ';border-radius:50%"></span>' +
          '<span class="nm">' + esc(nm) + '</span>' +
          '<span class="only" data-only="1" title="Show only ' + esc(nm) + '">only</span>' +
          '<span class="ct">' + m[nm] + '</span></button>' +
          '</div>' + subtree(pk, depth + 1, col);
      }).join("");
    };

    setHTML($("legend"), names.map(function (g) {
      var vis = !isHidden(g);
      // A PIN BYPASSES BOTH SIZE GATES, not just the subfolder-count one -- see
      // groupHasPinnedSub's own comment. NEST_MIN exists to skip nesting a folder too
      // small to be worth it automatically; it says nothing about a folder somebody
      // has deliberately pinned a subfolder colour on, however small that folder has
      // since shrunk to.
      var hasSubs = state.dim === "folder" &&
                    (groupHasPinnedSub(g) ||
                     ((subOrder[g] || []).length > 1 && (counts[g] || 0) >= NEST_MIN));
      var open = hasSubs && !state.collapsed[g];
      var hl = !!state.highlight[g];

      var row = '<div class="lgr">' +
        twBtn(hasSubs ? 'data-tw="' + esc(g) + '"' : null, open) +
        eyeBtn('data-eye="' + esc(g) + '"', vis, g) +
        '<button class="lg" data-g="' + esc(g) + '" data-hl="' + (hl ? "on" : "off") +
          '" aria-pressed="' + vis + '" title="Highlight ' + esc(g) + '">' +
        // THE SWATCH SAYS WHICH RING, by its size: a small square for the inner band and a
        // full one for the outer. The legend named a colour and a count and said nothing about
        // where on the disc to look, which is the one thing a two-ring layout needs it to say.
        // Size rather than another glyph or another colour: it costs no room, adds no
        // vocabulary, and the inner ring IS the smaller ring -- so the mark and the thing it
        // stands for read the same way round.
        '<span class="sw' + (bandLock && bandLock[g] ? ' sw-in' : '') +
          '" title="' + (bandLock && bandLock[g] ? 'Inner ring' : 'Outer ring') +
          '" style="background:' + colorOf(g) + '"></span>' +
        '<span class="nm" title="' + esc(g) + '">' + esc(g) + '</span>' +
        '<span class="only" data-only="1" title="Show only ' + esc(g) + '">only</span>' +
        '<span class="ct">' + counts[g] + '</span></button>' +
        '</div>';

      // Subfolders are listed only when they actually get their own wedges, so the
      // legend never promises a split the pie does not show -- and only when the
      // group is unfolded and visible.
      if (open && vis) {
        var subs = subOrder[g];
        // One row per subfolder: an eye, a tint swatch, a name and a count. The tail
        // row stands for several subfolders at once, so it carries all their indices
        // and its eye toggles every folder it represents.
        var srow = function (col, nm, ct, idx, depth, twAttrs, twOpen) {
          var on = !state.hiddenSub[g + "/" + subs[idx[0]]];
          // Highlighted only when EVERY subfolder the row stands for is highlighted,
          // so the aggregate tail row reports the truth rather than lighting up for
          // a partial selection made further down.
          var hlSub = idx.every(function (i) {
            return !!state.highlightSub[g + "/" + subs[+i]];
          });
          return '<div class="lgr ' + (depth === 2 ? "sub2" : "sub") + '">' +
            twBtn(twAttrs || null, !!twOpen) +
            eyeBtn('data-esub="' + esc(g) + '" data-idx="' + idx.join(",") + '"', on, nm) +
            '<button class="lgs" data-hsub="' + esc(g) + '" data-idx="' + idx.join(",") +
              '" data-hl="' + (hlSub ? "on" : "off") + '" aria-pressed="' + on +
              '" title="Highlight ' + esc(nm) + '">' +
            '<span class="sw" style="background:' + col + ';border-radius:50%"></span>' +
            '<span class="nm">' + esc(nm) + '</span>' +
            '<span class="only" data-only="1" title="Show only ' + esc(nm) + '">only</span>' +
            '<span class="ct">' + ct + '</span></button>' +
            '</div>';
        };
        subs.slice(0, SUB_NAMED).forEach(function (sb, k) {
          var pk = g + "/" + sb, tint = subShade[pk] || colorOf(g);
          // A named sub-wedge gets a twisty of its own when the vault nests deeper
          // under it -- that is how `00 1 on 1` keeps being one wedge of 62 notes AND
          // opens to the seven people inside it.
          row += srow(tint, sb || "(directly in folder)", subCount[pk] || 0, [k], 1,
                      kids[pk] ? 'data-twp="' + esc(pk) + '"' : null,
                      !!state.pathOpen[pk]);
          row += subtree(pk, 2, tint);
        });
        var tail = subs.slice(SUB_NAMED);
        if (tail.length) {
          var n = 0;
          tail.forEach(function (sb) { n += subCount[g + "/" + sb] || 0; });
          var tOpen = !!state.tailOpen[g];
          // The aggregate row keeps its own eye (toggling all of them at once) and
          // gains a twisty, because "7 smaller subfolders" is exactly the row you
          // want to look inside. They all share the last tint step, so the swatches
          // beneath are deliberately identical -- the pie does not distinguish them
          // either, and pretending otherwise in the legend would be a lie.
          row += srow(subShade[g + "/" + tail[0]] || colorOf(g),
                      tail.length + " smaller subfolders", n,
                      tail.map(function (_, j) { return SUB_NAMED + j; }), 1,
                      'data-twtail="' + esc(g) + '"', tOpen);
          if (tOpen) {
            tail.forEach(function (sb, j) {
              var pk = g + "/" + sb, tint = subShade[pk] || colorOf(g);
              row += srow(tint, sb || "(directly in folder)", subCount[pk] || 0,
                          [SUB_NAMED + j], 2,
                          kids[pk] ? 'data-twp="' + esc(pk) + '"' : null,
                          !!state.pathOpen[pk]);
              row += subtree(pk, 3, tint);
            });
          }
        }
      }
      return row;
    }).join(""));

    var each = function (sel, fn) {
      Array.prototype.forEach.call($("legend").querySelectorAll(sel), fn);
    };

    // "only" at a LEVEL-1 row: keep the named subfolders this row stands for (the tail
    // row stands for several) and hide every sibling. subOrder[g] carries "" for notes
    // sitting directly in the folder, and "<g>/" is exactly the key visible() checks
    // for those, so the empty name needs no special case.
    var onlySubs = function (g, keep) {
      var h = state.hidden[state.dim] || (state.hidden[state.dim] = Object.create(null));
      (order[state.dim] || []).forEach(function (n) { h[n] = (n !== g); });
      state.hiddenSub = Object.create(null);
      (subOrder[g] || []).forEach(function (sb) {
        if (keep.indexOf(sb) < 0) state.hiddenSub[g + "/" + sb] = true;
      });
    };

    // "only" at any DEEPER row, keyed by full path. Hiding is inherited by a subtree,
    // so the right thing to hide is the shallowest key on each note's own chain that
    // DIVERGES from the wanted path -- one key per sibling branch, at whatever depth it
    // branches. Derived from the notes rather than from the tree the legend drew,
    // because the legend only renders what is unfolded.
    var onlyUnder = function (g, path) {
      var h = state.hidden[state.dim] || (state.hidden[state.dim] = Object.create(null));
      (order[state.dim] || []).forEach(function (n) { h[n] = (n !== g); });
      state.hiddenSub = Object.create(null);
      var rest = path.slice(g.length + 1);
      var want = rest ? rest.split("/") : [];
      graph.forEachNode(function (_id, a) {
        if (a.folder !== g) return;
        var d = a.dirs || [], i = 0;
        while (i < want.length && i < d.length && d[i] === want[i]) i++;
        if (i === want.length) return;                  // at or under the wanted path
        // Diverged at i, or ran out of depth before reaching it. slice(0, i + 1) is the
        // branch to hide; for a note directly in the folder that is "" -> "<g>/".
        state.hiddenSub[g + "/" + d.slice(0, i + 1).join("/")] = true;
      });
    };

    // Twisties: pure disclosure. No layout consequence at all, so no cascade -- the
    // pie already shows every sub-wedge whether or not the legend lists it.
    each("[data-tw]", function (b) {
      var g = b.getAttribute("data-tw");
      // THE WHOLE ROW ANSWERS "WHERE IS THIS FOLDER", not just the label. The halo was bound
      // to the label button alone, and the eye and the twisty are its SIBLINGS -- so reaching
      // for the control you actually wanted lost the answer you were using to aim with.
      b.onmouseenter = function () { hoverHighlight(g, null); };
      b.onmouseleave = function () { hoverHighlight(null, null); };
      b.onclick = function () {
        if (state.collapsed[g]) delete state.collapsed[g]; else state.collapsed[g] = true;
        buildLegend();
        // The settings panel's own subfolder rows read this SAME flag -- see
        // buildSettings, and refreshSettingsPanel's own comment for why this is not a
        // direct call -- so opening a folder here opens it there too, rather than the
        // two disagreeing about which folders are expanded.
        if (refreshSettingsPanel) refreshSettingsPanel();
      };
    });
    // Deeper folder levels: a twisty, an eye and a highlight each, keyed by full path
    // so one handler serves every depth.
    each("[data-twp]", function (b) {
      b.onclick = function () {
        var p = b.getAttribute("data-twp");
        if (state.pathOpen[p]) delete state.pathOpen[p]; else state.pathOpen[p] = true;
        buildLegend();
      };
    });
    each("[data-epath]", function (b) {
      b.onclick = function () {
        var p = b.getAttribute("data-epath");
        if (state.hiddenSub[p]) delete state.hiddenSub[p]; else state.hiddenSub[p] = true;
        buildLegend();
        cascade(null, { colToggle: true });
      };
    });
    each("[data-hpath]", function (b) {
      var hp = b.getAttribute("data-hpath");
      b.onmouseenter = function () { hoverHighlight(null, [hp]); };
      b.onmouseleave = function () { hoverHighlight(null, null); };
      b.onclick = function (ev) {
        var p = b.getAttribute("data-hpath");
        if (ev && ev.target && ev.target.getAttribute("data-only")) {
          onlyUnder(p.slice(0, p.indexOf("/")), p);
          buildLegend();
          cascade(null, { colToggle: true });
          return;
        }
        if (state.highlightSub[p]) delete state.highlightSub[p];
        else state.highlightSub[p] = true;
        buildLegend();
        applyLayout(true);
        renderer.refresh();
      };
    });

    each("[data-twtail]", function (b) {
      b.onclick = function () {
        var g = b.getAttribute("data-twtail");
        if (state.tailOpen[g]) delete state.tailOpen[g]; else state.tailOpen[g] = true;
        buildLegend();
      };
    });

    // Eyes: visibility, which is what the whole row used to do.
    each("[data-eye]", function (b) {
      var g = b.getAttribute("data-eye");
      // NO hover halo on the eye, deliberately re-removed 2026-08-24. It was added so the
      // whole row answers "where is this folder" (see the twisty above, which keeps it), but
      // the eye is the one control whose click starts an animation OF the haloed notes -- and
      // the pointer is still on the eye while that animation runs, so the halo's size bump and
      // radial push ride the entire fade and the departing notes read as touching. The deeper
      // eyes (data-esub, data-epath) never had a halo, so this also makes the eyes consistent
      // with each other. The label keeps its halo: highlighting is that button's whole job.
      b.onclick = function () {
        var h = state.hidden[state.dim] || (state.hidden[state.dim] = Object.create(null));
        h[g] = !h[g];
        buildLegend();
        cascade(null, { colToggle: true });
      };
    });
    each("[data-esub]", function (b) {
      b.onclick = function () {
        var f = b.getAttribute("data-esub");
        var subs = subOrder[f] || [];
        var off = b.getAttribute("aria-pressed") === "true";
        b.getAttribute("data-idx").split(",").forEach(function (i) {
          var key = f + "/" + subs[+i];
          if (off) state.hiddenSub[key] = true; else delete state.hiddenSub[key];
        });
        buildLegend();
        cascade(null, { colToggle: true });               // one note at a time, and the wedge opens for it
      };
    });

    // Subfolder label: highlight that subfolder. The tail row stands for several, so
    // it toggles them as a block -- all on if any were off, all off once they are all
    // on, which is the same "make it so" behaviour the tail's eye has.
    each("[data-hsub]", function (b) {
      // Hover lights every subfolder the row stands for -- which for the pooled tail row
      // is several. Resolved here rather than in hoverHighlight, because the row carries
      // tint-slot INDICES and only subOrder can turn those back into names.
      var hoverKeys = function () {
        var f = b.getAttribute("data-hsub");
        var subs = subOrder[f] || [];
        return b.getAttribute("data-idx").split(",").map(function (i) {
          return f + "/" + subs[+i];
        });
      };
      b.onmouseenter = function () { hoverHighlight(null, hoverKeys()); };
      b.onmouseleave = function () { hoverHighlight(null, null); };
      b.onclick = function (ev) {
        var f = b.getAttribute("data-hsub");
        var subs = subOrder[f] || [];
        var idx = b.getAttribute("data-idx").split(",");
        if (ev && ev.target && ev.target.getAttribute("data-only")) {
          onlySubs(f, idx.map(function (i) { return subs[+i]; }));
          buildLegend();
          cascade(null, { colToggle: true });
          return;
        }
        var allOn = idx.every(function (i) { return !!state.highlightSub[f + "/" + subs[+i]]; });
        idx.forEach(function (i) {
          var key = f + "/" + subs[+i];
          if (allOn) delete state.highlightSub[key]; else state.highlightSub[key] = true;
        });
        buildLegend();
        applyLayout(true);
        renderer.refresh();
      };
    });

    // The label: HIGHLIGHT, not hide. Only a radius changes, so this animates the
    // positions rather than running a reveal cascade -- nothing appears or leaves.
    each(".lg[data-g]", function (b) {
      var g = b.getAttribute("data-g");
      b.onmouseenter = function () { hoverHighlight(g, null); };
      b.onmouseleave = function () { hoverHighlight(null, null); };
      b.onclick = function (ev) {
        if (ev.target && ev.target.getAttribute("data-only")) {
          var h = state.hidden[state.dim] || (state.hidden[state.dim] = Object.create(null));
          (order[state.dim] || []).forEach(function (n) { h[n] = (n !== g); });
          buildLegend();
          cascade(null, { colToggle: true });
          return;
        }
        if (state.highlight[g]) delete state.highlight[g]; else state.highlight[g] = true;
        buildLegend();
        applyLayout(true);       // tween the push; the halo lands with the refresh
        renderer.refresh();
      };
    });
  }

  // THE DEFAULT VISIBILITY, in one place, for the same reason collapseAll exists: boot and
  // Refresh have to agree about what "default" means, and two copies of that answer drift.
  //
  // This is what changes Refresh's meaning slightly, and the change is deliberate: it used
  // to clear every filter to "everything visible", and now it returns to the CONFIGURED
  // default, which hides archives. Anything else would make Refresh the one control that
  // disagrees with the settings.
  function seedHidden() {
    var h = state.hidden[state.dim] = Object.create(null);
    (order[state.dim] || []).forEach(function (g) {
      if (hiddenByDefault(g)) h[g] = true;
    });
  }

  // Collapse every group. Used for the initial state and by resetView, so "collapsed by
  // default" has one definition rather than two that can drift apart.
  function collapseAll() {
    state.collapsed = Object.create(null);
    (order[state.dim] || []).forEach(function (g) { state.collapsed[g] = true; });
  }
  var collapsedInit = false;

  function regroup() {
    counts = computeOrder();
    buildColors();
    // Once, at boot. Not on every regroup: __vg.relayout() calls this too, and
    // re-collapsing there would throw away whatever the user had opened -- which is
    // exactly why seedHidden belongs in here with it rather than beside it. Seeding the
    // hidden set on every regroup would put the archives back every time the layout was
    // rebuilt, silently undoing an eye the user had just clicked.
    if (!collapsedInit) { collapsedInit = true; collapseAll(); seedHidden(); }
    // Fix the ring each group belongs to, from the whole data set, before
    // anything is filtered.
    if (!bandLock) {
      var base = buildWedgePlan(false);
      if (base) {
        bandLock = Object.create(null);
        base.cells.forEach(function (c) { bandLock[c.g] = c.inner; });
        // maxR is kept as well as the two band radii: the edge curvature needs to
        // know how big the disc is to judge which chords pass near its centre.
        // Each band's OUTER EDGE, in graph units, which is what the seam is sized against.
        // Taken from the full-vault plan's own slots -- the radius a note actually sits at,
        // rather than the lattice figure maxR holds, because a seam is a width between dots.
        // PER BAND as well as in total, because the spacing is solved per band now: each ring
        // asks how much of ITS OWN notes are showing, so hiding a folder moves the ring that
        // folder is in and leaves the other one alone.
        var bandTotal = { i: 0, o: 0 };
        base.cells.forEach(function (c) { bandTotal[c.inner ? "i" : "o"] += c.wsum; });
        var bandR = { i: 0, o: 0 }, bandRows = { i: 0, o: 0 };
        base.cells.forEach(function (c) {
          var k = c.inner ? "i" : "o";
          // How deep the band is: the deepest cell in it, which is what sets its outer edge
          // and is the number the seam falls off against.
          if (c.rows > bandRows[k]) bandRows[k] = c.rows;
          // sl.r is in LATTICE units and UNIT converts to the graph coordinates the seam
          // width is expressed in -- a note at graph radius 3879 has slotR 24. Skipping
          // this made the seam 160x too wide, and because a wider seam eats avail, forces
          // narrower wedges and so more rows, the disc GREW to absorb it: 3879 -> 4840 on
          // the 1402-note vault and 9885 -> 12444 on the 10k one.
          (c.slots || []).forEach(function (sl) {
            var rr = sl.r * UNIT;
            if (rr > bandR[k]) bandR[k] = rr;
          });
        });
        // total is the DENOMINATOR of the density solve: every later plan asks "how much of
        // the vault is on screen" and this is the "of the vault" half. Captured from the same
        // unfiltered plan as the radii, so the two cannot disagree.
        geomLock = { r0: base.r0, rOuter: base.rOuter, maxR: base.maxR,
                     total: base.total, bandTotal: bandTotal,
                     bandR: bandR, rows: bandRows };

        // AND THEN AGAIN, now that bandR exists. This plan was built before it did, so its
        // gaps came from the pre-lock fallback rule rather than from the seam -- a different
        // reservation, so a different maxR. Every later plan uses the seam, which left the
        // full-vault disc measuring 96% of its own locked reference: fit() then zoomed to
        // 1.0372 where the whole page assumes 1.08, and two camera checks failed on a number
        // that was right about the disc and wrong about the constant.
        //
        // One extra plan build at load, once, and only the radii are taken from it -- bandLock
        // stays with the first, because which ring a group belongs to must not depend on a gap.
        var again = buildWedgePlan(false);
        if (again) geomLock = { r0: again.r0, rOuter: again.rOuter, maxR: again.maxR,
                                total: again.total, bandTotal: bandTotal,
                                bandR: bandR, rows: bandRows };

        // PIN THE NORMALISATION BOX. Sigma rescales node coordinates against the
        // graph's bounding box on every refresh (autoRescale, on by default), so
        // as the disc shrinks the box shrinks and the whole thing is re-normalised
        // -- measured, hiding one folder moved the graph origin 13px on screen and
        // zoomed everything 8.2%, with the camera provably untouched at
        // x=0.5 y=0.5 ratio=1.08. That is the ring centre appearing to jump, and
        // during a cascade it happens every single frame.
        //
        // Fixing the box to the full-vault extent means the disc genuinely shrinks
        // on screen when filtered, instead of the camera silently zooming to
        // refill the viewport. The box is symmetric about the origin, so the
        // camera's (0.5, 0.5) is still the centre of the disc and fit() is
        // unaffected.
        if (renderer) {
          var span = base.maxR * UNIT * 1.02;
          renderer.setCustomBBox({ x: [-span, span], y: [-span, span] });
        }
      }
    }
    buildLegend();
    syncAlpha();
    applyLayout(false);
    // The band paints with nodeColor(), which buildColors() just re-derived. That is
    // invisible to heatDraw's signature -- it tracks counts, not hues -- so the
    // cached paint is dropped explicitly rather than waiting for a count to move.
    if (heat) { heatSig = ""; heatDraw(); }
  }


  function buildSearch() {
    var q = $("q");
    q.oninput = function () {
      state.query = q.value.trim().toLowerCase();
      var hits = $("hits");
      if (!state.query) { hits.replaceChildren(); renderer.refresh(); return; }
      var found = [];
      graph.forEachNode(function (id, a) {
        if (a.label.toLowerCase().indexOf(state.query) > -1) found.push(id);
      });
      found.sort(function (p, o) { return graph.getNodeAttribute(o, "deg") - graph.getNodeAttribute(p, "deg"); });
      setHTML(hits, found.slice(0, 40).map(function (id) {
        return '<button data-hit="' + id + '">' + esc(graph.getNodeAttribute(id, "label")) +
               ' <span style="color:var(--text-3)">' + graph.getNodeAttribute(id, "deg") + '</span></button>';
      }).join("") || '<div style="color:var(--text-3);font-size:11px;padding:4px">No match</div>');
      Array.prototype.forEach.call(hits.querySelectorAll("[data-hit]"), function (b) {
        b.onclick = function () {
          var id = b.getAttribute("data-hit");
          q.value = ""; state.query = ""; hits.replaceChildren();
          select(id); centerOn(id);
        };
      });
      renderer.refresh();
    };
    q.onkeydown = function (e) {
      if (e.key !== "Enter") return;
      var first = $("hits").querySelector("[data-hit]");
      if (first) first.click();
    };
  }

  var play = null;                       // in-flight timeline playback
  // AND IF THE TAB GOES AWAY MID-INTRO, finish it rather than leave it parked.
  //
  // The load branch already comes up at rest when the page starts hidden. This is the other
  // half: a tab that is visible at load and hidden a second later stalls in exactly the same
  // place, because requestAnimationFrame stops and the watchdog that would rescue the run is
  // driven by rAF too. Both playback and any cascade in flight are dropped and the resting
  // layout assigned, which is where they were going.
  //
  // A timer, not a frame, on purpose: visibilitychange fires in a background tab where rAF does
  // not, so this is the one hook that still runs when the thing it is fixing happens.
  var introOwed = false;      // hidden at load: the intro is owed to the first look at the tab
  if (DOC && typeof DOC.addEventListener === "function") {
    DOC.addEventListener("visibilitychange", function () {
      var away = typeof DOC.visibilityState === "string"
        ? DOC.visibilityState === "hidden" : !!DOC.hidden;
      if (!away) {
        // The tab has been looked at for the first time and an intro was deferred: play it now.
        if (introOwed) { introOwed = false; playTimeline(); }
        return;
      }
      // THE INTRO IS `anim`, not `play` and not cascadeRun. Checking only those two meant this
      // did nothing for the case it was written for -- the intro is what a fresh tab is running.
      if (!play && !cascadeRun && !anim) return;
      var wasPlaying = !!play;      // the timeline, as opposed to a filter cascade
      // AND THE SAME TEARDOWN A COMPLETED RUN DOES, which is the whole of it.
      //
      // Cancelling the frame is not stopping a cascade: a cascade holds pinnedPlan and planKeep
      // for its duration and clears them when it lands, and every layout after one that was
      // cancelled without clearing them reuses a plan that was pinned for a run that no longer
      // exists. The disc then stops responding to anything -- reported as animations stopping
      // altogether after switching tabs, which is exactly what a permanently pinned plan looks
      // like. playTimeline does this teardown too; see the note there.
      stopPlay();
      if (cascadeRun) {
        WIN.cancelAnimationFrame(cascadeRun.raf);
        WIN.clearTimeout(cascadeRun.guard);
        cascadeRun = null;
      }
      if (anim) { WIN.cancelAnimationFrame(anim); anim = null; }
      if (animGuard) { WIN.clearTimeout(animGuard); animGuard = null; }
      pinnedPlan = null; planKeep = null; roomNow = null; cellNow = null; edgeNow = null;
    colWalk = null;
      posSrc = posDst = null;
      // AND THE TIMELINE HAS TO BE COMPLETED, not merely re-rendered.
      //
      // timelineFrame() draws whatever state.until says, and until is how far through the intro
      // we are. It is null at load -- which is why the load path draws the whole disc -- and
      // partway through while the intro runs, so calling it here without clearing until first
      // re-renders the same unfinished frame the tab was already stuck on. Which was the whole
      // report: open three vaults, each one gets focus for a moment as it opens and then loses
      // it to the next, and every tab but the last stays parked mid-intro.
      state.until = null;
      timelineFrame(true);
      // An intro cut short is still owed, so it plays when the tab is next looked at rather
      // than being lost because the tab happened to be focused for a moment while loading.
      if (wasPlaying) introOwed = true;
    });
  }

  function stopPlay() {
    if (!play) return;
    // The intro drives a cascade now rather than its own loop, so stopping it means tearing that
    // cascade down -- otherwise Stop would clear the button and leave the animation running.
    // Landing on the fully grown disc is what the old land() did, so Stop still means "skip to
    // the end" rather than "freeze here".
    var viaCascade = play.viaCascade;
    WIN.cancelAnimationFrame(play.raf);
    if (play.guard) WIN.clearTimeout(play.guard);   // or the deadline lands after a manual stop
    play = null;
    endSweep();
    if (!viaCascade) return;
    if (cascadeRun) {
      WIN.cancelAnimationFrame(cascadeRun.raf);
      WIN.clearTimeout(cascadeRun.guard);
      cascadeRun = null;
    }
    pinnedPlan = null; planKeep = null; roomNow = null; cellNow = null; edgeNow = null; posSrc = posDst = null;
    colWalk = null;
    state.until = null;
    timelineFrame(true);
  }

  // `full` forces a re-indexing refresh. Pass it on any frame that LANDS -- the end
  // of playback, or a jump to "all" -- and leave it off while scrubbing.
  //
  // It matters because curved edges take their curvature from node POSITIONS, and
  // the disc moves every frame of the timeline. skipIndexation does not re-upload
  // edge data, so the GPU keeps whatever curvature was computed when the edge
  // buffers were last built -- and if that was the `until = 0` frame, where
  // ringsLayout() returns null and positions are degenerate, every value collapses
  // to CURVE_MIN and the finished disc draws with visibly straight links. The edge
  // reducer still reports type "curve" the whole time, which is what makes this
  // invisible to measurement: the data is right and the buffer is stale.
  function timelineFrame(full) {
    // The timeline ALWAYS grows a ring: the disc keeps its full circle and simply
    // gets denser as notes arrive. This line is why -- `fullRing` was written in
    // exactly one place, inside cascade(), and read in another, inside the packer,
    // so this path silently inherited whatever mode the last cascade had left
    // behind. After a fresh load that is false (first paint has an empty screen,
    // so it draws the pie clockwise in wedges); after any legend toggle it is
    // true. Same Play button, two different animations, and which one you got
    // depended on whether you had touched a filter first.
    //
    // Detecting the mode from what is present() is not merely unset here, it is
    // actively wrong: draw mode exists for a screen with no ring to trade
    // against, and the timeline empties the screen itself on its first frame
    // (until = 0). It would therefore choose draw mode every single time it was
    // asked honestly. The timeline is a density animation by construction, so the
    // mode is a constant, not an observation.
    fullRing = true;
    syncAlpha();
    var targets = ringsLayout();
    if (targets) assignPositions(targets);
    renderer.refresh({ skipIndexation: !full });
    // NOTHING LEFT TO READ OUT. This ended by writing "<date> . <rank>" into the sidebar's
    // Timeline readout, which is gone along with the slider it belonged to. `state.until`
    // itself stays: it is how the visibility handler and the ?rest boot say "the whole disc,
    // no animation", and timeFactor still ramps on it.
  }

  // The vault growing from its first note to now. Extracted from the Play button
  // so the intro and Refresh run the same animation rather than three subtly
  // different ones.
  // The vault growing from its first note to now -- AS A CASCADE, the same transition every
  // other change to what is shown goes through.
  //
  // It used to run its own frame loop: per frame it moved state.until, called syncAlpha() and
  // then assigned a fresh unpinned ringsLayout(). That is a sequence of independent RESTING
  // layouts, one per date cutoff, so nothing connected one frame to the next -- and when the
  // solved row count changed between two cutoffs the whole disc reseated. Measured on the real
  // vault, rowsInner walked 0 -> 2 -> 3 during the intro and each step moved most of the disc at
  // once: 81 of 98 notes past 300 units on one frame, 125 of 207 on another, 200 of 361 on a
  // third, worst single note 5964 units. Reported as a big jump in the inner ring with notes
  // suddenly popping up.
  //
  // None of the continuity work applied to it, because none of that lives in this path: the row
  // walk, the room walk, the plan-derived gap presence and a settle that assigns the plan are all
  // inside cascade(). Growing the vault is not a different kind of change -- it is the date
  // range end moving, which is a filter change -- so it goes through the one path and inherits all
  // of it. What is left here is the two things that genuinely differ: notes arrive by DATE rather
  // than clockwise, and the window is TIMELINE_MS rather than derived from how many are moving.
  function playTimeline() {
    stopPlay();
    // Anything else that drives positions has to be cancelled first, or it fights
    // the playback frame for frame. A load cascade still in flight was the other
    // half of the "wrong animation" report: press Play before the intro finished
    // and two loops wrote node coordinates on the same frames.
    if (cascadeRun) {
      WIN.cancelAnimationFrame(cascadeRun.raf);
      WIN.clearTimeout(cascadeRun.guard);
      cascadeRun = null;
    }
    if (anim) { WIN.cancelAnimationFrame(anim); anim = null; }
    if (animGuard) { WIN.clearTimeout(animGuard); animGuard = null; }
    pinnedPlan = null; planKeep = null; roomNow = null; cellNow = null; edgeNow = null;
    colWalk = null;
    posSrc = posDst = null;

    // DESTINATION: the whole vault. SOURCE: an empty screen. cascade() reads the current alphas
    // as its starting point, so clearing them is what makes this the growth animation rather
    // than a no-op -- state.until stays null throughout, and the ribbon's sweep below is driven
    // from the cascade's own progress instead of being the thing that drives the frame.
    var dur = TIMELINE_MS * TIME_SCALE;
    state.until = null;
    // AND THE WHOLE VAULT MEANS NO DATE RANGE. The range is a hard cap in timeFactor, so a
    // range left over from before would hold part of the vault at zero for the entire intro --
    // "grow from the first note to now" quietly growing to somebody's old filter instead. It
    // was left behind because resetView never cleared it, so Refresh replayed the intro
    // through whatever range was applied. Cleared here as well as there, because boot and the
    // debug API reach this function without going through resetView.
    //
    // rangeChrome() rather than applyRange(): the chrome has to follow the state, but a
    // cascade here would be a second animation racing the one about to start.
    state.from = null; state.to = null;
    rangeChrome();
    clearAlpha();

    cascade(function () {
      if (play) play = null;
      endSweep();
    }, {
      fullRing: true,
      order: function (id) { return tlRank[id] || 0; },
      // NO spread override: the stagger is windowFor(n), the same expression a folder toggle
      // uses, so a note's own fade is the same fraction of the animation in both. Overriding it
      // (240 frames against windowFor's ~74) made the timeline a visibly different animation
      // rather than the same one over a longer clock, which is the whole point of it going
      // through cascade().
      totalMs: dur,
      onFrame: function (pr) { sweepTo(pr); }
    });
    // AFTER cascade(), because cascade() calls stopPlay() on the way in -- setting this first
    // would have the run tear its own sweep down on the frame it started.
    play = { raf: 0, guard: 0, viaCascade: true };
  }

  /**
   * THE INTRO IS THE RIGHT-HAND SCRUBBER, TRAVELLING.
   *
   * Growing the vault from its first note to now and dragging the range end from one end of
   * the strip to the other are the same statement about the same history, and the page used to
   * make it twice in two different units: the disc revealed notes by RANK while the sidebar's
   * slider counted them, and the ribbon -- the control that actually draws the history -- sat
   * still through all of it. With the slider gone the animation had nothing left to say what
   * it was doing.
   *
   * SO THE PROGRESS FRACTION IS TURNED INTO A DATE THROUGH THE RANK, not through the span.
   * That is the whole of why it stays in sync. Interpolating dateSpan.lo -> dateSpan.hi
   * linearly would put the handle at 2020 while the disc was already showing every note from
   * 2026, because the reveal is ordered by rank and this vault is not spread evenly in time:
   * measured, 409 of 442 notes fall in the last three months against a handful back to 2015.
   * Reading tlDateMs at rank `pr * tlMax` makes the handle's position mean exactly "everything
   * left of here is on screen", which is the same thing it means when a hand is on it.
   *
   * The consequence is honest and worth naming: the handle crawls through the empty years and
   * sprints across the last months. That is the shape of the vault, and the bars underneath it
   * are drawing the same shape.
   *
   * THE TOOLTIP COMES TOO, because it is what a real drag shows -- the strip has no numeric
   * readout of its own, and the date under the handle is what the deleted sidebar readout was
   * for. Same helper, same position, so the intro and the gesture look alike down to the label.
   */
  function sweepTo(pr) {
    if (!dateSpan || !tlMax) return;
    var k = Math.max(0, Math.min(1, pr)) * tlMax;
    var i = Math.round(k) - 1;
    if (i < 0) i = 0;
    if (i > tlDateMs.length - 1) i = tlDateMs.length - 1;
    var ms = tlDateMs[i];
    // An unparseable stamp keeps its rank in tlDateMs as NaN -- see buildTimeline. Clamp it
    // to the span rather than letting NaN reach ribbonX, where it would take the handle and
    // the wash with it.
    if (!(ms >= dateSpan.lo)) ms = dateSpan.lo;
    if (ms > dateSpan.hi) ms = dateSpan.hi;
    brushSweep = pr >= 1 ? dateSpan.hi : ms;
    drawRibbon();
    if (pr >= 1) { hideRTip(); return; }
    showRTip(ribbonX(brushSweep, ribbonW()), isoDay(brushSweep));
  }

  /** Put the strip back on the real state, whichever way the run ended. */
  function endSweep() {
    if (brushSweep === null) return;
    brushSweep = null;
    hideRTip();
    // drawDateUI, not drawRibbon: the year buttons carry a pressed state derived from the
    // range, and leaving them showing the sweep's last frame is the same class of bug as a
    // control disagreeing with its own state.
    drawDateUI();
  }

  /* buildTimelineUI() IS GONE, and with it the whole sidebar Timeline block.
   *
   * It wired four controls, and every one of them had a better answer somewhere else in the
   * page:
   *
   *   slider, Play, All   a second instrument for scrubbing the same history, in a second
   *                       unit -- the slider was linear in note COUNT, the ribbon under the
   *                       band is linear in TIME. Two controls answering one question
   *                       differently is one too many to explain, and the ribbon is the one
   *                       that also draws the history it scrubs. The intro drives the
   *                       ribbon's own right-hand handle now; see sweepTo.
   *   Mark today          the band's today column already marks exactly those notes, by the
   *                       same `created` predicate, in the place the count is drawn. The
   *                       button was a second predicate in a second place, and it is the one
   *                       that got "today" wrong twice. Clicking the last cell of the band
   *                       is the control; nodeStyle carries the fill treatment it owned.
   */

  // Every filter back to its default, in state AND in the controls that show it.
  // Both halves matter: leaving a control showing one value while the state holds
  // another is worse than not resetting at all, because the next interaction jumps
  // from a value the user can see to one they cannot.
  function resetView() {
    stopPlay();
    seedHidden();
    state.hiddenSub = Object.create(null);
    state.highlight = Object.create(null);
    state.highlightSub = Object.create(null);
    collapseAll();                // back to the DEFAULT, which is folded, not unfolded
    state.tailOpen = Object.create(null);
    state.pathOpen = Object.create(null);
    state.markDay = null;
    state.hoverDay = null;
    state.until = null;
    state.query = "";
    state.hovered = null;
    select(null);                 // closes the detail card and clears state.selected
    hideTip();
    $("q").value = "";    $("hits").replaceChildren();
    // THE DATE RANGE IS A FILTER AND WAS NOT BEING RESET. Refresh claims to clear every
    // filter and replay the intro, and it cleared every filter but this one -- so replaying
    // the intro through an applied range grew the vault to a slice of itself. Invisible for
    // as long as the "timeline" meant the rank slider, which this did reset; the ribbon is
    // the timeline now, so the omission is the bug.
    //
    // No applyRange() here: resetView is followed by a cascade of its own -- playTimeline
    // from Refresh -- and rangeChrome is the half that has to happen either way.
    state.from = null; state.to = null; state.heatEnd = null;
    rangeChrome();
    buildLegend();
  }

  function buildTools() {
    // See refreshSettingsPanel's own declaration, beside buildLegend: this is the one
    // assignment that makes it reachable from there. buildSettings is hoisted within
    // this function body, so the forward reference is fine.
    refreshSettingsPanel = buildSettings;

    // BACK TO DEFAULTS, not literally everything -- a folder disabled in the settings
    // panel (hiddenByDefault(), the same rule seedHidden() applies at load) stays hidden
    // here too. A blanket clear used to override that: a person who deliberately hid
    // "08 - Archive" by default would have it pop back on the moment they clicked All,
    // which reads as the button ignoring a choice made one screen over. "None" (below)
    // is the true blanket override -- hiding everything has nothing to preserve, so it
    // stays as it was.
    $("allon").onclick = function () {
      seedHidden();
      state.hiddenSub = Object.create(null);
      buildLegend(); cascade(null, { colToggle: true });
    };
    $("alloff").onclick = function () {
      var h = state.hidden[state.dim] = Object.create(null);
      (order[state.dim] || []).forEach(function (g) { h[g] = true; });
      buildLegend(); cascade(null, { colToggle: true });            // recedes rim-inward; nothing to lay out
    };
    // Refresh clears every filter and replays the intro, in-page -- no navigation.
    //
    // It used to navigate to its own URL with a cache-busting "?t=" query, on the
    // theory that it was re-reading the file from disk. That was never worth what
    // it cost: the data is BAKED IN at build time, so a reload can only ever
    // redisplay the same bytes unless refresh-graph.ps1 has rewritten the file in
    // the meantime -- and picking up new notes is that script's job anyway, since
    // it rebuilds and opens in one step. Meanwhile the navigation itself is
    // unreliable on the one protocol this page actually runs on: a query string
    // on a file:// URL is not honoured consistently, so the click could do
    // nothing at all, which is exactly how it was reported.
    //
    // Resetting state directly always works, costs no reload, and -- the point --
    // always plays the animation.
    //
    // THE PLUGIN CAN DO BETTER, AND NOW DOES. Everything above is about a file whose
    // data was baked in at build time -- true of the standalone page and only of the
    // standalone page. Mounted in Obsidian the vault is right there, and the view can
    // rebuild from the metadata cache in a few hundred milliseconds, so the host passes
    // `onRefresh` and the button means what everybody assumed it meant: pick up what I
    // have written since. Reported as "Refresh doesn't seem to pick up new files"
    // (github#6), which was a fair reading of a button labelled Refresh.
    var onRefresh = typeof deps.onRefresh === "function" ? deps.onRefresh : null;
    if (onRefresh) {
      $("refresh").title = "Rebuild from the vault and replay. Picks up notes written " +
                           "since the graph was drawn, and clears every filter.";
    }
    $("refresh").onclick = function () {
      // The host tears this mount down and builds a new one, so there is nothing to
      // reset here and no animation to start -- the fresh mount plays its own intro.
      if (onRefresh) { onRefresh(); return; }
      resetView();
      fit();
      playTimeline();
    };
    // THE CAMERA CLUSTER (github#4). The Fit button in View is gone: it did exactly what
    // the corner control does, and two buttons for one job is one too many to explain.
    if ($("reset")) $("reset").onclick = fit;
    if ($("zin")) $("zin").onclick = function () { zoomBy(1); };
    if ($("zout")) $("zout").onclick = function () { zoomBy(-1); };
    if ($("pan")) $("pan").onclick = function () { setPan(!panEnabled, true); };
    // The saved default, applied once the renderer exists. Not persisted -- writing here
    // would save a value the host just handed us.
    setPan(panEnabled, false);
    // Unconditional, unlike the settings-panel row (buildOptions), because this is the
    // only in-VIEW way to flip it on the plugin host -- the gear there opens Obsidian's
    // own settings tab instead of #vg-settings (github#23).
    if ($("compact")) $("compact").onclick = function () { setCompactAxis(!compactAxis, true); };
    setCompactAxis(compactAxis, false);
    $("png").onclick = savePng;
    // COPIES THE DUMP, and falls back to the console when the clipboard refuses -- which it
    // does on a page opened from a file in some builds. Either way the text exists somewhere
    // it can be pasted from, which is the whole job.
    if ($("dbg")) $("dbg").onclick = function () {
      var txt = JSON.stringify(API.debugDump(), null, 2);
      var done = function (how) {
        var b = $("dbg");
        b.textContent = how;
        WIN.setTimeout(function () { b.textContent = "Debug"; }, 1600);
      };
      // THE FALLBACK SAVES A FILE, it does not log.
      //
      // It used to `console.log(txt)` and label the button "In console", which is a fair
      // affordance and a rule violation the Obsidian directory will not let anyone waive:
      // obsidianmd/rule-custom-message is on the preset's restricted-disable list, so an
      // eslint-disable for it is itself an error. Correctly so -- the guideline is about
      // plugins that chatter into a console shared by every other plugin.
      //
      // A file is the better answer anyway. This path exists for the case where the clipboard
      // refuses, which is a file:// page outside a secure context -- exactly the host where
      // collecting a bug report is hardest -- and a .json somebody can attach beats a console
      // dump they have to select by hand. It reuses the mechanism Save PNG already proves in
      // both hosts: an anchor with a data URL and a download attribute.
      var save = function () {
        try {
          var a = DOC.createElement("a");
          a.href = "data:application/json;charset=utf-8," + encodeURIComponent(txt);
          a.download = "vault-graph-debug.json";
          a.click();
          done("Saved");
        } catch (e2) {
          // Nothing left to try, and a button that lies is worse than one that admits it.
          done("Failed");
        }
      };
      try {
        WIN.navigator.clipboard.writeText(txt).then(function () { done("Copied"); }, save);
      } catch (e) {
        save();
      }
    };

    // THE GEAR, if a host asked for it in either of the two ways. It is hidden by default
    // because a page that cannot store a setting should not offer to change one.
    if (openHostSettings) {
      // HOSTED: the gear is a shortcut to the host's own settings, so it opens nothing
      // here. `aria-expanded` is removed rather than left at "false" -- it would be
      // claiming to control a panel this button no longer has.
      $("gear").hidden = false;
      $("gear").removeAttribute("aria-expanded");
      $("gear").removeAttribute("aria-controls");
      $("gear").onclick = function () { openHostSettings(); };
    } else if (SETTINGS_UI) {
      $("gear").hidden = false;
      $("gear").onclick = function () {
        var open = $("settings").hidden;
        $("settings").hidden = !open;
        $("gear").setAttribute("aria-expanded", String(open));
        if (open) { buildOptions(); buildSettings(); }
      };
      // BOTH MAPS, not just folderColors -- this button sits under "Folder colours",
      // which now covers subfolders too (see subfolderRows), so Reset dropping only
      // the top-level overrides would leave a subfolder pin behind for the user to
      // go find and clear by hand. Mirrors plugin/main.js's own "Reset all".
      $("fcreset").onclick = function () {
        pickColor(null, null);
        var savedSub = applySubfolderColors({});
        if (saveSubfolderColors) saveSubfolderColors(Object.assign({}, savedSub));
        buildSettings();
      };
      // DELEGATED on the container, which survives -- buildSettings replaces its
      // children on every pick, so a listener bound to a swatch would be bound to an
      // element that is about to be thrown away.
      $("setbody").addEventListener("click", function (ev) {
        var t = ev.target instanceof Element ? ev.target : null;
        if (!t) return;
        var v = t.closest("[data-vis]");
        if (v) { pickVisible(v.getAttribute("data-vis")); return; }
        var tw = t.closest("[data-stw]");
        if (tw) {
          var fg = tw.getAttribute("data-stw");
          if (state.collapsed[fg]) delete state.collapsed[fg]; else state.collapsed[fg] = true;
          buildSettings();
          buildLegend();          // see the [data-tw] handler: the two stay in sync
          return;
        }
        var s = t.closest("[data-sfc]");
        if (s) {
          var pk = s.getAttribute("data-sfc"), slash = pk.indexOf("/");
          pickSubColors(pk.slice(0, slash), [pk.slice(slash + 1)],
                        s.getAttribute("data-key") || null);
          return;
        }
        var b = t.closest("[data-fc]");
        if (b) pickColor(b.getAttribute("data-fc"), b.getAttribute("data-key") || null);
      });
      // DELEGATED for the same reason as setbody above, though #vg-optbody's rows are only
      // ever rebuilt by buildOptions() itself (never thrown away mid-interaction the way a
      // folder pick rebuilds the swatch grid) -- kept delegated anyway so a second option
      // row needs no second listener.
      $("optbody").addEventListener("click", function (ev) {
        var t = ev.target instanceof Element ? ev.target : null;
        var b = t && t.closest("[data-opt]");
        if (!b) return;
        var key = b.getAttribute("data-opt");
        var row = OPTION_ROWS.filter(function (o) { return o.key === key; })[0];
        if (row) { row.set(!row.get()); buildOptions(); }
      });
    }

    // RIGHT-CLICK, IN THE LEGEND ITSELF -- a second, faster way to reach the same twelve
    // swatches the settings panel offers, without opening it. Unconditional: unlike the
    // gear above, this does not depend on SETTINGS_UI or openSettings, because a context
    // menu is not a second settings surface, just a shortcut to the pick functions below,
    // which already no-op safely with no host to save through.
    function closeCtxMenu() {
      var el = $("ctxmenu");
      if (el) el.hidden = true;
      DOC.removeEventListener("mousedown", ctxOutside, true);
      DOC.removeEventListener("keydown", ctxKey, true);
      WIN.removeEventListener("resize", closeCtxMenu);
    }
    function ctxOutside(ev) {
      var el = $("ctxmenu");
      if (el && !el.hidden && !el.contains(ev.target)) closeCtxMenu();
    }
    function ctxKey(ev) { if (ev.key === "Escape") closeCtxMenu(); }

    // THE ONE SWATCH-BUTTON MARKUP, shared by the three places that draw the twelve slots:
    // the context menu below, a subfolder's row, and a folder's row in the settings panel.
    // All three build the same <button class="swatch vg-<key>"> shape and used to each
    // write it out by hand -- structurally identical, differing only in which extra
    // data-* attribute carries the row's own key (or none, for the context menu, which
    // reads data-key alone) and in the title-suffix wording, which stays a per-caller
    // callback because "chosen" vs "automatic" vs "automatic default" is a real
    // difference in what each surface can even claim (a context menu has no separate
    // "automatic default" state to report; a subfolder row never marks Auto at all).
    //
    //   pal        paletteInfo()'s list
    //   opts.role         "radio" or "menuitemradio"
    //   opts.dataAttr     extra data-* name carrying the row's own key ("fc"/"sfc"), omit
    //                     for the context menu (data-key alone is enough there)
    //   opts.dataValue    that attribute's value (a folder name, or "<folder>/<sub>")
    //   opts.current      the slot key in force, "" for none
    //   opts.autoKey      the slot Auto would give back, "" if this row never marks one
    //   opts.titleFor(on, isAuto)  the title-text suffix for one swatch
    function swatchButtonsHTML(pal, opts) {
      return pal.map(function (p) {
        var on = opts.current === p.key;
        var isAuto = !!opts.autoKey && opts.autoKey === p.key;
        return '<button class="swatch vg-' + p.key + '" role="' + opts.role + '"' +
               (opts.dataAttr ? ' data-' + opts.dataAttr + '="' + esc(opts.dataValue) + '"' : '') +
               ' data-key="' + p.key + '" aria-checked="' + on + '"' +
               (isAuto ? ' data-auto="1"' : '') +
               ' title="' + esc(p.name) + (opts.titleFor ? opts.titleFor(on, isAuto) : "") +
               '" aria-label="' + esc(p.name) + '"></button>';
      }).join("");
    }

    // The same 12-swatch + Auto markup buildSettings' own rows build, at the click point
    // instead of in the sidebar. `current` is a slot key or "" for none; onPick(key) fires
    // with the chosen key, or null for Auto.
    //
    // x/y are VIEWPORT coordinates (straight off the contextmenu event), and the menu is
    // positioned relative to ROOT rather than the true viewport -- `position: fixed`
    // measures from the nearest ancestor with a transform or CSS containment, not
    // necessarily the window, and Obsidian's own workspace panes carry exactly that.
    // Measured there: the menu opened detached from the row that was right-clicked,
    // floating wherever the transformed ancestor's origin happened to be. ROOT's own
    // getBoundingClientRect() already reflects whatever transform sits above it, so
    // subtracting it converts a viewport point into ROOT's coordinate space correctly
    // regardless of what Obsidian does further up the tree -- the same reason #vg-detail
    // and #vg-tip are `position: absolute` against their own ancestor rather than fixed.
    // autoKey is the folder's own automatic slot, omitted by the subfolder caller below --
    // a subfolder's automatic colour is a computed tint, not one of these twelve, so there
    // is nothing among them for it to mark. See page.css's data-auto rule.
    function openCtxMenu(x, y, current, onPick, autoKey) {
      var el = $("ctxmenu");
      if (!el) return;
      var pal = paletteInfo();
      var sws = swatchButtonsHTML(pal, {
        role: "menuitemradio", current: current, autoKey: autoKey,
        titleFor: function (on, isAuto) { return isAuto ? " (automatic)" : ""; }
      });
      setHTML(el, '<div class="sws">' + sws + '</div>' +
                  '<button class="auto" data-key="" aria-pressed="' + !current +
                  '" title="Back to automatic">Auto</button>');
      Array.prototype.forEach.call(el.querySelectorAll("[data-key]"), function (b) {
        b.onclick = function () { onPick(b.getAttribute("data-key") || null); closeCtxMenu(); };
      });
      el.hidden = false;
      var root0 = ROOT.getBoundingClientRect();
      var rx = x - root0.left, ry = y - root0.top;
      // Clamped to ROOT's own box, in the same coordinate space: a right-click near the
      // edge must not open a popover that hangs off the view.
      var r = el.getBoundingClientRect();
      el.style.left = Math.max(4, Math.min(rx, ROOT.clientWidth - r.width - 4)) + "px";
      el.style.top = Math.max(4, Math.min(ry, ROOT.clientHeight - r.height - 4)) + "px";
      DOC.addEventListener("mousedown", ctxOutside, true);
      DOC.addEventListener("keydown", ctxKey, true);
      WIN.addEventListener("resize", closeCtxMenu);
    }

    // ONE DELEGATED LISTENER, for the same reason the settings panel's click handler is
    // delegated: buildLegend replaces every row on every render, so anything bound to a
    // row itself would be bound to an element about to be thrown away.
    $("legend").addEventListener("contextmenu", function (ev) {
      var t = ev.target instanceof Element ? ev.target : null;
      if (!t) return;
      var gBtn = t.closest(".lg[data-g]");
      if (gBtn) {
        ev.preventDefault();
        var g = gBtn.getAttribute("data-g");
        openCtxMenu(ev.clientX, ev.clientY, folderColors[g] || groupSlot[g] || "",
                    function (key) { pickColor(g, key); }, groupAutoSlot[g] || "");
        return;
      }
      // The pooled tail row ("N smaller subfolders") carries several indices -- a pick
      // there applies to every subfolder it stands for, matching how that row's eye and
      // highlight already act as a block rather than forcing one right-click per member.
      var subBtn = t.closest(".lgs[data-hsub]");
      if (subBtn) {
        ev.preventDefault();
        var f = subBtn.getAttribute("data-hsub");
        var subs = subOrder[f] || [];
        var idx = subBtn.getAttribute("data-idx").split(",").map(Number);
        var picked = idx.map(function (i) { return subs[i]; });
        var cur = idx.length === 1 ? (subfolderColors[f + "/" + picked[0]] || "") : "";
        openCtxMenu(ev.clientX, ev.clientY, cur,
                    function (key) { pickSubColors(f, picked, key); });
        return;
      }
      // Depth 2+ (a `data-hpath` row): no picker. Per design/0003-subfolder-
      // differentiation.md these levels take no tint of their own and inherit their
      // parent's swatch, so there is nothing here to pin -- the browser's own context
      // menu shows instead.
    });

    // One folder's slot, or -- with both arguments null -- every override dropped.
    // Nothing else in the map is touched: two folders may hold the same slot, and that
    // is a way of saying they belong together rather than a collision to resolve.
    function pickColor(folder, key) {
      var next = Object.create(null);
      if (folder) {
        Object.keys(folderColors).forEach(function (g) { next[g] = folderColors[g]; });
        if (key) next[folder] = key; else delete next[folder];
      }
      var saved = applyFolderColors(next);
      if (saveFolderColors) saveFolderColors(Object.assign({}, saved));
      buildSettings();
    }

    // One or more subfolders' slot, in the SAME folder, set to the same key in one write.
    // The array is what lets the pooled "N smaller subfolders" legend row apply a single
    // pick to every subfolder it stands for, matching how that row's eye and highlight
    // already act as a block rather than forcing N separate clicks.
    function pickSubColors(folder, subs, key) {
      var next = Object.create(null);
      Object.keys(subfolderColors).forEach(function (k) { next[k] = subfolderColors[k]; });
      subs.forEach(function (sb) {
        var pk = folder + "/" + sb;
        if (key) next[pk] = key; else delete next[pk];
      });
      var saved = applySubfolderColors(next);
      if (saveSubfolderColors) saveSubfolderColors(Object.assign({}, saved));
      buildSettings();
    }

    // Flip one folder's DEFAULT visibility, and apply it now so the click does something
    // visible. Written as an explicit true/false rather than by deleting the key: the
    // point of the tri-state is that "shown, and I said so" survives a later change to
    // what `_` folders do by default.
    function pickVisible(folder) {
      var next = Object.create(null);
      Object.keys(folderShown).forEach(function (g) { next[g] = folderShown[g]; });
      next[folder] = hiddenByDefault(folder);      // was hidden -> show it, and back
      var saved = applyFolderShown(next);
      if (saveFolderShown) saveFolderShown(Object.assign({}, saved));
      // The live filter follows the default it just changed. Without this the setting
      // would only take effect on the next Refresh, which reads as the click not working.
      var h = state.hidden[state.dim] || (state.hidden[state.dim] = Object.create(null));
      if (hiddenByDefault(folder)) h[folder] = true; else delete h[folder];
      buildLegend();
      cascade(null, { colToggle: true });                                   // notes fade in or out; nothing jumps
      buildSettings();
    }

    // Rebuilt whole on every pick. The list is one row per top-level folder and the
    // vault has tens of those, not thousands, so there is nothing here worth the bugs
    // that come with patching rows in place.
    // Every named subfolder of g, as its own radiogroup: a live tint preview, its name
    // and count, an Auto button, and the same twelve swatches a folder row gets. Unlike
    // a folder row, no swatch here is ever marked "current" while unpinned -- the
    // automatic tint is a computed shade, not one of the twelve slot hexes, so there is
    // nothing among them to ring. The Auto button's own pressed state already says
    // "this one is automatic"; ringing a swatch that is not actually being used would be
    // the lie a folder row's "(automatic)" marking specifically avoids.
    function subfolderRows(g, pal) {
      return (subOrder[g] || []).map(function (sb) {
        var pk = g + "/" + sb;
        var pin = subfolderColors[pk] || "";
        var tint = subShade[pk] || colorOf(g);
        var nm = sb || "(directly in folder)";
        var sws = swatchButtonsHTML(pal, {
          role: "radio", dataAttr: "sfc", dataValue: pk, current: pin,
          titleFor: function (on, isAuto) { return on ? " (chosen)" : ""; }
        });
        return '<div class="scr scrsub" role="radiogroup" aria-label="Colour for ' +
               esc(g + "/" + nm) + '">' +
               '<div class="scrh">' +
               '<span class="sw" style="background:' + tint + ';border-radius:50%"></span>' +
               '<span class="nm" title="' + esc(nm) + '">' + esc(nm) + '</span>' +
               '<span class="ct">' + (subCount[pk] || 0) + '</span>' +
               '<button class="auto" data-sfc="' + esc(pk) + '" data-key=""' +
               ' aria-pressed="' + (!pin) + '"' +
               ' title="Back to the automatic tint">Auto</button>' +
               '</div>' +
               '<span class="sws">' + sws + '</span></div>';
      }).join("");
    }

    // GENERIC BOOLEAN OPTIONS for the standalone settings panel -- one row today
    // (compactAxis, github#23), written as a table so a second one is an entry here, not a
    // new mechanism. Reuses the existing .mini button[aria-pressed] convention (a toggle
    // shows its state by being filled; the label stays constant), so no new CSS is needed.
    var OPTION_ROWS = [
      { key: "compactAxis", label: "Compact date axis",
        title: "Give each year width by how many notes it holds, instead of every year reading the same width",
        get: function () { return compactAxis; },
        set: function (v) { setCompactAxis(v, true); } }
    ];
    function buildOptions() {
      var host = $("optbody");
      if (!host) return;
      setHTML(host, OPTION_ROWS.map(function (o) {
        var on = !!o.get();
        return '<div class="row" style="margin-bottom:7px">' +
               '<div class="lbl" style="margin:0">' + esc(o.label) + '</div>' +
               // "vg-" prefixed, to match what $() actually looks up ($() prepends ID="vg-"
               // itself; a literal "opt-<key>" id here would never resolve through $()).
               '<div class="mini"><button id="vg-opt-' + o.key + '" data-opt="' + o.key + '"' +
               ' aria-pressed="' + on + '" title="' + esc(o.title) + '">Enabled' +
               '</button></div></div>';
      }).join(""));
    }

    function buildSettings() {
      var pal = paletteInfo();
      var rows = (order[state.dim] || []).map(function (g) {
        var pinned = folderColors[g] || "";
        // THE SLOT THIS FOLDER IS ACTUALLY USING, read back from buildColors rather than
        // recomputed -- archives are skipped in the rotation, so `i % SLOT_COUNT` is no
        // longer the answer and an archive is on no slot at all.
        //
        // Marking only the pinned one meant that a folder on Auto, which is every folder
        // until somebody changes something, had no mark anywhere: the panel showed twelve
        // colours and would not say which of them the folder was. The two states are
        // marked differently because "this is the colour" and "this is the colour I chose"
        // are different facts, and the Auto button is otherwise the only thing saying so.
        var cur = pinned || groupSlot[g] || "";
        // THE SLOT THIS FOLDER WOULD BE ON WITH NO OVERRIDE AT ALL -- distinct from `cur`
        // exactly when the folder is pinned to something else. Marked independently of
        // aria-checked so it stays visible after an override (github#29): before this,
        // "automatic" and "currently in use" were the same ring, and a pin erased the
        // only trace of what Auto would give back.
        var autoKey = groupAutoSlot[g] || "";
        // The colour comes from `.vg-<key>` in page.css, not from an inline style: a hex
        // baked in here would not survive the theme flip that readTheme handles.
        var sws = swatchButtonsHTML(pal, {
          role: "radio", dataAttr: "fc", dataValue: g, current: cur, autoKey: autoKey,
          titleFor: function (on, isAuto) {
            return on ? (pinned ? " (chosen)" : " (automatic)") : (isAuto ? " (automatic default)" : "");
          }
        });
        // AUTO SITS ON THE NAME LINE, not at the end of the swatches. As a thirteenth
        // item in that row it was the one thing that could not fit beside twelve
        // swatches in a 288px sidebar, so the row wrapped and left a single swatch and
        // a button stranded on a line of their own. Up here it costs no width at all --
        // the name was already short of the full line -- and the swatches below are a
        // fixed twelve-column grid that cannot wrap however narrow the sidebar gets.
        // The eye sits with the colour, because "which folders am I looking at" and "what
        // colour are they" are the two things this panel is for. It is a DEFAULT, not the
        // live filter: the legend's own eye is the live one, and this is what the disc
        // comes back to on Refresh.
        var shown = !hiddenByDefault(g);
        // SAME GATE buildLegend uses for its own twisty (pin included -- see
        // groupHasPinnedSub), and the SAME state (state.collapsed) -- opening a
        // folder's subfolders here opens them in the legend too.
        var hasSubs = state.dim === "folder" &&
                      (groupHasPinnedSub(g) ||
                       ((subOrder[g] || []).length > 1 && (counts[g] || 0) >= NEST_MIN));
        var open = hasSubs && !state.collapsed[g];
        return '<div class="scr" role="radiogroup" aria-label="Colour for ' + esc(g) + '">' +
               '<div class="scrh">' +
               twBtn(hasSubs ? 'data-stw="' + esc(g) + '"' : null, open) +
               '<button class="eye vis" data-vis="' + esc(g) + '" aria-pressed="' + shown +
               '" title="' + (shown ? "Shown by default" : "Hidden by default") +
               '" aria-label="' + (shown ? "Hide" : "Show") + " " + esc(g) + '">' +
               eyeSvg(shown) + '</button>' +
               '<span class="nm" title="' + esc(g) + '">' + esc(g) + '</span>' +
               '<button class="auto" data-fc="' + esc(g) + '" data-key=""' +
               ' aria-pressed="' + (!pinned) + '"' +
               ' title="Back to the slot this folder gets automatically">Auto</button>' +
               '</div>' +
               '<span class="sws">' + sws + '</span></div>' +
               (open ? subfolderRows(g, pal) : "");
      }).join("");
      setHTML($("setbody"), rows);
    }
    // No theme toggle: the page is dark, always. `<html data-theme="dark">` is set in
    // the markup, which outranks both the bare :root tokens and the
    // prefers-color-scheme block, so a light OS still gets the dark disc. The wedge
    // hues were validated against the dark surface anyway -- see [[Vault Graph]] on
    // why the tint ladder steps away from the surface rather than toward it.
  }

  // ratio 1 frames the NODES exactly, which clips their labels at the edges;
  // pull back a little so the outermost labels stay on screen.
  // With the rim labels gone there is nothing outside the node extent, so the disc
  // can fill more of the canvas. A little margin still keeps the outermost notes
  // clear of the edge.
  // Fit is now purely a zoom reset: the centre never moves, so this only has to
  // put the ratio back.
  var FIT_RATIO = 1.08;      // the full disc, filling the stage

  /**
   * FIT THE DISC THAT IS THERE, not the one the vault started with.
   *
   * The normalisation box is pinned to the FULL-VAULT extent on purpose -- see the note on
   * setCustomBBox -- so that filtering makes the disc genuinely shrink instead of the camera
   * silently zooming to refill the viewport every frame of every cascade. That is right
   * during an animation and wrong the moment somebody asks to be centred: hide half the
   * vault and "fit" would frame the empty ring the notes used to occupy.
   *
   * So the ratio is scaled by how much of the locked extent the disc currently uses. At full
   * vault that is 1 and this is exactly the old constant; with the outer ring toggled away it
   * closes in on what is left. The box does the not-moving and the camera does the zooming,
   * which keeps the two jobs apart -- renormalising the box per frame is what moved the ring
   * centre 13px and zoomed 8.2% with the camera provably untouched.
   *
   * Measured on the live radius rather than the plan's: a plan is what the layout intends and
   * this question is about what is on screen, which after a cascade are the same thing and
   * during one are not.
   */
  function fitRatio() {
    var locked = geomLock && geomLock.maxR ? geomLock.maxR : 0;
    var live = lastMaxR;
    if (!locked || !live) return FIT_RATIO;
    // The upper clamp used to be 1, on the reasoning that the box is the full-vault size
    // so anything beyond it is empty margin. That stopped being true with the density
    // solve (github#13): a filtered disc spreads its lattice to keep its notes at an
    // honest density, and the outermost row lands a few percent PAST the locked extent
    // -- measured up to 1.079. That is disc, not margin, and clamping at 1 framed it
    // with its rim cut off. Allowed out to 1.35, which covers a full row of overshoot at
    // any density the cap permits, and no further: past that something else is wrong and
    // framing empty space would hide it.
    // Clamped low as well, or a single surviving note in the hub would fill the stage.
    var k = live / locked;
    if (k > 1.35) k = 1.35;
    if (k < 0.12) k = 0.12;
    return FIT_RATIO * k;
  }

  function fit() {
    var to = { x: 0.5, y: 0.5, ratio: fitRatio(), angle: 0 };
    // SIGMA DROPS x AND y WHILE PANNING IS OFF (Camera.validateState), so with the pan
    // toggle off this would apply the ratio and leave the disc wherever it was last
    // dragged -- centring that does not centre. Panning is turned back on for the flight
    // and taken away again in the callback, which sigma also fires if the animation is
    // cut short. Reported on github#4, which hit it from the other direction.
    if (!panEnabled) {
      renderer.setSetting("enableCameraPanning", true);
      renderer.getCamera().animate(to, { duration: 380 }, function () {
        renderer.setSetting("enableCameraPanning", false);
      });
      return;
    }
    renderer.getCamera().animate(to, { duration: 380 });
  }

  // The wheel's own step, so a button press and a notch agree. Sigma's zoomingRatio is the
  // setting the wheel reads; reading it back rather than repeating 1.2 means they cannot
  // drift apart.
  function zoomBy(dir) {
    var cam = renderer.getCamera();
    var step = renderer.getSetting("zoomingRatio") || 1.2;
    var r = cam.getState().ratio * (dir > 0 ? 1 / step : step);
    var lo = renderer.getSetting("minCameraRatio"), hi = renderer.getSetting("maxCameraRatio");
    if (typeof lo === "number" && r < lo) r = lo;
    if (typeof hi === "number" && r > hi) r = hi;
    cam.animate({ ratio: r }, { duration: renderer.getSetting("zoomDuration") || 120 });
  }

  // PAN IS A MODE, and the host owns its default. Turning it off flies home FIRST and
  // re-locks on landing, for the validateState reason in fit(): a camera left off-centre
  // with panning disabled cannot be recovered by anything that sets x or y, which includes
  // fit() itself.
  function setPan(on, persist) {
    panEnabled = !!on;
    var btn = $("pan");
    if (btn) btn.setAttribute("aria-pressed", panEnabled ? "true" : "false");
    if (renderer) {
      if (panEnabled) renderer.setSetting("enableCameraPanning", true);
      else fit();      // fit() re-locks in its own callback
    }
    if (persist && onPanEnabled) onPanEnabled(panEnabled);
    return panEnabled;
  }

  // Flips which axis ribbonX/ribbonMs use and repaints everything that reads it -- the bars,
  // the brush and the year chips are all downstream of dateSpan.axis/compactAxis, so a
  // redraw (not a rebuild: dateSpan.axis already exists either way) is enough, live, with no
  // reload -- same shape as setPan above.
  function setCompactAxis(on, persist) {
    compactAxis = !!on;
    // TWO POSSIBLE BUTTONS, not one -- the settings-panel row (standalone only,
    // buildOptions) and the view-level icon (both hosts, github#23). Either or both may
    // be absent depending on host and whether the panel has been opened yet; each syncs
    // independently.
    ["opt-compactAxis", "compact"].forEach(function (id) {
      var btn = $(id);
      if (btn) btn.setAttribute("aria-pressed", compactAxis ? "true" : "false");
    });
    if (dateSpan) drawDateUI();
    if (persist && onCompactAxis) onCompactAxis(compactAxis);
    return compactAxis;
  }

  function savePng() {
    var canvases = renderer.getCanvases();
    var src = canvases.nodes;
    var out = DOC.createElement("canvas");
    out.width = src.width; out.height = src.height;
    var ctx = out.getContext("2d");
    ctx.fillStyle = css("--surface-1");
    ctx.fillRect(0, 0, out.width, out.height);
    // The logo is a DOM overlay rather than one of Sigma's canvases, and it is CSS
    // masked, so it cannot be drawImage'd. It is rebuilt here instead: paint the same
    // ring gradient into a scratch canvas, then punch it to the mask's alpha with
    // `destination-in`. Drawn underneath the graph layers, matching the on-screen
    // stacking.
    var lg = $("logo");
    if (lg && logoMaskImg && logoMaskImg.complete && !lg.hidden) {
      var w = parseFloat(lg.style.width) || 0;
      var dpr = src.width / ($("graph").clientWidth || src.width);
      if (w > 0) {
        var side = Math.round(w * dpr);
        // One masked layer. Same sweep as the CSS conic gradient (12 o'clock,
        // clockwise) drawn as filled wedges, because canvas has no conic gradient --
        // and a conic gradient is a fan of wedges anyway. Both paths take their
        // colours from ringColorsSmooth, so screen and export cannot drift.
        var layer = function (cols) {
          var lc = DOC.createElement("canvas");
          lc.width = side; lc.height = side;
          var lx = lc.getContext("2d");
          var cx = side / 2, step = 2 * Math.PI / RING_BUCKETS;
          for (var i = 0; i < RING_BUCKETS; i++) {
            if (!cols[i]) continue;
            lx.beginPath();
            lx.moveTo(cx, cx);
            // -90deg puts 0 at 12 o'clock; canvas arcs already run clockwise.
            lx.arc(cx, cx, side, i * step - Math.PI / 2, (i + 1) * step - Math.PI / 2);
            lx.closePath();
            lx.fillStyle = cols[i];
            lx.fill();
          }
          // Same mean-coloured core as the CSS gradient, over the wedges, so the
          // export does not reintroduce the centre singularity the screen hides.
          var mm = [0, 0, 0], mk = 0;
          for (var j = 0; j < RING_BUCKETS; j++) {
            if (!cols[j]) continue;
            var cc = toRgb(cols[j]); mm[0] += cc[0]; mm[1] += cc[1]; mm[2] += cc[2]; mk++;
          }
          if (mk) {
            var mr = Math.round(mm[0] / mk), mg = Math.round(mm[1] / mk), mb = Math.round(mm[2] / mk);
            var half = side / 2;
            var cg = lx.createRadialGradient(half, half, 0, half, half, half * CORE_FADE / 100);
            cg.addColorStop(0, "rgba(" + mr + "," + mg + "," + mb + ",1)");
            cg.addColorStop(CORE_SOLID / CORE_FADE, "rgba(" + mr + "," + mg + "," + mb + ",0.92)");
            cg.addColorStop(1, "rgba(" + mr + "," + mg + "," + mb + ",0)");
            lx.fillStyle = cg;
            lx.fillRect(0, 0, side, side);
          }
          lx.globalCompositeOperation = "destination-in";
          lx.drawImage(logoMaskImg, 0, 0, side, side);
          return lc;
        };

        var two = state.logoTwoRing;
        var base = layer(ringColorsSmooth());
        var innerRaw = two ? bandColors(true) : null;
        if (two && innerRaw) {
          // The inner layer gets the same radial fade the CSS mask applies, as a
          // second destination-in pass.
          var ic = layer(ringColorsSmooth(innerRaw));
          var ix = ic.getContext("2d");
          var f = LOGO_INNER_FADE.split(",");
          var r0f = parseFloat(f[0]) / 100 * (side / 2), r1f = parseFloat(f[1]) / 100 * (side / 2);
          var rg = ix.createRadialGradient(side / 2, side / 2, r0f, side / 2, side / 2, r1f);
          rg.addColorStop(0, "rgba(0,0,0,1)");
          rg.addColorStop(1, "rgba(0,0,0,0)");
          ix.globalCompositeOperation = "destination-in";
          ix.fillStyle = rg;
          ix.fillRect(0, 0, side, side);
          base.getContext("2d").drawImage(ic, 0, 0);
        }
        ctx.globalAlpha = 0.95;
        ctx.drawImage(base, (parseFloat(lg.style.left) - w / 2) * dpr,
                            (parseFloat(lg.style.top) - w / 2) * dpr, side, side);
        ctx.globalAlpha = 1;
      }
    }
    ["edges", "nodes", "labels"].forEach(function (k) {
      if (canvases[k]) ctx.drawImage(canvases[k], 0, 0);
    });
    var a = DOC.createElement("a");
    a.href = out.toDataURL("image/png");
    a.download = "codanna-graph.png";   /* codanna */
    a.click();
  }

  function buildStats() {
    // codanna: the footer reads the adapter's extras (relations, kinds, scope, dates);
    // the title is the project, the line under it says what the disc is.
    var s = DATA.stats, c = DATA.codanna || {};
    $("vname").textContent = DATA.vault;
    var scope = c.root ? esc(c.root) : "whole index";
    var rels = (c.relations || []).join(", ");
    setHTML($("vscope"), "<b>" + scope + "</b>" + (rels ? " &middot; " + esc(rels) : "") +
      (c.group && c.group !== "module/module" ? " &middot; " + esc(c.group) : ""));
    if (DOC) DOC.title = DATA.vault + " \u00b7 " + (c.root || "index") + " \u2014 Codanna Graph";
    setHTML($("stats"), "<b>" + s.nodes + "</b> symbols &middot; <b>" + s.edges + "</b> edges &middot; <b>" +
      s.orphans + "</b> unlinked<br>" +
      (c.relations ? "edges: " + esc(c.relations.join(", ")) + " &middot; kinds: " + esc((c.kinds || []).join(", ").toLowerCase()) + "<br>" : "") +
      (c.group ? "group: " + esc(c.group) + " &middot; " : "") +
      (c.root ? "scope: " + esc(c.root) + " &middot; " : "") +
      (c.symbolsTotal ? "index: " + c.symbolsTotal + " symbols, " + c.relationshipsTotal + " relationships<br>" : "") +
      (c.dates === "blame" ? "dates: per-symbol via git blame &middot; " : c.dates === "git" ? "dates: first commit of each file &middot; " : "dates: none &middot; ") +
      (c.unlinked === "drop" ? "unlinked: dropped at build &middot; " : "") +
      "Generated " + esc(DATA.generated));
  }

  function esc(s) {
    return String(s).replace(/[&<>"']/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c];
    });
  }

  /* ------------------------------------------------------------- heatmap */

  // How many notes were ADDED on each day, as a calendar of squares above the disc,
  // each square coloured by the notes that landed in it.
  //
  // WHICH DATE. `created` (frontmatter, falling back to `date`), which is the same
  // field the timeline ranks by -- so the band and the slider tell one story instead
  // of two. The two alternatives were measured and both are wrong for "added":
  //
  //   birthtime  472 of 934 files "born" today. OneDrive re-creates files on sync,
  //              so NTFS creation time says when this MACHINE first saw the file.
  //   mtime      2026-08-19 shows 240 files, which was the folder renumbering. It
  //              answers "what did I touch", which is not what this asks -- and it is
  //              why the sidebar button that once read it is gone.
  //   created    894 valid of 916, and its big day (2026-06-27, 180 notes) is the
  //              initial import -- i.e. the day those notes really were added.
  //
  // WEEKS START MONDAY, because the vault's own weeks do: weekly reviews are filed
  // by ISO week, and an ISO week starts Monday. This is not GitHub's grid.
  // A CEILING, not the answer: heatGeom picks the count that fills the band at a legible cell
  // and clamps to this. Three years, because past that the window stops being "recently" and
  // the ribbon below is the instrument for picking a year.
  // A ROLLING YEAR. It was raised to three so a wide band could be filled, and filling it is
  // not worth the trade: the window is what "notes added" is about, and a year is the span a
  // person reads a year-over-year grid as. A wide band therefore has room left over, which is
  // handled by CENTRING the grid rather than by inventing more history to put in it.
  var HEAT_WEEKS = 52;
  // 52 columns of the trailing year, WHEN THEY FIT. Measured on this vault that window
  // holds 420 of 454 dated notes; the other 30 carry CONTENT dates back to 2015 (books,
  // quotes) and are reported as a count rather than either stretching the axis over
  // eleven years of empty columns or being silently dropped -- the same call the
  // timeline makes for undated notes.
  //
  // When they do not fit, the window CROPS to the most recent weeks that do, rather
  // than the band scrolling. Scrolling was the first behaviour and it was wrong in the
  // worst possible way: the grid starts at scrollLeft 0, which is the OLDEST end, so a
  // narrow window opened on eleven empty months with every note off the right edge --
  // it read as a broken stylesheet, and was reported as one. Cropping keeps the recent
  // weeks, which is the half anyone is looking at, and it costs no chrome.
  var HEAT_WEEKS_MIN = 8;
  var HEAT_GAP = 2, HEAT_CELL_MIN = 7, HEAT_CELL_MAX = 13;
  var HEAT_GUTTER = 18;    // weekday initials
  var HEAT_MONTH_H = 12;   // month labels above the grid
  var HEAT_ARROW_W = 9;    // right margin, for the today arrow
  // NO AVERAGING, AND NO PARTIAL SQUARES. A day with any notes fills its whole cell,
  // and the cell is PIECED TOGETHER out of that day's notes: one block per note, in
  // that note's exact colour. Nothing on screen is a colour no note has. The count is
  // carried by how finely the square is divided -- one note is a solid block, 180 are
  // about a pixel each -- so a busy day reads as fine-grained and mixed and a quiet one
  // as a flat slab of a single folder's colour.
  //
  // Two earlier versions are worth knowing about, because both were measured to fail.
  //
  // AVERAGING the day's colours failed in both directions. Mixing many hues in OKLab
  // collapses toward grey, so the busiest day -- being also the most mixed -- came out
  // DULLER than a quiet single-colour day: 180 notes at OKLab L=0.713 against L=0.781
  // for a 13-note day. And with five quantile levels the count channel was flat at the
  // top (every day from 8 notes up shared the last bucket) and overlapping at the
  // bottom (1-note days reached L=0.47 while 2-note days started at 0.45).
  //
  // SIZING the square by the count worked -- measured strictly monotonic, 5.2px at one
  // note to the full 13px at 180 -- but a grid of squares that are mostly not touching
  // stops reading as a calendar, and it spends the cell's area on an axis the tooltip
  // already reports exactly. Full squares read as a surface. The cost is real and worth
  // stating: two days with the same folder mix and different counts now differ only in
  // granularity, which below about four notes is nearly invisible. The number is in the
  // tooltip; the band is for the shape of the year.
  var HEAT_EMPTY_A = 0.5;  // the unfilled lattice, so the calendar reads as a grid
  var DAY_MS = 86400000, WEEK_MS = 7 * DAY_MS;
  var MONTH_ABBR = ["Jan", "Feb", "Mar", "Apr", "May", "Jun",
                    "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  var heat = null;         // grid geometry + per-day buckets, rebuilt on resize
  var heatSig = "";        // last painted state, so a resting page repaints nothing
  var heatRz = null;

  // UTC throughout. `created` is a bare calendar date with no zone, and doing the
  // week arithmetic in local time means an hour of DST can slide a note into the
  // neighbouring column twice a year.
  function heatParse(s) {
    var m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(s || "");
    return m ? Date.UTC(+m[1], +m[2] - 1, +m[3]) : NaN;
  }
  function heatKey(ms) {
    var d = new Date(ms), p = function (n) { return (n < 10 ? "0" : "") + n; };
    return d.getUTCFullYear() + "-" + p(d.getUTCMonth() + 1) + "-" + p(d.getUTCDate());
  }
  function heatMonday(ms) {
    return ms - ((new Date(ms).getUTCDay() + 6) % 7) * DAY_MS;
  }

  // Grid geometry and the day -> notes index. Rebuilt on resize (the cell size is a
  // function of the width available) and cheap enough that it need not be
  // incremental: one pass over the nodes.
  // ONE definition of the geometry, used by the build and by the reflow guard. Two
  // copies of this arithmetic is how the guard ends up disagreeing with the build and
  // the band quietly stops resizing.
  //
  // THE WINDOW IS AS MANY WEEKS AS THE BAND CAN SHOW at a legible cell -- not 52 of them with
  // the rest of the band left empty.
  //
  // It used to take 52 and grow the cell to fill, which cannot work: the cell is capped at
  // HEAT_CELL_MAX because the band is seven cells TALL and a 33px cell would be a 245px band
  // eating the disc. So the cap bound first and the leftover width simply stayed empty --
  // measured, 52 cols at 13px is 805px of a 1268px band, and on a 1900px window barely half of
  // it. Two instruments about the same axis, the strip below spanning the full width and the
  // grid above stopping in the middle.
  //
  // So the CELL is what is fixed and the COLUMNS are what flex: pick the count that lands the
  // cell near its cap and fills the width. A wide band shows more history at the same
  // legibility, which is the useful direction -- and the readout already says "last N weeks"
  // from this number, so it stays honest without a second change.
  //
  // Bounded by the vault's own span, because a window reaching past the first note is empty
  // columns pretending to be data.
  function heatGeom() {
    var wrap = $("heatwrap");
    var avail = ((wrap && wrap.clientWidth) || $("stage").clientWidth || 900) - HEAT_GUTTER;
    avail -= HEAT_ARROW_W;
    // At the target cell, then clamped: never below the minimum legible count, never past the
    // ceiling, never wider than the vault is old.
    var want = Math.floor((avail + HEAT_GAP) / (HEAT_CELL_MAX + HEAT_GAP));
    var span = dateSpan ? Math.ceil((dateSpan.hi - dateSpan.lo) / WEEK_MS) + 1 : HEAT_WEEKS;
    var cols = Math.max(HEAT_WEEKS_MIN, Math.min(HEAT_WEEKS, span, want));
    // Whatever the count came out as, spend the width on the cell rather than leaving a gap.
    var cell = Math.floor((avail - (cols - 1) * HEAT_GAP) / cols);
    return { cols: cols, cell: Math.max(HEAT_CELL_MIN, Math.min(HEAT_CELL_MAX, cell)) };
  }

  function heatBuild() {
    var wrap = $("heatwrap"), cv = $("heatc");
    if (!wrap || !cv) return;

    var g = heatGeom();
    var cols = g.cols, cell = g.cell;
    var pitch = cell + HEAT_GAP;
    // THE WINDOW'S RIGHT EDGE IS STATE NOW, not always today. A fixed sliding window onto
    // today is what made everything before it unreachable -- on the 10-year fixture that is
    // nine years of the vault with nothing to point at. `heatEnd` is null for "the last 52
    // weeks", which is still the default and still where it opens.
    var endMs = state.heatEnd === null ? heatParse(TODAY) : state.heatEnd;
    var start = heatMonday(endMs) - (cols - 1) * WEEK_MS;

    var days = Object.create(null), keys = [];
    for (var c = 0; c < cols; c++) {
      for (var r = 0; r < 7; r++) {
        var ms = start + c * WEEK_MS + r * DAY_MS;
        var k = heatKey(ms);
        days[k] = { key: k, ms: ms, col: c, row: r, ids: [], parts: [], n: 0 };
        keys.push(k);
      }
    }

    // Every dated note, bucketed. Notes outside the window are counted, not binned:
    // the readout says how many, so a short axis never reads as a complete one.
    var before = 0, after = 0, undated = 0, all = Object.create(null);
    graph.forEachNode(function (id, a) {
      var k = a.created;
      if (!heatParse(k)) { undated++; return; }
      all[k] = (all[k] || 0) + 1;
      var d = days[k];
      if (d) d.ids.push(id);
      else if (heatParse(k) < start) before++;
      else after++;
    });

    // Quantile cuts from the FULL data set, exactly as the group colours are: a
    // filter must not re-scale the survivors, or hiding one folder repaints every
    // remaining day a different shade for no reason the reader can see.
    var counts = [];
    for (var kk in all) counts.push(all[kk]);
    counts.sort(function (x, y) { return x - y; });
    var q = function (p) {
      return counts.length
        ? counts[Math.min(counts.length - 1, Math.floor(p * counts.length))] : 1;
    };
    var cuts = [q(0.2), q(0.4), q(0.6), q(0.8)];
    // The top of the size ramp. Taken from the WINDOW's days only: a day off the left
    // edge cannot grow a square that is on screen, so letting it set the scale would
    // do nothing but shrink every square the reader can actually see.
    var nMax = 1;
    for (var w = 0; w < keys.length; w++) {
      var wn = days[keys[w]].ids.length;
      if (wn > nMax) nMax = wn;
    }

    heat = {
      cols: cols, cell: cell, pitch: pitch, start: start, days: days, keys: keys,
      cuts: cuts, nMax: nMax, before: before, after: after, undated: undated,
      dated: counts.length,
      w: HEAT_GUTTER + cols * pitch - HEAT_GAP + HEAT_ARROW_W,
      h: HEAT_MONTH_H + 7 * pitch - HEAT_GAP
    };

    // Sorted by HUE, so each square is its own little gradient -- a mini heatmap
    // inside the heatmap -- rather than confetti.
    //
    // Folder order was the first attempt and it is the wrong key: the group palette is
    // assigned in NAME order (01, 02, 03 ...) precisely so a folder keeps its colour as
    // the vault grows, and name order is not hue order. Measured on this palette,
    // consecutive folders run blue 264deg, orange 42deg, aqua 168deg -- so grouping by
    // folder puts the three most distant hues on the wheel side by side. Hue angle
    // groups the folders that ARE close (aqua next to green next to cyan) and reads as
    // one surface.
    //
    // Tie-broken by lightness, which is the axis the subfolder tints move along, so
    // siblings inside one hue family also come out in order instead of shuffled.
    //
    // Done here rather than per frame: it depends on nodeColor(), which regroup() has
    // already settled by the time heatBuild runs, and the palette is stable for the
    // life of the data. Re-deriving it 60 times a second buys nothing.
    var hkey = Object.create(null);
    graph.forEachNode(function (id) {
      var c = nodeColor(id), l = hex2lab(c);
      hkey[id] = [hueOf(c), l[0]];
    });
    keys.forEach(function (k) {
      days[k].ids.sort(function (a, b) {
        return hkey[a][0] - hkey[b][0] || hkey[a][1] - hkey[b][1];
      });
    });

    var dpr = window.devicePixelRatio || 1;
    cv.width = Math.round(heat.w * dpr);
    cv.height = Math.round(heat.h * dpr);
    cv.style.width = heat.w + "px";
    cv.style.height = heat.h + "px";

    var inWin = 0;
    for (var i = 0; i < keys.length; i++) inWin += days[keys[i]].ids.length;
    $("heatnote").textContent =
      "last " + cols + " weeks · " + inWin + " of " + graph.order + " symbols" +   /* codanna: vocabulary */
      (before ? " · " + before + " earlier" : "") +
      (after ? " · " + after + " later" : "") +
      (undated ? " · " + undated + " undated" : "");

    heatSig = "";
    heatDraw();
  }

  // Which of the five legend steps a day falls in. Not used for drawing -- the ramp is
  // continuous -- but it is what heatReport counts, and the quantile cuts are still
  // the honest way to ask whether the band is using its range.
  function heatLevel(n) {
    var c = heat.cuts;
    for (var i = 0; i < c.length; i++) if (n <= c[i]) return i;
    return c.length;
  }

  // Tile a square with one block per note, exactly -- no gaps, no overlap, no colour
  // that is not a note's own.
  //
  // Vertical strips, each strip split into horizontal bands. Blocks come out roughly
  // square (strips = round(sqrt(n))) and the arithmetic tiles the square exactly at
  // any n, which a row-major grid does not: ceil(sqrt(n)) rows leaves the last one
  // part-empty, and a ragged corner reads as a different count.
  //
  // The notes arrive sorted by hue and are consumed in order, strip by strip and top
  // to bottom within a strip -- so the square sweeps the hue wheel from its top-left
  // to its bottom-right, and a day's folder mix reads as bands rather than as noise.
  function heatTile(ctx, x, y, side, parts) {
    var n = parts.length;
    if (!n) return;
    var cols = Math.max(1, Math.round(Math.sqrt(n)));
    for (var c = 0; c < cols; c++) {
      var i0 = Math.floor(c * n / cols), i1 = Math.floor((c + 1) * n / cols);
      if (i1 <= i0) continue;
      var x0 = x + side * c / cols, x1 = x + side * (c + 1) / cols;
      var m = i1 - i0;
      for (var j = 0; j < m; j++) {
        var q = parts[i0 + j];
        var y0 = y + side * j / m, y1 = y + side * (j + 1) / m;
        ctx.globalAlpha = q.w;
        ctx.fillStyle = q.c;
        ctx.fillRect(x0, y0, x1 - x0, y1 - y0);
      }
    }
    ctx.globalAlpha = 1;
  }

  // Weight is ALPHA, not membership -- the same source of truth the disc reads. So the
  // band fills in as the timeline plays and dims as a folder fades out, and it needs no
  // filter clause of its own. A note mid-fade contributes a translucent block, so it
  // arrives the way it arrives on the disc rather than popping in.
  function heatCompute() {
    for (var i = 0; i < heat.keys.length; i++) {
      var d = heat.days[heat.keys[i]];
      d.n = 0;
      d.parts.length = 0;
      for (var j = 0; j < d.ids.length; j++) {
        var id = d.ids[j], w = alpha[id] || 0;
        if (w <= 0.004) continue;
        d.n += w;
        d.parts.push({ c: nodeColor(id), w: w });
      }
    }
  }

  function heatDraw() {
    var cv = $("heatc");
    if (!heat || !cv || !cv.getContext) return;
    heatCompute();

    // Repaint only when something changed. This runs from afterRender, so it fires
    // on every frame of every animation; at rest that would be 364 rectangles of
    // identical work per frame. Quantising to a quarter of a note keeps the guard
    // from flickering on floating-point noise while still catching a real fade.
    var sig = [];
    for (var i = 0; i < heat.keys.length; i++) {
      sig.push(Math.round(heat.days[heat.keys[i]].n * 4));
    }
    sig.push(state.markDay || "", state.hoverDay || "", heat.cell);
    sig = sig.join(",");
    if (sig === heatSig) return;
    heatSig = sig;

    var dpr = window.devicePixelRatio || 1;
    var ctx = cv.getContext("2d");
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, heat.w, heat.h);

    var cell = heat.cell, pitch = heat.pitch;
    var R = Math.max(2, Math.round(cell * 0.22));

    // WHERE TODAY IS. Always the LAST column, by construction: `start` is
    // (cols - 1) weeks before this Monday, so the trailing column is always the
    // current week. That is exactly why the marker does not point at the column --
    // the column is a foregone conclusion and pointing at it says nothing. The row
    // is the part that moves, so the arrow lives in the right margin and points at
    // today's cell, which names the weekday as well as the day.
    var td = heat.days[TODAY];

    // Month labels, at the first column whose week opens a month. Skipped when one
    // would collide with the previous, which is what happens at the 7px cell floor.
    ctx.font = "9px ui-sans-serif, -apple-system, 'Segoe UI', system-ui, sans-serif";
    ctx.textBaseline = "alphabetic";
    ctx.fillStyle = THEME.text;
    ctx.globalAlpha = 0.45;
    var lastEnd = -99;
    for (var c = 0; c < heat.cols; c++) {
      var first = new Date(heat.start + c * WEEK_MS);
      if (first.getUTCDate() > 7) continue;      // this week does not open a month
      var x = HEAT_GUTTER + c * pitch;
      if (x < lastEnd + 4) continue;
      var lab2 = MONTH_ABBR[first.getUTCMonth()];
      ctx.fillText(lab2, x, HEAT_MONTH_H - 3);
      lastEnd = x + ctx.measureText(lab2).width;
    }
    ctx.globalAlpha = 1;

    // TODAY: an arrow in the right margin, on today's row, pointing back at its cell.
    // Drawn unconditionally -- NOT as one more ring. Every ring on this band is
    // --today and they differ only in weight, so today's own ring is structurally the
    // one that loses: it is the weakest of three, and it disappears entirely the
    // moment the same cell is hovered or picked, because the three are one if/else
    // chain. A mark outside that competition is what makes "where is today"
    // answerable at a glance whatever else is selected.
    if (td) {
      var ax = HEAT_GUTTER + heat.cols * pitch - HEAT_GAP;   // right edge of the grid
      var ay = HEAT_MONTH_H + td.row * pitch + cell / 2;
      ctx.fillStyle = THEME.today;
      ctx.beginPath();
      ctx.moveTo(ax + 2.5, ay);
      ctx.lineTo(ax + HEAT_ARROW_W - 1, ay - 3.5);
      ctx.lineTo(ax + HEAT_ARROW_W - 1, ay + 3.5);
      ctx.closePath();
      ctx.fill();
    }

    // Weekday initials: Mon / Wed / Fri only. Seven labels at a 9px font on a 9px
    // pitch is a grey smear, and three is enough to orient the grid.
    var INIT = ["M", "", "W", "", "F", "", ""];
    ctx.fillStyle = THEME.text;
    ctx.globalAlpha = 0.45;
    for (var r = 0; r < 7; r++) {
      if (INIT[r]) ctx.fillText(INIT[r], 0, HEAT_MONTH_H + r * pitch + cell - 1);
    }
    ctx.globalAlpha = 1;

    for (var i2 = 0; i2 < heat.keys.length; i2++) {
      var d = heat.days[heat.keys[i2]];
      var x2 = HEAT_GUTTER + d.col * pitch, y2 = HEAT_MONTH_H + d.row * pitch;

      // The empty slot first, always: it is what makes the band read as a CALENDAR
      // rather than as a scatter of squares, and it gives the eye the frame that the
      // filled square's size is judged against.
      ctx.globalAlpha = HEAT_EMPTY_A;
      ctx.fillStyle = THEME.dim;
      heatRect(ctx, x2, y2, cell, cell, R);
      ctx.fill();
      ctx.globalAlpha = 1;

      // Then the day itself: the WHOLE cell, one block per note, each block that
      // note's exact colour. Clipped to the same rounded rect as the empty slot, so a
      // one-note day and a 180-note day are the same shape and only their grain
      // differs. Blocks are plain rectangles; the corners come from the clip, which is
      // one path per cell instead of one per note.
      if (d.n > 0.004) {
        ctx.save();
        heatRect(ctx, x2, y2, cell, cell, R);
        ctx.clip();
        heatTile(ctx, x2, y2, cell, d.parts);
        ctx.restore();
      }

      // Rings, all in --today: the neutral extreme of the palette, deliberately not
      // a group hue, so a ring can never be misread as a folder. Three weights --
      // picked, hovered, and today -- in that priority, because a cell can be all
      // three at once and only one ring can be drawn.
      if (d.key === state.markDay) {
        ctx.strokeStyle = THEME.today;
        ctx.lineWidth = 1.5;
        heatRect(ctx, x2 - 1, y2 - 1, cell + 2, cell + 2, R + 1);
        ctx.stroke();
      } else if (d.key === state.hoverDay) {
        ctx.strokeStyle = THEME.today;
        ctx.globalAlpha = 0.75;
        ctx.lineWidth = 1;
        heatRect(ctx, x2 - 1, y2 - 1, cell + 2, cell + 2, R + 1);
        ctx.stroke();
        ctx.globalAlpha = 1;
      } else if (d.key === TODAY) {
        // Full strength, but 1px against a selection's 1.5px -- it was 0.4 alpha and
        // simply could not be seen. The caret above the column carries the finding;
        // this ring is what confirms which cell the caret is pointing at.
        ctx.strokeStyle = THEME.today;
        ctx.lineWidth = 1;
        heatRect(ctx, x2 - 1, y2 - 1, cell + 2, cell + 2, R + 1);
        ctx.stroke();
      }
    }

    heatDrawKey(cell, R);
  }

  // The legend is a GRANULARITY ramp, because grain is what the count is encoded in.
  // Sampled at the vault's OWN quantiles (1 / 2 / 3 / 7 / busiest) rather than at five
  // arbitrary steps, so each square is a count this vault actually has, and drawn
  // through the same heatTile as the cells so it cannot drift from them.
  //
  // Two NEUTRALS, alternating, deliberately: the legend is about how finely a square
  // is divided, and any hue here would read as a claim about which folder. They are the
  // palette's own greys, so they belong to the same family as everything else.
  function heatDrawKey(cell, R) {
    var cv = $("heatkey");
    if (!cv || !cv.getContext) return;
    // Deduped: the quantiles collapse on a vault whose days are mostly 1 note --
    // measured here they came out 1/1/2/5, and two identical swatches read as a
    // rendering fault rather than as a tie. Fewer steps is the honest answer; the
    // canvas width follows the count.
    var anchors = [];
    heat.cuts.concat([heat.nMax]).forEach(function (a) {
      if (anchors.indexOf(a) < 0) anchors.push(a);
    });
    var pitch = cell + 4;
    var dpr = window.devicePixelRatio || 1;
    var w = anchors.length * pitch - 4;
    if (cv.width !== Math.round(w * dpr) || cv.height !== Math.round(cell * dpr)) {
      cv.width = Math.round(w * dpr);
      cv.height = Math.round(cell * dpr);
      cv.style.width = w + "px";
      cv.style.height = cell + "px";
    }
    var ctx = cv.getContext("2d");
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, cell);
    var greys = [THEME.neutrals[0], THEME.neutrals[2]];
    for (var i = 0; i < anchors.length; i++) {
      var parts = [];
      for (var j = 0; j < anchors[i]; j++) parts.push({ c: greys[j % 2], w: 1 });
      ctx.save();
      heatRect(ctx, i * pitch, 0, cell, cell, R);
      ctx.clip();
      heatTile(ctx, i * pitch, 0, cell, parts);
      ctx.restore();
    }
    cv.title = anchors.map(function (a) {
      return a + (a === 1 ? " symbol" : " symbols");   /* codanna: vocabulary */
    }).join("  ·  ");
  }

  // roundRect is not everywhere yet, and this page runs from file:// in whatever
  // browser is set as default, so the path is built by hand.
  function heatRect(ctx, x, y, w, h, r) {
    r = Math.min(r, w / 2, h / 2);
    ctx.beginPath();
    ctx.moveTo(x + r, y);
    ctx.arcTo(x + w, y, x + w, y + h, r);
    ctx.arcTo(x + w, y + h, x, y + h, r);
    ctx.arcTo(x, y + h, x, y, r);
    ctx.arcTo(x, y, x + w, y, r);
    ctx.closePath();
  }

  // Which day is under the pointer, or null. Hit-tested arithmetically rather than
  // by walking the cells -- it is a lattice, so there is nothing to search.
  function heatHit(ev) {
    if (!heat) return null;
    var b = $("heatc").getBoundingClientRect();
    var x = ev.clientX - b.left - HEAT_GUTTER, y = ev.clientY - b.top - HEAT_MONTH_H;
    if (x < 0 || y < 0) return null;
    var c = Math.floor(x / heat.pitch), r = Math.floor(y / heat.pitch);
    if (c < 0 || c >= heat.cols || r < 0 || r > 6) return null;
    // Inside the cell, not merely inside its slot: the gap between two squares
    // should not report the one to its left.
    if (x - c * heat.pitch > heat.cell || y - r * heat.pitch > heat.cell) return null;
    return heat.days[heatKey(heat.start + c * WEEK_MS + r * DAY_MS)] || null;
  }

  var HEAT_WD = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
  function heatShowTip(d) {
    var t = $("htip"), n = Math.round(d.n);
    var by = Object.create(null);
    for (var i = 0; i < d.ids.length; i++) {
      if ((alpha[d.ids[i]] || 0) <= 0.004) continue;
      var g = groupOf(d.ids[i]);
      by[g] = (by[g] || 0) + 1;
    }
    var top = Object.keys(by).sort(function (a, b) { return by[b] - by[a]; }).slice(0, 3);
    var wd = HEAT_WD[(new Date(d.ms).getUTCDay() + 6) % 7];
    setHTML(t, '<div class="t">' + esc(d.key) + " · " + wd +
      (d.key === TODAY ? " · today" : "") + "</div>" +
      '<div class="m">' +
      (n ? n + " symbol" + (n === 1 ? "" : "s") + " added" : "nothing added") +   /* codanna: vocabulary */
      (top.length ? "<br>" + top.map(function (g2) {
        return '<b style="color:' + colorOf(g2) + '">■ </b> ' + esc(g2) + " " + by[g2];
      }).join("<br>") : "") +
      (n ? "<br><i>click to mark them on the disc</i>" : "") +
      "</div>");
    t.hidden = false;
    var box = t.getBoundingClientRect();
    var host = $("heat").getBoundingClientRect(), cv = $("heatc").getBoundingClientRect();
    var cx = cv.left - host.left + HEAT_GUTTER + d.col * heat.pitch + heat.cell / 2;
    var cy = cv.top - host.top + HEAT_MONTH_H + d.row * heat.pitch;
    t.style.left = Math.max(4, Math.min(cx - box.width / 2, host.width - box.width - 4)) + "px";
    // Below the square when there is no room above it, which there is not for the
    // top rows of a band this short.
    var above = cy - box.height - 8;
    t.style.top = (above >= 2 ? above : cy + heat.cell + 8) + "px";
  }

  function buildHeatmapUI() {
    var cv = $("heatc");
    // Hovering a square haloes that day's notes on the disc. Refreshed only when the
    // day under the pointer actually CHANGES -- mousemove fires many times per cell,
    // and a renderer refresh per event would repaint the disc dozens of times while
    // crossing one square.
    var setHover = function (key) {
      if (state.hoverDay === key) return;
      state.hoverDay = key;
      heatSig = "";            // the hovered cell gets its own ring
      renderer.refresh();      // colour and halo only; nothing moves
    };
    cv.addEventListener("mousemove", function (ev) {
      var d = heatHit(ev);
      if (d) heatShowTip(d); else $("htip").hidden = true;
      cv.style.cursor = (d && d.n > 0.004) ? "pointer" : "default";
      setHover(d && d.n > 0.004 ? d.key : null);
    });
    cv.addEventListener("mouseleave", function () {
      $("htip").hidden = true;
      setHover(null);
    });
    // Clicking a day marks its notes on the disc -- recoloured, haloed, not moved -- and
    // this is the whole reason the band sits above the rings rather than in the sidebar:
    // the question a heatmap raises is "what were those notes?"
    //
    // IT IS ALSO "MARK TODAY" NOW. That was a sidebar toggle doing this to today's column
    // by a second predicate; clicking the last cell of the band does it by this one, in the
    // place the answer is already drawn.
    cv.addEventListener("click", function (ev) {
      var d = heatHit(ev);
      if (!d || d.n <= 0.004) return;
      state.markDay = (state.markDay === d.key) ? null : d.key;
      heatSig = "";
      // No applyLayout: a marked day only changes colour and halo, so no note's
      // target position moves. Running the tween anyway would animate every note
      // from where it is to where it already is.
      renderer.refresh();
    });
    // The cell size is a function of the width available, so the grid has to be
    // re-derived whenever that width moves. A window `resize` listener is NOT
    // enough, and this was measured: the band came up at the 7px cell floor in a
    // 1124px slot, because boot ran before the embedded pane had settled to its
    // final width and no window resize event ever followed. A ResizeObserver
    // answers the question actually being asked -- how wide is this box -- and
    // fires on its first observation as well, so the boot race is the same code
    // path as a later resize rather than a special case.
    //
    // No rebuild loop: heatBuild only writes the CANVAS size, which changes the
    // wrapper's scroll width and not its client width. Guarded on the derived cell
    // size anyway, so an observation that changes nothing costs nothing.
    var reflow = function () {
      if (heatRz) WIN.clearTimeout(heatRz);
      heatRz = WIN.setTimeout(function () {
        heatRz = null;
        var g = heatGeom();
        if (heat && g.cell === heat.cell && g.cols === heat.cols) return;
        heatBuild();
      }, 60);
    };
    if (window.ResizeObserver) new ResizeObserver(reflow).observe($("heatwrap"));
    else window.addEventListener("resize", reflow);
  }

  /* ---------------------------------------------------------------- demo */

  // A scripted walkthrough, for watching and for recording. `?demo` in the URL arms it.
  //
  // THE PAGE DOES NOT PERFORM THE INPUT. It publishes a storyboard and answers two
  // questions about itself -- where is that control, and are you still moving -- and a
  // driver outside the browser does the clicking through Chrome's DevTools protocol
  // (`Input.dispatchMouseEvent`). See `scripts/demo.mjs`.
  //
  // Why not just call el.click() from in here, which is far less machinery: because
  // that is not what a user does, and the difference is exactly the part worth
  // demonstrating. A dispatched click skips hit-testing entirely -- it fires on an
  // element that is covered by something else, or scrolled out of view, or 0x0 -- so an
  // in-page demo keeps passing after the button it aims at has become unclickable. CDP
  // input goes in at the top of the same pipeline a mouse does: it hit-tests, it raises
  // the hover states the page draws for real, and it fails when a real click would.
  //
  // An earlier cut drew its own SVG arrow and dispatched synthetic MouseEvents to move
  // it. Removed. Besides the hit-testing hole, the mark fought the physical mouse:
  // measured, it finished a run at (0, 847) instead of on the button it had just
  // clicked at (42, 650), because clicking the eye rebuilds the legend's DOM and Chrome
  // then re-dispatches a mousemove at the REAL cursor -- so a mouse parked anywhere
  // teleported the mark on every rebuild.

  var DEMO_DONE_TITLE = "vault-graph demo complete";

  /* ------------------------------------------------------------ date range --
   * A BRUSH OVER THE WHOLE HISTORY, under the band.
   *
   * The band is a 52-week sliding window onto today, so everything before it is
   * unreachable -- on a ten-year vault that is nine years with nothing to point at. This is
   * the axis that reaches them, and the two ends of the brush are the filter.
   *
   * WHY THE AXIS IS LINEAR IN TIME, which is the one real decision here. Measured on the
   * author's vault: 452 notes over 11.4 years, 389 of them in 2026, and a whole year (2021)
   * holding none. A time axis therefore spends most of its width on very little -- the
   * sidebar's timeline slider dodges exactly this by being linear in note count instead.
   *
   * It is kept anyway, because the two are not the same instrument. A slider is for
   * *setting* a value and wants its travel spent evenly. This is for *finding* one, and what
   * makes a date findable is that it sits where you expect on a calendar: 2019 is at 2019,
   * the burst is visibly a burst, and the year with nothing in it is visibly empty. Spending
   * the width proportionally to notes would put 2021 and 2026 side by side at the same size
   * and lose all three of those facts. Two other concepts were built -- a year rail beside
   * the band, and twin rank-linear sliders -- and this is the one that survived.
   *
   * The bars are sqrt-scaled for the same reason the disc's node sizes are: one month here
   * holds 631 notes and most hold under 40, so a linear height is one bar and a flat line.
   */

  /* THREE LANES, because the strip carries two independent things and a legend.
   *
   *   bars    the months, and the BRUSH -- the two ends of the date filter
   *   track   the band's own 52-week window, draggable on its own
   *   labels  the year ticks
   *
   * The window used to follow whichever brush end was being dragged, which is convenient
   * right up to the point where you want to look at one stretch while filtering to another.
   * They are separate instruments -- the brush says which notes count, the window says which
   * weeks the grid above is drawing -- so they get separate lanes and separate gestures.
   * Which lane the pointer is in decides what it grabs; nothing has to be moded.
   */
  var RIBBON_BARS = 26;      // the bars and the brush
  var RIBBON_TRACK = 14;     // the band-window track
  // The year labels used to be painted in a band at the bottom of this canvas. They are real
  // buttons under it now (#vg-years), so the canvas is shorter by exactly that band and the
  // strip is nothing but the strip.
  var RIBBON_H = RIBBON_BARS + RIBBON_TRACK;
  var GRAB_PX = 6;           // how close to an edge counts as grabbing it
  var DRAG_MIN = 3;          // px before a press counts as a drag rather than a click

  var brushDrag = null;
  /**
   * The intro's right-hand scrubber, mid-flight, in ms -- or null when nothing is sweeping.
   *
   * The intro grows the vault from its first note to now, and that IS the range end travelling
   * from one end of the strip to the other; the two were simply not connected. It used to
   * drive a rank slider in the sidebar, and when that went the animation had nothing left to
   * say what it was doing -- the disc filled up and the one control that shows the history sat
   * still through all of it.
   *
   * A PREVIEW, exactly like brushDrag: state.from and state.to are untouched for the whole
   * sweep. Writing them per frame would put a hard date cap in timeFactor on top of the rank
   * ramp the cascade is already animating, which is the same reveal computed twice -- and the
   * second one would fight the first, since a range change goes through applyRange -> cascade
   * and cascade stops playback. The disc's reveal stays the cascade's; this is the strip
   * saying where the reveal has got to.
   */
  var brushSweep = null;

  // REMEASURE FIRST. Every path that redraws the strip is also a path where the slot may
  // have changed -- a window resize, an Obsidian pane drag, the band's own reflow -- and the
  // bars, the brush and the year buttons all have to come out on one scale. Measuring here
  // is what makes that true by construction instead of by three callers remembering to.
  function drawDateUI() {
    ribW = measureRibbon() || ribW;
    drawRibbon();
    buildYears();
  }

  /**
   * One button per year, under the strip, at that year's own position on it.
   *
   * Rebuilt rather than repositioned, because the set of years that FITS changes with the
   * width: at 11 years on a narrow band the labels collide, so alternate ones are dropped --
   * and a dropped year should not be a button nobody can see. Cheap enough to redo: a dozen
   * elements, and only when the band is laid out or the range changes.
   *
   * The pressed state is a real one: a range that spans exactly one calendar year marks that
   * year, so the strip says which one is selected rather than only offering to select.
   */
  function buildYears() {
    var host = $("years");
    if (!host) return;
    if (!dateSpan || !dateSpan.years.length) { host.textContent = ""; return; }
    var w = ribbonW();
    // Room for a label is measured, not estimated from the calendar span -- that estimate
    // (average px per YEAR over the whole span) was accurate while the axis was linear, but
    // github#23's note-weighted axis can put two sparse years' chips much closer together
    // than the average while a busy year sits comfortably wide elsewhere; a vault-wide
    // average never sees that. The worst (smallest) gap between any two CONSECUTIVE years'
    // actual positions is what a label can actually collide with, so that's what decides.
    var positions = dateSpan.years.map(function (yy) {
      return Math.max(0, Math.min(w, ribbonX(Date.UTC(yy.y, 0, 1), w)));
    });
    var minGap = Infinity;
    for (var gi = 1; gi < positions.length; gi++) {
      minGap = Math.min(minGap, positions[gi] - positions[gi - 1]);
    }
    // ~28px, not 20: a two-digit chip ("'26") with 5px padding either side runs close to
    // that wide, and two chips only clear each other when their CENTRES -- not their edges
    // -- are that far apart, since each is centred on its own position.
    var every = (positions.length > 1 && minGap < 28) ? 2 : 1;
    // WHICH YEAR IS SELECTED, read through the same clamping that setting it goes through.
    // An end AT the span's own end is stored as null -- "no bound" -- so the newest year, whose
    // December is past the last note, comes back as `to === null` and compared as itself: the
    // button for the year you just clicked read unpressed, on every vault whose span ends
    // mid-year. Resolving nulls to the span's ends first is all it needs.
    var cur = null;
    var cf = state.from === null ? dateSpan.lo : state.from;
    var ct = state.to === null ? dateSpan.hi : state.to;
    var ca = new Date(cf), cb = new Date(ct);
    if (ca.getUTCFullYear() === cb.getUTCFullYear() &&
        ca.getUTCMonth() === 0 && ca.getUTCDate() === 1 &&
        (cb.getUTCMonth() === 11 && cb.getUTCDate() === 31 ||
         ct >= dateSpan.hi)) cur = ca.getUTCFullYear();
    // BUILT AS ELEMENTS, not as a string assigned to innerHTML.
    //
    // The old version concatenated markup and assigned it. Every value in it was a number
    // out of dateSpan, so nothing could have been injected -- but the Obsidian directory's
    // linter rejects the assignment on sight (no-unsanitized/property), and it is right to:
    // the safety of that line rested on an argument about where its inputs came from, made
    // in one place and checked nowhere. Building elements moves the question out of reach
    // instead of answering it, and textContent cannot be markup whatever it holds.
    //
    // replaceChildren() rather than innerHTML = "": same clear, and it is the verb for it.
    var made = [];
    dateSpan.years.forEach(function (yy, yi) {
      if ((yy.y % every) !== 0) return;
      var at = positions[yi];        // already computed above, same ribbonX call
      var b = DOC.createElement("button");
      b.type = "button";
      b.setAttribute("data-yr", String(yy.y));
      b.setAttribute("aria-pressed", cur === yy.y ? "true" : "false");
      b.title = yy.y + " -- " + yy.n + " symbol" + (yy.n === 1 ? "" : "s");   /* codanna: vocabulary */
      // Dynamic by nature -- it is the year's own place on the strip -- so it stays inline.
      // setProperty rather than an assignment: the value is not static, so the rule does not
      // fire either way, and this form cannot start firing if the rule ever tightens.
      b.style.setProperty("left", Math.round(at) + "px");
      b.textContent = "'" + String(yy.y).slice(2);
      made.push(b);
    });
    host.replaceChildren.apply(host, made);
  }

  // A canvas sized for the device, drawn in CSS pixels. Same treatment the heat band gets;
  // without it every one-pixel rule in here lands on a half pixel and greys out.
  function fitCanvas(cv, w, h) {
    var dpr = Math.min(2, WIN.devicePixelRatio || 1);
    cv.width = Math.round(w * dpr); cv.height = Math.round(h * dpr);
    cv.style.width = w + "px"; cv.style.height = h + "px";
    var cx = cv.getContext("2d");
    cx.setTransform(dpr, 0, 0, dpr, 0, 0);
    cx.clearRect(0, 0, w, h);
    return cx;
  }

  /**
   * The bar ramp: the surface at nothing, the accent at the busiest month.
   *
   * It read `heat.mean` first, falling back to the accent -- and `heat.mean` is set to null
   * where the heat object is built and never assigned anywhere, so the fallback has always
   * been the whole function. Written as what it does. The accent is a validated colour and
   * "the mean of the group palette" was a guess, so this is the better of the two anyway.
   */
  function dateRamp(t) {
    return t <= 0 ? css("--dim")
                  : mixHex(css("--surface-2"), css("--accent"), 0.25 + 0.75 * Math.min(1, t));
  }

  /**
   * The scrubbers' colour: the bars' own hue, pushed for contrast against them.
   *
   * "Same colour as the bars" was already true and that was the problem -- the handles were
   * drawn at the accent and a busy month's bar IS the accent, so a handle standing on a tall
   * bar disappeared into it. Same hue, moved along the light/dark axis instead: toward
   * --text-1, which is near-black on the light theme and white on the dark one, so the shift
   * is away from the bars in both rather than toward white in one and invisible in the other.
   */
  function scrubColor() { return mixHex(css("--accent"), css("--text-1"), 0.3); }

  // withAlpha takes an rgba string and mixHex takes two hexes; neither does this one thing.
  function rgbaHex(hex, a) {
    var c = toRgb(hex);
    return "rgba(" + c[0] + "," + c[1] + "," + c[2] + "," + a + ")";
  }

  // The strip's width in CSS pixels, measured once per layout and cached until something
  // moves. Zero means "not measured yet".
  var ribW = 0;

  /**
   * How wide the strip's SLOT is -- asked of the slot, never of the canvas.
   *
   * This is the bug the cache exists for. fitCanvas has to pin an inline pixel width on the
   * canvas: the bitmap is sized in device pixels and the CSS box has to stay in CSS pixels,
   * so it writes both. An inline width beats the stylesheet's `width:100%`, so from the first
   * paint onward the canvas IS whatever it was last drawn at -- and asking it how wide it is
   * returns that number for ever. Every later measurement confirms the first one.
   *
   * Measured on the real vault in a 1284px slot: the strip was 600px wide -- the fallback
   * below, taken because the very first call ran before the band had been laid out -- and it
   * stayed 600px through every window resize, because the ResizeObserver's redraw asked the
   * canvas and the canvas answered 600. The bars, the brush and the year buttons were all on
   * that scale, so the whole control simply did not resize.
   *
   * Dropping the inline width first and letting the stylesheet answer is the measurement. It
   * costs one forced layout, which is why it happens from drawDateUI and from the observer
   * only, and never from a pointermove: a drag reads the cache.
   */
  // MEASURING LEAVES NOTHING BEHIND. The inline width is restored whichever way this
  // returns, so the only thing that ever sets it is fitCanvas -- which sets the bitmap in
  // the same breath, and those two have to agree or a fractional slot width puts the CSS
  // box half a pixel off the pixels behind it. An earlier version dropped the inline width
  // to measure and only restored it when the measurement failed, which left the canvas
  // sized by the stylesheet after any observation the guard below short-circuited: the same
  // number by luck, and a stretched bitmap the moment the luck ran out.
  function measureRibbon() {
    var cv = $("ribbon");
    if (!cv) return 0;
    // removeProperty / setProperty rather than `style.width = ""`. An empty string IS a
    // static value, which obsidianmd/no-static-styles-assignment rejects -- and the DOM has
    // a verb for "unset this" that says what is meant more plainly than assigning "".
    var keep = cv.style.width;
    cv.style.removeProperty("width");
    var w = cv.getBoundingClientRect().width;
    if (keep) cv.style.setProperty("width", keep);
    return w;
  }
  function ribbonW() {
    if (!ribW) ribW = measureRibbon();
    return ribW || 600;
  }
  function ribbonXLinear(ms, w) {
    var span = dateSpan.hi - dateSpan.lo;
    return span > 0 ? ((ms - dateSpan.lo) / span) * w : 0;
  }
  function ribbonMsLinear(x, w) {
    return dateSpan.lo + (Math.max(0, Math.min(w, x)) / w) * (dateSpan.hi - dateSpan.lo);
  }

  // Month index for `ms`, by calendar arithmetic against the first bucketed month rather
  // than a binary search -- months are contiguous by construction (buildDateSpan walks
  // every one between lo and hi), so this is exact and O(1).
  function monthIndexOfMs(ms) {
    var d = new Date(ms), m0 = new Date(dateSpan.months[0].ms);
    var idx = (d.getUTCFullYear() - m0.getUTCFullYear()) * 12 + (d.getUTCMonth() - m0.getUTCMonth());
    return Math.max(0, Math.min(dateSpan.months.length - 1, idx));
  }
  // The end of month `i`'s ms range -- the next month's start, or dateSpan.hi itself when
  // `i` is the last month (which is always populated: dateSpan.hi is derived FROM its last
  // note, so nothing is ever dated past it).
  function monthEndMs(i) {
    return (i + 1 < dateSpan.months.length) ? dateSpan.months[i + 1].ms : dateSpan.hi;
  }
  // Every segment is exactly one calendar month now (github#23's note-weighted axis gives
  // each month its own segment, weighted by content rather than grouped by emptiness), so
  // this only exists to keep ribbonXCompact/ribbonMsCompact from repeating `monthEndMs`.
  function segSpanMs(seg) {
    return [dateSpan.months[seg.i].ms, monthEndMs(seg.i)];
  }

  /**
   * ms -> x through dateSpan.axis's weight-space instead of raw time. Sub-month position
   * still interpolates by real elapsed time WITHIN the month (there's nothing else to go
   * by at that resolution), but which pixel-width slice that lands in is decided by
   * dateSpan.axis's note-weighted segments, not by the month's own calendar length
   * (github#23) -- a quiet month draws a thin slice and a busy one a wide one even when
   * both are full calendar months.
   */
  function ribbonXCompact(ms, w) {
    var ax = dateSpan.axis, seg = ax.segs[ax.segOfMonth[monthIndexOfMs(ms)]];
    var span = segSpanMs(seg), lo = span[0], hi = span[1];
    var frac = hi > lo ? Math.max(0, Math.min(1, (ms - lo) / (hi - lo))) : 0;
    var wPos = seg.w0 + frac * (seg.w1 - seg.w0);
    return (wPos / ax.totalW) * w;
  }
  function ribbonMsCompact(x, w) {
    var ax = dateSpan.axis, xc = Math.max(0, Math.min(w, x));
    var wPos = (xc / w) * ax.totalW, segs = ax.segs, seg = segs[segs.length - 1];
    for (var i = 0; i < segs.length; i++) {
      if (wPos <= segs[i].w1) { seg = segs[i]; break; }
    }
    var frac = seg.w1 > seg.w0 ? Math.max(0, Math.min(1, (wPos - seg.w0) / (seg.w1 - seg.w0))) : 0;
    var span = segSpanMs(seg);
    return span[0] + frac * (span[1] - span[0]);
  }

  function ribbonX(ms, w) {
    return (compactAxis && dateSpan.axis) ? ribbonXCompact(ms, w) : ribbonXLinear(ms, w);
  }
  function ribbonMs(x, w) {
    return (compactAxis && dateSpan.axis) ? ribbonMsCompact(x, w) : ribbonMsLinear(x, w);
  }

  /**
   * The brush's two ends, with null meaning "the end of the span".
   *
   * Reads the in-flight drag first, so the handles follow the cursor while the disc behind
   * them stays put. This is the one place the preview and the applied state are allowed to
   * disagree, and it is what makes a drag cost a canvas repaint instead of a relayout.
   */
  function brushEnds() {
    // A DRAG BEATS THE SWEEP. Both are previews and only one can be showing; if a hand is on
    // the handle while the intro is running, the hand is the one that means something -- and
    // the release goes through cascade(), which stops playback anyway. Getting this order
    // wrong would pin the handle under the cursor to wherever the animation had got to.
    if (brushDrag && brushDrag.pFrom !== undefined) return [brushDrag.pFrom, brushDrag.pTo];
    if (brushSweep !== null) return [dateSpan.lo, brushSweep];
    return [state.from === null ? dateSpan.lo : state.from,
            state.to === null ? dateSpan.hi : state.to];
  }

  /**
   * Where the band's window is.
   *
   * Reads state directly -- the window drag is LIVE and writes state on every frame, unlike
   * the brush, which previews. The two are not inconsistent: dragging the brush re-lays out
   * the whole disc, which is O(n) three times over and cannot keep up at ten thousand notes,
   * while dragging the window only re-buckets the band. Those are different budgets, so they
   * get different answers, and the one that can afford to be live is live.
   */
  function winEndNow() {
    if (state.heatEnd !== null) return state.heatEnd;
    return heat ? heat.start + heat.cols * WEEK_MS : heatParse(TODAY);
  }

  // One month's bar: height against nRef, dim when empty. Shared by both the linear and
  // compact bar-layout paths in drawRibbon so the fill/height math has exactly one copy.
  function paintMonthBar(cx, top, m, x, bw) {
    var t = Math.min(1, m.n / dateSpan.nRef);
    var bh = m.n ? Math.max(1.5, (top - 2) * t) : 0;
    cx.fillStyle = m.n ? dateRamp(t) : css("--dim");
    cx.fillRect(x, top - bh, bw, bh || 1);
  }

  function drawRibbon() {
    var cv = $("ribbon");
    if (!cv || !dateSpan) return;
    var w = Math.max(200, ribbonW());
    var cx = fitCanvas(cv, w, RIBBON_H);
    var top = RIBBON_BARS;                     // the bars live above this line
    var ms = dateSpan.months, n = ms.length;
    var useCompact = compactAxis && dateSpan.axis;

    if (useCompact) {
      // SEGMENT-SPACE LAYOUT, using the SAME note-weighted segments dateSpan.axis carries
      // for ribbonX/ribbonMs -- so the bars, the brush and the year chips agree about where
      // things are rather than being two formulas that happen to look similar. Every
      // segment is one calendar month, painted exactly as the linear layout paints it
      // (paintMonthBar doesn't know or care how its x/width were decided) -- the compaction
      // shows up as a quiet month drawing a thin bar and a busy one a wide one, and a
      // shrunk YEAR reading as a tight cluster of thin bars between two year rules close
      // together, not as a separate "collapsed" visual treatment (github#23).
      var segs = dateSpan.axis.segs, totalW = dateSpan.axis.totalW;
      for (var si = 0; si < segs.length; si++) {
        var seg = segs[si];
        var segX = (seg.w0 / totalW) * w;
        var segW = Math.max(1, ((seg.w1 - seg.w0) / totalW) * w - 0.6);
        paintMonthBar(cx, top, ms[seg.i], segX, segW);
      }
      // Year boundaries, in segment space -- every year gets its own rule regardless of how
      // compacted it is, since every month (including a note-less one in a shrunk year) is
      // still its own segment with its own x (github#23).
      for (var j = 0; j < n; j++) {
        if (ms[j].m !== 0) continue;
        var jSeg = segs[dateSpan.axis.segOfMonth[j]];
        cx.fillStyle = rgbaHex(css("--text-3"), 0.28);
        cx.fillRect((jSeg.w0 / totalW) * w, 0, 1, top);
      }
    } else {
      var pitch = w / n;
      for (var i = 0; i < n; i++) {
        paintMonthBar(cx, top, ms[i], i * pitch, Math.max(1, pitch - 0.6));
      }
      // Year boundaries. Only January gets a rule, so the strip reads as years rather than as
      // 121 months. The years themselves are named by the buttons below -- see buildYears.
      for (var j2 = 0; j2 < n; j2++) {
        if (ms[j2].m !== 0) continue;
        cx.fillStyle = rgbaHex(css("--text-3"), 0.28);
        cx.fillRect(j2 * pitch, 0, 1, top);
      }
    }

    // THE BAND'S WINDOW, on its own track and draggable. A rail the full width of the span
    // with a pill on it showing the 52 weeks the grid above is drawing -- which is also a
    // scrollbar, and reads as one, which is the point: it is the thing you move to look
    // somewhere else.
    var tw = winTrack(w);
    cx.fillStyle = rgbaHex(css("--text-3"), 0.16);
    heatRect(cx, 0, tw.y + tw.h / 2 - 1, w, 2, 1);
    cx.fill();
    // The pill in the bars' own hue, so the whole strip reads as one instrument, with a rim
    // in the band's background so its ends are legible against the rail behind them.
    var pillW = Math.max(10, tw.x1 - tw.x0);
    cx.fillStyle = scrubColor();
    cx.globalAlpha = brushDrag && brushDrag.mode === "win" ? 1 : 0.86;
    heatRect(cx, tw.x0, tw.y, pillW, tw.h, tw.h / 2);
    cx.fill();
    cx.globalAlpha = 1;
    cx.strokeStyle = rgbaHex(css("--surface-0"), 0.9);
    cx.lineWidth = 1;
    heatRect(cx, tw.x0, tw.y, pillW, tw.h, tw.h / 2);
    cx.stroke();

    // The brush: a wash over everything OUTSIDE it, two hard edges, and a grip on each.
    // 0.72 of the surface rather than 0.62 -- at the lower value the difference between
    // inside and outside was a few percent of luminance on a dark ground and the brush read
    // as two lines with nothing between them.
    //
    // DRAWN UNCONDITIONALLY, with the handles resting on the ends of the span when nothing
    // is filtered. They used to appear only once a range existed, which meant the control
    // opened as a bar chart with no visible way to operate it -- you had to guess that
    // dragging it did something. The wash needs no condition either: at rest the handles are
    // at the ends, so there is nothing outside them and both rectangles are zero-width.
    //
    // `from`/`to` stay NULL at rest rather than being set to the span's ends. Null means "no
    // bound", which is what lets timeFactor skip the comparison entirely and what keeps a
    // note dated outside the known span -- a bad stamp, a note edited later -- from being
    // excluded by a filter nobody asked for.
    var e = brushEnds(), x0 = ribbonX(e[0], w), x1 = ribbonX(e[1], w);
    cx.fillStyle = rgbaHex(css("--surface-0"), 0.72);
    cx.fillRect(0, 0, x0, top);
    cx.fillRect(x1, 0, w - x1, top);

    // The two scrubbers. Each is a full-height rule plus a grip, and each is outlined in the
    // band's own background -- which is what makes it legible standing on a bar of any height,
    // and the reason a plain accent-coloured handle was invisible on a busy month.
    var col = scrubColor(), rim = rgbaHex(css("--surface-0"), 0.92);
    var gw = 9, gh = Math.max(12, top - 8), gy = (top - gh) / 2;
    [x0, x1].forEach(function (x) {
      // The RULE sits exactly on the date; the GRIP is nudged to stay inside the canvas. At
      // rest the two handles are at the ends of the span, so a grip centred on the date has
      // half of itself outside the element -- which is the one moment it most needs to be
      // seen, since that is the state the control opens in.
      var gx = Math.max(0, Math.min(w - gw, x - gw / 2));
      cx.fillStyle = rim;
      cx.fillRect(x - 2.5, 0, 5, top);
      cx.fillStyle = col;
      cx.fillRect(x - 1.5, 0, 3, top);
      heatRect(cx, gx, gy, gw, gh, 3);
      cx.fill();
      cx.strokeStyle = rim;
      cx.lineWidth = 1;
      heatRect(cx, gx, gy, gw, gh, 3);
      cx.stroke();
      // Two notches, so the grip reads as something to take hold of rather than as a tab.
      cx.fillStyle = rim;
      cx.fillRect(gx + gw / 2 - 2, gy + gh / 2 - 3, 1, 6);
      cx.fillRect(gx + gw / 2 + 1, gy + gh / 2 - 3, 1, 6);
    });
  }

  /**
   * Repaint the band for the current window, doing as little as the change allows.
   *
   * THE GRID IS COLUMNED BY WEEK. heatMonday() quantises the window's end, so a drag across a
   * few pixels inside one week produces a byte-identical grid -- and heatBuild() re-buckets
   * every note in the vault to produce it, ten thousand of them on the fixture. So the week is
   * computed first and the rebuild is skipped when it has not moved.
   *
   * The ribbon is redrawn either way, which is what makes the pill glide while the grid steps.
   * That difference is not a compromise, it is the truth about the two things: the pill is a
   * continuous position and the band is a row of weeks.
   */
  function rebuildBand() {
    var endMs = state.heatEnd === null ? heatParse(TODAY) : state.heatEnd;
    var wantStart = heatMonday(endMs) - ((heat ? heat.cols : HEAT_WEEKS) - 1) * WEEK_MS;
    var moved = !heat || heat.start !== wantStart;
    if (moved) heatBuild();
    drawDateUI();
    if (moved) heatDraw();
  }

  /** The window pill's box on the track, in canvas pixels. Follows a drag if one is live. */
  function winTrack(w) {
    var span = (heat ? heat.cols : HEAT_WEEKS) * WEEK_MS;
    var end = winEndNow();
    return { x0: ribbonX(end - span, w), x1: ribbonX(end, w),
             y: RIBBON_BARS + 2, h: RIBBON_TRACK - 5 };
  }

  /** True when a press at this height is aiming at the window track rather than the bars. */
  function inWinTrack(y) { return y >= RIBBON_BARS && y < RIBBON_BARS + RIBBON_TRACK; }

  /**
   * Move the band's window so it ENDS at `ms`, clamped to today and to the span.
   *
   * Ends rather than starts, because the window is 52 weeks looking backwards -- that is what
   * it has always been, and a window that started where you pointed would show the year after
   * the date you picked rather than the year up to it.
   */
  /** The window's span, in ms. */
  function winSpan() { return (heat ? heat.cols : HEAT_WEEKS) * WEEK_MS; }

  function clampWinEnd(ms) {
    var todayMs = heatParse(TODAY);
    var lo = dateSpan.lo + winSpan();         // never scroll off the left end of the history
    return Math.max(Math.min(ms, todayMs), Math.min(lo, todayMs));
  }

  /**
   * The window end that puts the POINTER'S PIXEL, not its date, in the middle of the pill.
   *
   * Grabbing it used to set the window's END to the pointer, so the pill jumped to sit
   * entirely to the left of the hand and the thing being dragged was somewhere else. Centring
   * is what a scrollbar thumb does when you click the trough, and it means the date under the
   * cursor is the middle of what the grid is showing -- which is the date you were pointing at.
   *
   * TAKES A PIXEL, NOT A DATE, and solves for `end` by bisection rather than adding half the
   * window's span in ms -- that shortcut only works when ribbonX is linear, because it
   * assumes a constant px-per-ms ratio to convert "half the span in time" into "half the
   * pill's width on screen". compactAxis breaks that assumption on purpose: the same 52-week
   * span can be many pixels wide sitting in a dense year and few sitting in a sparse one, so
   * a window whose end tracked the pointer by TIME visibly resized as it crossed that
   * boundary and stopped tracking the pointer's actual pixel at all (github#23). Solving in
   * pixel space instead asks the one question that is actually true regardless of the axis:
   * where does `end` have to be for the drawn pill's midpoint to BE this pixel. Monotonic in
   * `end` (ribbonX only ever increases with ms), so bisection converges in a fixed handful of
   * steps and needs no derivative. On a linear axis this converges to the exact same answer
   * the old formula gave -- centring by pixel and centring by time are the same statement
   * there, so nothing about the non-compact behaviour changes.
   */
  function winEndCentredAtPx(px, w) {
    var span = winSpan(), todayMs = heatParse(TODAY);
    var lo = Math.min(dateSpan.lo + span, todayMs), hi = todayMs;
    if (lo >= hi) return clampWinEnd(hi);
    for (var i = 0; i < 24; i++) {
      var mid = (lo + hi) / 2;
      var midPx = (ribbonX(mid - span, w) + ribbonX(mid, w)) / 2;
      if (midPx < px) lo = mid; else hi = mid;
    }
    return clampWinEnd((lo + hi) / 2);
  }

  /**
   * What a press at `x` grabs: an existing edge, the span between them, or empty strip.
   *
   * THE BUG THIS EXISTS TO FIX: every press used to start a NEW brush anchored where the
   * pointer went down, with the other end following the pointer. So grabbing the left edge
   * and dragging moved the right edge as well -- reported as "when I move the left slider
   * the right one moves too", and it was not a slider at all, it was a fresh selection every
   * time. An edge has to be a thing you can take hold of.
   */
  function brushHit(x, w, y) {
    if (y !== undefined && inWinTrack(y)) return "win";
    var e = brushEnds(), x0 = ribbonX(e[0], w), x1 = ribbonX(e[1], w);
    // The nearer edge wins when both are in reach, or a narrow brush has one grabbable edge.
    var d0 = Math.abs(x - x0), d1 = Math.abs(x - x1);
    if (d0 <= GRAB_PX || d1 <= GRAB_PX) return d0 <= d1 ? "from" : "to";
    // A press INSIDE the brush pans it -- unless the brush is the whole span, where panning
    // is a clamped no-op and what the press obviously means is "select from about here".
    // Without that exception the resting control had no way to start a range except by
    // finding one of the two handles at the very ends of the strip.
    if (x > x0 && x < x1) {
      return (state.from === null && state.to === null) ? "new" : "body";
    }
    return "new";
  }

  /**
   * The date under the handle, floated above the strip.
   *
   * Positioned against #vg-heat, which is the band's own positioning context -- the same one
   * #vg-htip uses. Clamped to the band so a handle at either extreme does not push its own
   * label off the edge, which is the failure every tooltip in this page has had once.
   */
  function showRTip(x, text) {
    var t = $("rtip"), rib = $("ribbon"), band = $("heat");
    if (!t || !rib || !band) return;
    setHTML(t, esc(text));
    t.hidden = false;
    var bb = band.getBoundingClientRect(), rb = rib.getBoundingClientRect();
    var tb = t.getBoundingClientRect();
    var left = Math.max(4, Math.min(bb.width - tb.width - 4, (rb.left - bb.left) + x - tb.width / 2));
    t.style.left = left + "px";
    t.style.top = ((rb.top - bb.top) - tb.height - 3) + "px";
  }
  function hideRTip() { var t = $("rtip"); if (t) t.hidden = true; }

  /** A range end as a plain ISO day. */
  function isoDay(ms) { return new Date(ms).toISOString().slice(0, 10); }

  /** What the grid above is currently drawing, for the track's tooltip. */
  function winLabel() {
    if (!heat) return "";
    return isoDay(heat.start) + "  \u2192  " + isoDay(heat.start + heat.cols * WEEK_MS - DAY_MS);
  }

  function buildDateUI() {
    var rib = $("ribbon");
    if (!rib) return;

    // ONE UPDATE PER FRAME -- see makeFrameCoalescer, also used by bindNodeDrag's hub-pin
    // drag. A pointermove fires 120+ times a second on a decent mouse and each update
    // re-lays out the disc; without this the handler is the bottleneck and the drag lags
    // behind the cursor. The last position is the only one that matters.
    var onFrame = makeFrameCoalescer();

    $("rangeall").onclick = function () {
      state.from = null; state.to = null; state.heatEnd = null;
      heatBuild();
      applyRange();
      heatDraw();
    };

    // THE TWO DATE FIELDS. `change` and not `input`: a picker fires input on every keystroke
    // while a date is half-typed, and "2" parses as the year 2 -- which would relayout the
    // disc for a range nobody asked for, twice per digit.
    var fieldMs = function (el) {
      var v = el && el.value;
      if (!v) return null;
      var t = heatParse(v);
      return isFinite(t) ? t : null;
    };
    ["from", "to"].forEach(function (which) {
      var el = $(which);
      if (!el) return;
      el.onchange = function () {
        // An emptied field means "no bound on this end", which is exactly what the span's own
        // end means to setRangeMs.
        setRangeMs(fieldMs($("from")), fieldMs($("to")));
      };
    });

    var xOf = function (ev) { return ev.clientX - rib.getBoundingClientRect().left; };
    var yOf = function (ev) { return ev.clientY - rib.getBoundingClientRect().top; };

    rib.addEventListener("pointerdown", function (ev) {
      if (!dateSpan) return;

      var w = ribbonW(), x = xOf(ev), mode = brushHit(x, w, yOf(ev)), e = brushEnds();
      brushDrag = { mode: mode, x0: ev.clientX, moved: false,
                    // For an edge drag the OTHER end is the anchor and does not move. For a
                    // body drag both ends move together, so both originals are kept.
                    anchor: mode === "from" ? e[1] : e[0],
                    from0: e[0], to0: e[1], grab: ribbonMs(x, w),
                    winEnd0: heat ? heat.start + heat.cols * WEEK_MS : 0 };
      try { rib.setPointerCapture(ev.pointerId); } catch { /* fine */ }
      rib.setAttribute("data-grab", mode === "win" ? "moving"
                                  : mode === "body" ? "moving" : "edge");
      // A PRESS ON THE TRACK IS ALSO A JUMP. Dragging the pill across eleven years to reach
      // 2018 is a lot of mouse; pressing at 2018 puts it there and the drag then refines it.
      if (mode === "win") {
        state.heatEnd = winEndCentredAtPx(x, w);
        rebuildBand();
        showRTip(x, winLabel());
      }
    });

    rib.addEventListener("pointermove", function (ev) {
      if (!brushDrag) {
        // Cursor as affordance: the edges are 2px of paint and nothing else says they can be
        // taken hold of.
        if (dateSpan) {
          var x = xOf(ev), w2 = ribbonW(), m = brushHit(x, w2, yOf(ev));
          if (m === "win") rib.setAttribute("data-grab", "body");
          else if (m === "from" || m === "to") rib.setAttribute("data-grab", "edge");
          else if (m === "body") rib.setAttribute("data-grab", "body");
          else rib.removeAttribute("data-grab");
          // The date under the pointer, hovering as well as dragging. This is a date axis
          // eleven years wide in 1268px -- a month is nine pixels, and reading one off the
          // year ticks alone is guesswork. On the track it names the window instead, because
          // there the thing being moved is a 52-week span rather than a day.
          showRTip(x, m === "win" ? winLabel() : isoDay(ribbonMs(x, w2)));
        }
        return;
      }
      if (Math.abs(ev.clientX - brushDrag.x0) > DRAG_MIN) brushDrag.moved = true;
      if (!brushDrag.moved) return;

      var w = ribbonW(), here = ribbonMs(xOf(ev), w), lo, hi, follow;

      // The band's window, moved on its own and touching neither end of the brush. LIVE:
      // rebuildBand() only does the expensive half when the window crosses a week boundary,
      // so this costs one canvas per frame and one re-bucket per week travelled.
      if (brushDrag.mode === "win") {
        var wx = xOf(ev);
        onFrame(function () {
          if (!brushDrag) return;
          state.heatEnd = winEndCentredAtPx(wx, w);
          rebuildBand();
          showRTip(wx, winLabel());
        });
        return;
      }

      if (brushDrag.mode === "body") {
        // Pan the whole range, clamped so it keeps its width at either end of the span.
        var d = here - brushDrag.grab;
        var width = brushDrag.to0 - brushDrag.from0;
        lo = Math.max(dateSpan.lo, Math.min(dateSpan.hi - width, brushDrag.from0 + d));
        hi = lo + width;
        follow = hi;
      } else if (brushDrag.mode === "from" || brushDrag.mode === "to") {
        // One end moves, the anchor does not. Crossing over is allowed and simply swaps
        // which end is which, so a drag past the far edge does not stick.
        lo = Math.min(brushDrag.anchor, here);
        hi = Math.max(brushDrag.anchor, here);
        follow = here;                    // the band follows the end being dragged
      } else {
        lo = Math.min(brushDrag.grab, here);
        hi = Math.max(brushDrag.grab, here);
        follow = here;
      }
      // THE BAND'S WINDOW IS NOT TOUCHED HERE. It used to follow whichever end was moving,
      // which is convenient until you want to read one stretch while filtering to another --
      // and it made the window impossible to place deliberately, since the next brush nudge
      // took it back. It has its own track now.
      var mode = brushDrag.mode;
      brushDrag.pFrom = lo;
      brushDrag.pTo = hi;
      onFrame(function () {
        if (!brushDrag) return;
        // Both ends labelled while panning, since both are moving; otherwise just the one
        // under the hand. A label naming the end you are not touching is noise.
        showRTip(ribbonX(follow, w),
                 mode === "body" ? isoDay(lo) + "  \u2192  " + isoDay(hi) : isoDay(follow));
        drawDateUI();               // the handles only; the disc waits for release
        var el = $("rangenote");
        if (el) el.textContent = isoDay(lo) + "  \u2192  " + isoDay(hi);
      });
    });

    // THE ONE MOMENT THE FILTER CHANGES. Everything the drag did was a preview on a canvas;
    // this is where it becomes state, and where the disc animates to it exactly as it does
    // for a legend toggle.
    var endDrag = function (ev) {
      if (!brushDrag) return;
      var d = brushDrag;
      brushDrag = null;
      rib.removeAttribute("data-grab");
      hideRTip();
      try { rib.releasePointerCapture(ev.pointerId); } catch { /* fine */ }

      // Nothing to apply for a window drag: it has been applying itself all along.
      if (d.mode === "win") return;
      if (d.moved && d.pFrom !== undefined) {
        // The ends of the span mean "no bound" rather than "the first and last note", so a
        // drag that lands on an end clears that half of the filter. Without this, brushing
        // the whole strip left a range that excluded anything dated outside the known span.
        state.from = d.pFrom <= dateSpan.lo ? null : d.pFrom;
        state.to = d.pTo >= dateSpan.hi ? null : d.pTo;
        applyRange();
      } else {
        rangeChrome();              // an abandoned drag: put the readout back
      }
    };
    rib.addEventListener("pointerup", endDrag);
    rib.addEventListener("pointercancel", endDrag);

    rib.addEventListener("pointerleave", function () { if (!brushDrag) hideRTip(); });

    // THE YEAR BUTTONS. Delegated, because buildYears replaces them whenever the band is laid
    // out -- binding per button would leak a listener per relayout and miss the new ones.
    //
    // HOVER HALOES THE YEAR'S NOTES, the same way hovering a legend row haloes a folder's: the
    // strip says how MANY notes a year holds, and the disc is where they are. Set through the
    // same state a marked day uses, so it ramps and clears with every other halo rather than
    // being a second highlight mechanism.
    var yrHost = $("years");
    var hoverYear = function (yr) {
      if (state.hoverYear === yr) return;
      state.hoverYear = yr;
      if (renderer) renderer.refresh();
    };
    if (yrHost) {
      var yrOf = function (ev) {
        var b = ev.target && ev.target.closest && ev.target.closest("button[data-yr]");
        return b ? b.getAttribute("data-yr") : null;
      };
      yrHost.addEventListener("click", function (ev) {
        var yr = yrOf(ev);
        if (yr === null) return;
        state.hoverYear = null;
        setRangeMs(Date.UTC(+yr, 0, 1), Date.UTC(+yr, 11, 31));
      });
      // pointerover/out rather than enter/leave: these delegate, and the button is a child.
      yrHost.addEventListener("pointerover", function (ev) { hoverYear(yrOf(ev)); });
      yrHost.addEventListener("pointerout", function (ev) {
        if (!ev.relatedTarget || !yrHost.contains(ev.relatedTarget)) hoverYear(null);
      });
    }

    // THE STRIP RESCALES WITH THE BAND. The observer was already here and did nothing,
    // because the width it redrew at came from the canvas rather than from the slot -- see
    // measureRibbon. With that fixed it needs one more thing: a guard, so an observation
    // that changes nothing costs nothing. The band fires this for its own reasons (the grid
    // rebuilding changes the wrapper's scroll width) and a redraw per observation would be a
    // canvas repaint and a dozen DOM writes for no change.
    var onSlot = function () {
      var w = measureRibbon();
      if (w && Math.abs(w - ribW) < 0.5) return;
      ribW = w;
      drawDateUI();
    };
    // A WINDOW FALLBACK, which was missing outright: without ResizeObserver the strip was
    // drawn once at boot and never again. It covers less -- a pane resized inside a window
    // that did not move is invisible to it -- but "less" beats "not at all".
    if (WIN.ResizeObserver) new WIN.ResizeObserver(onSlot).observe($("heat"));
    else WIN.addEventListener("resize", onSlot);
    applyRange();
  }

  // ?rest -- COME UP AT REST, with no intro. The intro is TIMELINE_MS * TIME_SCALE, 5.6
  // seconds, and it is the single largest cost in an automated run: every page a measurement
  // opens pays it before the first check, and the suite opens one per lane per vault.
  //
  // Same door ?demo already used for the same reason -- timelineFrame(true) is the resting
  // full disc, derived once with no animation -- so this is that branch given its own name
  // rather than a second way of doing it.
  /** --dev build, or ?wedges on the URL; ?nowedges wins over both. */
  function wantWedgeDebug() {
    var q = String(WIN.location ? WIN.location.search : "") + " " +
            String(WIN.location ? WIN.location.hash : "");
    if (/(^|[?&#])nowedges\b/.test(q)) return false;
    if (/(^|[?&#])wedges\b/.test(q)) return true;
    return !!(DATA && DATA.dev);
  }

  function restOn() {
    return /(^|[?&#])rest\b/.test(String(location.search) + " " + String(location.hash));
  }

  /**
   * `?rowarc` -- share each ROW's circle among the wedges that are actually in that row.
   *
   * A wedge's arc is decided once for the whole band, and a wedge reaches only as many rows as
   * it has notes to fill. 07 - Yearly Reviews holds one note, so it exists in exactly one row
   * -- and in the two rows it does not reach, its 10 degrees of arc simply sits empty. Measured
   * on the reporter's vault, that is a 108-unit hole in one row and a 178-unit hole in the next,
   * against a seam of 12 units everywhere else: the "gap on one side of yearly is larger than
   * the other" is yearly's own arc lying empty beside it, not yearly being off centre.
   *
   * So each row divides its circle among the cells present IN THAT ROW, weighted by the same
   * band shares, and pays one seam for each of them. Nothing else moves: a note keeps its seat
   * fraction inside its own wedge, its margins, its row and its size. This is the arc
   * redistribution on its own, without the note re-spacing that came with it last time and
   * looked, correctly, horrible.
   */
  function rowArcOn() {
    return /(^|[?&#])rowarc/.test(String(location.search) + " " + String(location.hash));
  }

  function demoOn() {
    return /(^|[?&#])demo\b/.test(String(location.search) + " " + String(location.hash));
  }

  /* ---- BEGIN: demo automation + debug API -- stripped from the plugin build, see scripts/build-plugin.mjs (stripDemoAndDebug) ---- */

  // Is anything moving? Every animation in this page owns one of these three, so this
  // is the whole answer rather than a sample of it. The driver polls this instead of
  // sleeping: a fixed wait fires part-way through on a page too slow to finish in time,
  // and the demo would then act on a disc that is still moving -- which is precisely
  // what the layout bugs in this project's history look like.
  function demoBusy() {
    return !!(play || cascadeRun || anim || hoverRaf || hlRaf);
  }

  /* --- resolving a target ------------------------------------------------ */

  // Groups are matched by PREFIX -- the storyboard says "05" and the vault says
  // "05 - Weekly Reviews" -- with two escapes so the demo runs on a vault that shares
  // none of those names:
  //
  //   "#2"   the second-largest group, by note count
  //   fallback: an unmatched prefix resolves to the largest group rather than nothing,
  //            so a beat degrades to "some folder" instead of being skipped
  //
  // That matters for a tool other people run: a storyboard that names this vault's
  // folders would silently skip half its beats on anyone else's.
  function demoGroup(spec) {
    var names = order[state.dim] || [];
    if (!names.length) return null;
    var bySize = names.slice().sort(function (a, b) { return (counts[b] || 0) - (counts[a] || 0); });
    if (/^#\d+$/.test(spec)) return bySize[parseInt(spec.slice(1), 10) - 1] || null;
    for (var i = 0; i < names.length; i++) {
      if (names[i].indexOf(spec) === 0) return names[i];
    }
    return bySize[0];
  }

  // A beat's target -> the actual element. Looked up by the attribute the handler reads,
  // never by position, so re-ordering the legend cannot silently aim a beat at a
  // different folder.
  function demoFind(kind, arg) {
    if (kind === "id") return $(arg);
    // A point on the STAGE, for the camera beats. "centre", or a fraction pair like "0.3,0.4"
    // measured from the stage's top-left, so a pan can start somewhere with disc under it.
    if (kind === "stage") {
      var stageEl = $("graph");
      if (!stageEl) return null;
      var sb = stageEl.getBoundingClientRect();
      var f = (arg === "centre" || !arg) ? [0.5, 0.5] : String(arg).split(",").map(Number);
      return demoPoint(sb.left + sb.width * f[0], sb.top + sb.height * f[1],
                       2, 2, "stage " + (arg || "centre"));
    }
    // A BRUSH HANDLE on the date ribbon, "from" or "to", or the window pill, "window".
    // Resolved from the same geometry the control draws with, so a beat aims where the
    // handle is rather than where the storyboard guessed it would be.
    if (kind === "brush") return demoRibbonPoint(arg);
    if (kind === "eye" || kind === "group") {
      var g = demoGroup(arg);
      if (!g) return null;
      var attr = kind === "eye" ? "data-eye" : "data-g";
      var all = $("legend").querySelectorAll("[" + attr + "]");
      for (var i = 0; i < all.length; i++) {
        if (all[i].getAttribute(attr) === g) return all[i];
      }
      return null;
    }
    if (kind === "sub") {                       // "03/People" -- a subfolder's label
      var slash = arg.indexOf("/");
      var g2 = demoGroup(arg.slice(0, slash)), nm = arg.slice(slash + 1);
      if (!g2) return null;
      // Rows carry the group and the tint-slot INDICES they stand for, not the name, so
      // the name is resolved through subOrder the same way the row itself was built.
      var subs2 = subOrder[g2] || [];
      var k2 = subs2.indexOf(nm);
      // Same reasoning as demoGroup: an unknown subfolder name becomes the group's
      // biggest one rather than a skipped beat. subOrder is size-ordered already.
      if (k2 < 0) k2 = subs2.length ? 0 : -1;
      if (k2 < 0) return null;
      var rows = $("legend").querySelectorAll("[data-hsub]");
      for (var j = 0; j < rows.length; j++) {
        if (rows[j].getAttribute("data-hsub") !== g2) continue;
        var idx = rows[j].getAttribute("data-idx").split(",");
        if (idx.indexOf(String(k2)) >= 0) return rows[j];
      }
      return null;
    }
    if (kind === "twisty") {                    // a group's disclosure triangle
      var gt = demoGroup(arg);
      if (!gt) return null;
      var tws = $("legend").querySelectorAll("[data-tw]");
      for (var t = 0; t < tws.length; t++) {
        if (tws[t].getAttribute("data-tw") === gt) return tws[t];
      }
      return null;                              // no subfolders, so no triangle
    }
    if (kind === "only") {                      // the `only` chip on a group's row
      var row = demoFind("group", arg);
      return row ? row.querySelector(".only") : null;
    }
    if (kind === "swatch") {                    // "01/g8" -- one slot in a folder's row
      var cut = arg.indexOf("/");
      var gsw = demoGroup(arg.slice(0, cut)), key = arg.slice(cut + 1);
      if (!gsw) return null;
      // Compared attribute by attribute rather than built into a selector, like every
      // other resolver here: a folder name goes into `data-fc` verbatim, and quoting one
      // into an attribute selector is an escaping problem this does not need to have.
      var sw = $("setbody").querySelectorAll(".swatch");
      for (var s = 0; s < sw.length; s++) {
        if (sw[s].getAttribute("data-fc") === gsw &&
            sw[s].getAttribute("data-key") === key) return sw[s];
      }
      return null;                              // the panel is closed, so nothing to aim at
    }
    // The right-click colour menu's own swatches -- "g8", or "" for its Auto button.
    // Unlike the settings panel's "swatch" kind above, only one folder or subfolder can
    // have this menu open at a time, so there is no folder half of the key to match.
    if (kind === "ctxswatch") {
      var cm = $("ctxmenu");
      if (!cm || cm.hidden) return null;
      var csw = cm.querySelectorAll("[data-key]");
      for (var cs = 0; cs < csw.length; cs++) {
        if (csw[cs].getAttribute("data-key") === (arg || "")) return csw[cs];
      }
      return null;                              // the menu is closed, so nothing to aim at
    }
    // A YEAR CHIP under the strip. "busiest" picks the fullest year that HAS a chip, which
    // is not the same as the fullest year: below about 20px of pitch buildYears names every
    // other year, so the busiest one may have no button to aim at. Choosing among the chips
    // that exist is also what keeps this working at any width -- and it can only ever land
    // on a year with notes, which the hover half of a year beat needs (a chip for an empty
    // year haloes nothing, and a beat that lights nothing reads as a miss on camera).
    if (kind === "year") {
      var yh = $("years");
      if (!yh || !dateSpan) return null;
      var chips = [].slice.call(yh.querySelectorAll("button[data-yr]"));
      if (!chips.length) return null;
      var have = Object.create(null);
      dateSpan.years.forEach(function (yy) { have[String(yy.y)] = yy.n; });
      var pickY = null, bestN = -1;
      chips.forEach(function (c) {
        var y = c.getAttribute("data-yr");
        if (arg && /^\d{4}$/.test(arg)) { if (y === arg) pickY = c; return; }
        var n = have[y] || 0;
        if (n > bestN) { bestN = n; pickY = c; }
      });
      if (!pickY || (!arg && bestN <= 0)) return null;
      pickY.demoLabel = "year " + pickY.getAttribute("data-yr") +
                        " (" + (have[pickY.getAttribute("data-yr")] || 0) + " notes)";
      return pickY;
    }
    if (kind === "note") return demoNoteRect(arg);
    if (kind === "biginner") return demoBigInnerNote();
    // The detail card's hub toggle. Resolvable only once a note is selected and the card
    // is open -- which the pin storyboard already orders itself around (click the note,
    // then this), the same way the folder acts order themselves around a twisty.
    if (kind === "pin") {
      var dcard = $("detail");
      return dcard && !dcard.hidden ? dcard.querySelector(".pin") : null;
    }
    // The detail card's own close button. A card left open after a beat is done with
    // it is not a resting state anything else in the storyboard expects -- every other
    // act starts from "nothing selected", and the pin act is the one act that opens a
    // card without a hover or a click-elsewhere already lined up to close it again.
    if (kind === "detailclose") {
      var dc2 = $("detail");
      return dc2 && !dc2.hidden ? dc2.querySelector(".x") : null;
    }
    if (kind === "day") return demoCellRect(heat && heat.days[arg]);
    if (kind === "busiest") {
      // Ranked by what is VISIBLE right now, not by membership -- `n` is the sum of the
      // day's alphas. So this never picks a cell that looks empty because the folder it
      // belongs to is hidden, which matters since the storyboard hides one first.
      if (!heat) return null;
      var ds = [];
      for (var kk in heat.days) if (heat.days[kk].n > 0.004) ds.push(heat.days[kk]);
      ds.sort(function (a, b) { return b.n - a.n; });
      return demoCellRect(ds[Math.max(1, parseInt(arg, 10) || 1) - 1]);
    }
    return null;
  }

  // A note in a group, picked as THE MOST ISOLATED one on screen -- the visible note of
  // that group whose nearest visible neighbour, in pixels, is furthest away.
  //
  // Not the best-connected one, which would be the obvious choice for showing edges light
  // up. Two reasons, and the second is the important one.
  //
  // The driver's input hit-tests for real, so aiming at a dot in a crowd means whichever
  // dot actually owns that pixel gets hovered -- and hubs are packed into the middle of
  // the disc where the rows are tightest. The best-connected note is therefore the least
  // reliable thing to aim at.
  //
  // And a miss is not merely a wrong note: hovering NAMES the note on camera. Land one dot
  // over and the label on screen belongs to whatever else was there, which on this vault
  // could be a person's note. So the aim has to be provably unambiguous, not just
  // probably right.
  //
  // THE BOUND. Sigma picks the node whose drawn disc contains the pointer, so the aim is
  // unambiguous exactly when no OTHER node's disc can reach it: the nearest neighbour must
  // be further than this note's radius plus the largest radius on screen. A first version
  // required only twice this note's own size and it was measured to fail -- aiming at the
  // daily note 2026-06-20 hovered 2026-W27 in a neighbouring wedge instead, because the
  // neighbour was a bigger dot than the margin allowed for. Returns null rather than a
  // risky target, and the beat is skipped and logged.
  function demoNoteRect(prefix) {
    var g = demoGroup(prefix);
    if (!g || !renderer) return null;
    // SIGMA'S VIEWPORT IS ITS CONTAINER, NOT THE PAGE. graphToViewport returns
    // coordinates relative to the #graph element, and this vault's disc sits at
    // (288, 155) in the page -- the sidebar's width and the heatmap band's height. The
    // driver dispatches input in PAGE coordinates, so every position needs that origin
    // added or the aim lands a sidebar and a band away from the note.
    //
    // Measured: node 158 (2026-06-20) reports (888, 607) from graphToViewport, and
    // pointing there hovered 2026-W27 instead -- a different note that happens to live
    // where those numbers land on the page. #tip gets away with the raw numbers because
    // it is positioned inside #canvas, whose box is exactly the sigma container's.
    var org = $("graph").getBoundingClientRect();
    var pts = [], maxR = 0;
    graph.forEachNode(function (id, a) {
      if ((alpha[id] || 0) < 0.5) return;                 // not on screen right now
      var v = renderer.graphToViewport({ x: a.x, y: a.y });
      v = { x: v.x + org.left, y: v.y + org.top };
      // THROUGH scaleSize, because everything else in this loop is in VIEWPORT pixels and
      // dotPx is in the units sigma divides by the camera ratio. They agree to within 8% at
      // rest and are a factor of 20 apart zoomed in, which would put every label collision
      // radius wrong by that much.
      var r = renderer.scaleSize ? renderer.scaleSize(dotPx(a.size, id)) : dotPx(a.size, id);
      if (r > maxR) maxR = r;
      pts.push({ id: id, x: v.x, y: v.y, r: r, mine: a.folder === g, label: a.label });
    });
    var best = null, bestGap = -1;
    for (var i = 0; i < pts.length; i++) {
      if (!pts[i].mine) continue;
      var gap = Infinity;
      for (var j = 0; j < pts.length; j++) {
        if (i === j) continue;
        var dx = pts[i].x - pts[j].x, dy = pts[i].y - pts[j].y;
        var d2 = dx * dx + dy * dy;
        if (d2 < gap) gap = d2;
      }
      if (gap > bestGap) { bestGap = gap; best = pts[i]; }
    }
    if (!best) return null;
    // NO ISOLATION THRESHOLD. Two were tried and both were unmeetable on a dense disc:
    // "gap > own radius + largest radius on screen" found nothing at 3000 notes, and
    // "gap > own radius + 4px" found nothing at 10000 -- the beat silently skipped and
    // the suite reported "no safely isolated note to aim at" instead of testing anything.
    //
    // The threshold was trying to PROVE the hit-test outcome from geometry. It does not
    // need to: every target carries `expect`, and the driver compares it against what
    // actually got hovered and warns on a miss. Verifying the outcome is both stronger
    // than a bound and available at any density, so the most isolated candidate is
    // returned unconditionally and the check asserts where the pointer landed.
    //
    // `bestGap` is still reported, so how much clearance the aim actually had is
    // measurable rather than assumed.
    var box = Math.max(6, best.r * 1.5);
    return {
      left: best.x - box / 2, top: best.y - box / 2, width: box, height: box,
      // `expect` lets the driver confirm afterwards that the hover landed where it aimed.
      // The title is logged because it is on camera anyway -- and the storyboard aims only
      // at date-titled folders, which is what makes that safe.
      expect: best.id,
      gap: Math.round(Math.sqrt(bestGap) * 10) / 10,
      demoLabel: "note " + best.label
    };
  }

  /**
   * The biggest note on the INNER ring, for a beat that drags a note somewhere rather
   * than hovering it in place.
   *
   * demoNoteRect optimises for the opposite property -- the MOST ISOLATED note in a
   * folder, because a hover's whole job is to be an unambiguous, unmistakeable label.
   * A drag target wants something else: it has to survive a PRESS, a hundred-plus
   * pixels of glide, and a release all landing correctly, under whatever load the rest
   * of the machine is carrying (ffmpeg's capture and the --cursor helper both compete
   * with the renderer for the same CPU during a real recording) -- and a small dot is a
   * small hit-test box the whole way. Biggest is also nearest: the inner ring is where
   * the best-connected notes already sit by construction, so this is also the shortest
   * drag to the hub at its centre.
   *
   * INNER, not "biggest anywhere" -- outer-ring notes can be just as big on a
   * dominant-folder vault, and a long drag across the seam between bands has more
   * distance for the render backlog in bindNodeDrag's own mousemovebody handler to fall
   * behind on. geomLock.bandR is graph-normalised the same way inHubHole reads it (see
   * there): divide the raw hypot by UNIT before comparing.
   */
  function demoBigInnerNote() {
    if (!renderer || !geomLock || !geomLock.bandR) return null;
    var org = $("graph").getBoundingClientRect();
    var lo = geomLock.bandR.i[0], hi = geomLock.bandR.i[1];
    var pts = [];
    graph.forEachNode(function (id, a) {
      if ((alpha[id] || 0) < 0.5) return;                 // not on screen right now
      var rNorm = Math.hypot(a.x, a.y) / UNIT;
      if (rNorm < lo || rNorm > hi) return;                // outer ring, or off the lattice
      var v = renderer.graphToViewport({ x: a.x, y: a.y });
      v = { x: v.x + org.left, y: v.y + org.top };
      var rad = renderer.scaleSize ? renderer.scaleSize(dotPx(a.size, id)) : dotPx(a.size, id);
      pts.push({ id: id, x: v.x, y: v.y, r: rad, size: a.size || 0, label: a.label });
    });
    if (!pts.length) return null;
    // BIGGEST FIRST, THEN MOST ISOLATED among a handful of the biggest -- ranking on size
    // alone can still hand back one of two big dots sitting side by side, which is the
    // same silent-miss risk demoNoteRect's own comment describes for hover.
    pts.sort(function (a, b) { return b.size - a.size; });
    var top = pts.slice(0, Math.min(5, pts.length));
    var best = null, bestGap = -1;
    top.forEach(function (p) {
      var gap = Infinity;
      for (var j = 0; j < pts.length; j++) {
        if (pts[j].id === p.id) continue;
        var dx = p.x - pts[j].x, dy = p.y - pts[j].y;
        var d2 = dx * dx + dy * dy;
        if (d2 < gap) gap = d2;
      }
      if (gap > bestGap) { bestGap = gap; best = p; }
    });
    var box = Math.max(6, best.r * 1.5);
    return {
      left: best.x - box / 2, top: best.y - box / 2, width: box, height: box,
      expect: best.id,
      gap: Math.round(Math.sqrt(bestGap) * 10) / 10,
      demoLabel: "note " + best.label
    };
  }

  // Heatmap cells are PAINTED, not DOM, so there is no element to hand back -- a
  // synthetic rect in viewport coordinates is what the driver needs and all it needs.
  function demoCellRect(d) {
    if (!d || !heat) return null;
    var b = $("heatc").getBoundingClientRect();
    return {
      left: b.left + HEAT_GUTTER + d.col * heat.pitch,
      top:  b.top + HEAT_MONTH_H + d.row * heat.pitch,
      width: heat.cell, height: heat.cell,
      demoLabel: "heatmap " + d.key + " (" + Math.round(d.n) + " notes)"
    };
  }

  // Viewport coordinates for the driver, or null if the target is not there. Scrolled
  // into view first, because CDP input hit-tests for real -- a control below the fold
  // is a control that cannot be clicked, and reporting its off-screen coordinates would
  // make the driver click whatever is at that spot instead.
  /**
   * A synthetic target: something with a bounding box that is not an element.
   *
   * demoWhere reads getBoundingClientRect off whatever demoFind returns, so anything that can
   * answer that question can be a target. The camera and the ribbon handles are positions
   * rather than elements, and inventing a DOM node for each would be a lot of DOM for a
   * recording.
   */
  function demoPoint(cx, cy, w, h, label) {
    return {
      getBoundingClientRect: function () {
        return { left: cx - w / 2, top: cy - h / 2, width: w, height: h };
      },
      demoLabel: label
    };
  }

  /** Where a ribbon handle is, in page coordinates. */
  function demoRibbonPoint(which) {
    var rib = $("ribbon");
    if (!rib || !dateSpan) return null;
    var b = rib.getBoundingClientRect();
    var w = b.width;
    if (which === "window") {
      var t = winTrack(w);
      return demoPoint(b.left + (t.x0 + t.x1) / 2, b.top + t.y + t.h / 2, 8, 8, "band window");
    }
    var e = brushEnds();
    var x = ribbonX(which === "to" ? e[1] : e[0], w);
    // Nudged inside the strip: an end handle sits exactly on the edge at rest, and half of a
    // press at x = 0 lands outside the element.
    x = Math.max(2, Math.min(w - 2, x));
    return demoPoint(b.left + x, b.top + RIBBON_BARS / 2, 8, 8,
                     which === "to" ? "range end" : "range start");
  }

  function demoWhere(kind, arg) {
    var el = demoFind(kind, arg);
    if (!el) return null;
    if (el.scrollIntoView) el.scrollIntoView({ block: "nearest", inline: "nearest" });
    var b = el.getBoundingClientRect ? el.getBoundingClientRect() : el;
    if (!b.width || !b.height) return null;
    return {
      x: Math.round(b.left + b.width / 2),
      y: Math.round(b.top + b.height / 2),
      w: Math.round(b.width), h: Math.round(b.height),
      expect: el.expect || null,
      // How much clear space the aim actually had, when the target knows. Reported rather
      // than asserted: the driver verifies the hit itself, and this is the number that
      // explains a miss.
      gap: el.gap != null ? el.gap : null,
      // Truncated: a control's `title` is help text, not a name -- Refresh's runs to
      // 180 characters and swamped the driver's log line for that beat.
      label: (el.demoLabel ||
              (el.getAttribute && (el.getAttribute("title") || el.id)) ||
              (kind + " " + arg)).slice(0, 44)
    };
  }

  /* --- the storyboard ---------------------------------------------------- */

  // THE DEMO ITSELF. Everything above is machinery; this is the script, and it is meant
  // to grow -- append beats here and the driver picks them up with no other change.
  //
  // Beats are DATA, not functions, because the thing executing them is in another
  // process. Seven verbs:
  //   {settle}                          wait until nothing is animating
  //   {click, target}                   move the real pointer there and click it
  //   {hover, target}                   move there and stop, to let a hover state show
  //   {dblclick, target}                two clicks inside the double-click window
  //   {drag: [dx, dy], target}          press there, glide by the offset, release
  //   {wheel: n, target}                n notches over it; positive zooms in
  //   {park}                            pointer back to the vetted nothing-under-it spot
  //
  // A target is [kind, arg]. Beyond the legend kinds there are synthetic ones, because the
  // camera, the ribbon handles and the heatmap's cells are positions rather than elements:
  //   ["stage", "centre"] or ["stage", "0.3,0.4"]   a point on the graph
  //   ["brush", "from" | "to" | "window"]           a handle on the date ribbon
  //   ["day", "2026-08-19"] / ["busiest", "1"]      a cell in the heatmap band
  //   ["year", "busiest"] or ["year", "2024"]       a year chip under the ribbon
  // `why` is for the log and the eventual captions -- it is the only part of a beat a
  // person reads.
  /**
   * ORDERED BY IMPACT, not by where the code lives.
   *
   * The storyboard used to run roughly in the order the features were built: the intro, a
   * couple of hovers, then the legend, the colour picker, the camera, and the date ribbon
   * LAST. That is backwards for a recording. A README hero is watched for a few seconds
   * before the reader decides whether to keep watching, so the two things this page does
   * that nothing else does -- the vault growing out of nothing, and a note lifting out of
   * the disc with its links lit -- have to be in those seconds. The colour picker, which is
   * a preference panel, was landing before the timeline, which is the point of the tool.
   *
   * The acts now run: the vault growing, one note, pinning a note, the timeline, the
   * heatmap, folders, subfolders, subfolder colours, the camera, colours. Roughly
   * descending in "would this make someone stop scrolling", with two exceptions that
   * are structural rather than editorial:
   *
   *   * FOLDERS BEFORE SUBFOLDERS, because the subfolder rows do not exist until a twisty
   *     has been clicked, and `only` must run while 03 is folded -- soloing while it is
   *     open deletes its rows and pulls everything below them up by 97px, so the pointer
   *     ends up over a row three below the one it clicked. That cost a take.
   *   * COLOURS LAST, because colour is a preference and every earlier act should land
   *     on the disc's own palette, not one an earlier take happened to leave behind.
   *
   * COLOURS IS EXCLUDED FROM THE FULL RUN -- see FULL_RUN_EXCLUDES, just below this
   * function. It still exists here and still plays on its own via `demoAct("colours")`,
   * for docs/features/colours.md's own clip: right-clicking a top-level folder's row for
   * its colour picker. But subfoldercolor already puts that same right-click menu on
   * camera, one level down, and running the identical gesture twice in the same take
   * added length without showing the reader anything new.
   *
   * PIN IS EARLY ON PURPOSE, not tucked in near the end where it used to sit. It is the
   * one act that puts a note somewhere other than its lattice seat, and it earns being
   * seen before the reader has settled into "this is a filter-and-browse tool". The
   * cost is real and accepted: it leaves up to three notes sitting in the hub for
   * every act after it, so a re-record of the full storyboard shows a filled hub in
   * the timeline, heatmap, folders, subfolders and camera acts too. That is not a bug
   * to route around -- the hub is not a special case to cut away from, it is what the
   * disc looks like once you have used it.
   *
   * Beats are DATA, so reordering is reordering -- the driver holds no state between them.
   * What it is NOT free of is the ordering constraints above and the ones each act names.
   *
   * Every beat below also carries an `act:` tag naming which of these ten it belongs to
   * -- `demoAct(name)`, just after this function, plays one act on its own instead of the
   * whole thing, for the per-feature clips in docs/features.md. `demoFullStoryboard()`,
   * also just below, is the same ten with `colours` filtered back out again for the full
   * run.
   */
  function demoMode() {
    return [
      /* --- 1. the vault, growing --------------------------------------- */
      // The intro is a BEAT, not the page's own boot animation -- see the `?demo` branch
      // at the bottom of this file. Refresh is the real control for "replay it", so the
      // demo presses it rather than calling playTimeline() behind the scenes.
      //
      // It also shows the STRIP now: the intro sweeps the range end from one end of the
      // ribbon to the other, with the date under the handle, so this one beat says both
      // "here is the vault" and "here is the control that scrubs it". Which is why the act
      // that picks that handle up by hand comes next but one.
      { settle: true, act: "intro", why: "start from a disc at rest" },
      { click: true, target: ["id", "refresh"], act: "intro", why: "replay the intro on camera" },
      { settle: true, act: "intro", why: "the vault grows from its first note to now, and the range end sweeps with it" },

      /* --- 2. one note -------------------------------------------------- */
      // Hovering a note names it, lifts it, and lights its links while the rest of the
      // disc recedes. The FOLDERS here are chosen, not incidental: daily notes and
      // weekly reviews are titled by date, so the label that appears on camera carries
      // no personal information -- unlike a note in 03 - Resources / People.
      // "04"/"05" resolve by prefix on a PARA-ish vault and fall back to the largest
      // group elsewhere. Both are date-titled folders on the vaults this is recorded
      // against -- see the note on demoNoteRect.
      //
      // BEFORE ANY FILTER RUNS, which is why this act moved up past the legend as well as
      // past the ribbon: demoNoteRect picks the most ISOLATED visible note, and the more
      // the disc has been thinned the further that aim drifts from a typical note.
      { hover: true, target: ["note", "04"], act: "note", why: "hover a daily note" },
      { hover: true, target: ["note", "05"], act: "note", why: "hover a meeting note" },

      /* --- 3. pinning a note ---------------------------------------------- */
      // EARLY, not tucked away near the end -- it is the one thing on this page that
      // puts a note somewhere OTHER than its lattice seat, and it is worth seeing before
      // the timeline/heatmap/folder acts that follow, which then run against a disc with
      // a filled hub for the rest of the recording. That is a real change to what those
      // later acts show on a re-record, and it is the point: the hub is not a special
      // case to cut away from, it is what the disc looks like once you have used it.
      //
      // Three DIFFERENT notes, one per gesture -- pinning is a TOGGLE (see togglePin), so
      // running two gestures against the same note would have the second one release what
      // the first just placed, and the act would end with fewer pins than it showed
      // landing. biginner rather than a folder prefix for the drag -- see
      // demoBigInnerNote: a drag has to survive a press, a glide and a release landing
      // correctly under real load, and a big dot on the inner ring is both the easiest
      // hit-test and the shortest trip to the hub.
      { drag: true, target: ["biginner"], act: "pin", to: ["stage", "centre"],
        why: "drag a note into the hole to pin it" },
      { settle: true, act: "pin", why: "the hub opens and the ring closes around where it was" },
      { rightclick: true, target: ["note", "05"], act: "pin", why: "right-click a note -- pins the same way" },
      { settle: true, act: "pin", why: "let the second pin land" },
      { click: true, target: ["note", "03"], act: "pin", why: "click a note to open its card" },
      { click: true, target: ["pin"], act: "pin", why: "...and pin it from the card itself" },
      { settle: true, act: "pin", why: "let the third pin land" },
      // Closed rather than left open: every other act starts from "nothing selected",
      // and this is the one act that opens a card with nothing later in its own beats
      // to close it again.
      { click: true, target: ["detailclose"], act: "pin", why: "close the card" },

      /* --- 4. the compact axis -------------------------------------------- */
      // The strip's own reflow, its own act now rather than folded into the timeline one:
      // the demo vault carries a genuinely sparse tail behind its two dense years on
      // purpose (github#23), so off-then-on has something real to show -- years snapping
      // back to plain calendar width, then regathering around where the notes actually
      // are. Split out because the feature earns its own gallery entry (docs/features/
      // compactaxis.md), the same way pin and subfoldercolor did rather than being a
      // paragraph inside an existing page.
      { click: true, target: ["id", "compact"], act: "compactaxis", why: "turn off the compact axis -- back to one width per month" },
      { settle: true, act: "compactaxis", why: "let the strip spread back out to plain calendar time" },
      { click: true, target: ["id", "compact"], act: "compactaxis", why: "...and back on, weighted by note count again" },
      { settle: true, act: "compactaxis", why: "let it compact again" },

      /* --- 5. the timeline ---------------------------------------------- */
      // The strip under the band carries every month of the vault, and it is the timeline:
      // two handles that are the filter, a pill below them for the 52 weeks the grid above
      // is drawing, and a chip per year. Moved up from LAST -- the sidebar's rank slider is
      // gone and this is the only timeline now, so it belongs beside the intro that has
      // just swept it rather than after the preference panel.
      //
      // The `to` handle first, and deliberately the same one the intro moved: the intro
      // showed it travelling, this shows a hand doing it. The disc waits for the release on
      // each of these, by design -- a drag repaints one small canvas and the filter lands
      // once, when the button comes up.
      { drag: [-320, 0], target: ["brush", "to"], act: "timeline", why: "pull the range end back by hand -- the handle the intro just swept" },
      { settle: true, act: "timeline", why: "let the disc thin out" },
      { drag: [200, 0], target: ["brush", "from"], act: "timeline", why: "...and bring the range start forward" },
      { settle: true, act: "timeline", why: "let it thin further" },

      // The band's window, moved on its own. The range above stays exactly where it was --
      // which is most of what this act is for: they are two instruments, not one. Before the
      // year chips on purpose: a year click sets the range to that year and can drag the
      // window along with it, and this act is clearer split into "the window, on its own"
      // then "the year chip, which touches both" rather than the other way round.
      { drag: [-260, 0], target: ["brush", "window"], act: "timeline", why: "slide the heatmap window back on its own" },
      { settle: true, act: "timeline", why: "let the band redraw" },
      { drag: [170, 0], target: ["brush", "window"], act: "timeline", why: "...and forward again" },
      { settle: true, act: "timeline", why: "let the band redraw" },

      // THE YEAR CHIPS, which have never been in the demo. Hover haloes that year's notes
      // wherever they landed; clicking sets the range to that calendar year, and the chip
      // reads pressed. "busiest" picks the fullest year that has a chip, so the hover
      // always lights something -- see demoFind.
      { hover: true, target: ["year", "busiest"], act: "timeline", why: "hover a year to find it on the disc" },
      { click: true, target: ["year", "busiest"], act: "timeline", why: "...and click it to filter to that year" },
      { settle: true, act: "timeline", why: "let the year land" },

      // Clear it, so everything after this runs on the whole vault. Also puts the window
      // back, which is what makes the `busiest` targets in the next act land on cells that
      // are actually on screen.
      { click: true, target: ["id", "rangeall"], act: "timeline", why: "clear the date range" },
      { settle: true, act: "timeline", why: "let the whole vault come back" },

      /* --- 6. the heatmap ----------------------------------------------- */
      // Hovering a day haloes the notes added that day, wherever they landed on the disc.
      // Ranked by what is VISIBLE rather than by date, so this works on any vault.
      { hover: true, target: ["busiest", "1"], act: "heatmap", why: "hover the busiest day" },
      { hover: true, target: ["busiest", "2"], act: "heatmap", why: "...and the next" },
      { hover: true, target: ["busiest", "3"], act: "heatmap", why: "...and the next" },

      // AND CLICKING KEEPS IT. This is what replaced the sidebar's "Mark today" in 1.7.0:
      // a picked day's notes are recoloured to the neutral extreme as well as haloed, and
      // it stays when the pointer leaves. Clicking today's square -- the last cell of the
      // grid -- is the whole of what that button did.
      //
      // The busiest day rather than today, because today is allowed to hold no notes and a
      // beat that marks nothing reads as a mis-click. Clicked twice, so nothing is left
      // marked for the acts below.
      { click: true, target: ["busiest", "1"], act: "heatmap", why: "click a day to keep it marked -- recoloured, haloed, nothing moved" },
      { settle: true, act: "heatmap", why: "let the mark ramp in" },
      { click: true, target: ["busiest", "1"], act: "heatmap", why: "...and click again to let it go" },
      { settle: true, act: "heatmap", why: "let it ramp back" },

      /* --- 7. folders --------------------------------------------------- */
      // Hiding: the wedges reallocate and the disc stays a full circle.
      { click: true, target: ["eye", "06"], act: "folders", why: "hide a folder -- the wedges reallocate" },
      { settle: true, act: "folders", why: "let the wedges reallocate" },

      // And `only`, which is the fastest way to answer "where does one folder live".
      // SAFE HERE because nothing has been unfolded yet: see the note at the top about the
      // 97px row shift that soloing an unfolded legend causes.
      { click: true, target: ["only", "08"], act: "folders", why: "solo a single folder" },
      { settle: true, act: "folders", why: "let everything else recede" },

      { click: true, target: ["id", "allon"], act: "folders", why: "show everything again" },
      { settle: true, act: "folders", why: "let the whole disc come back" },

      /* --- 8. subfolders ------------------------------------------------ */
      // The tree starts folded, so getting to a subfolder means opening its folder first.
      // That is the honest sequence and it is worth showing: the disc already draws 03's
      // sub-wedges, and this is where the legend admits they are there. It is also
      // load-bearing -- the row the next beats aim at does not exist until this has run.
      //
      // SELF-CONTAINED for isolated recording: unlike the full storyboard's ordering rule
      // above (folders before subfolders, because soloing while 03 is unfolded shifts
      // rows), this act never solos anything itself -- it only unfolds, hovers, clicks and
      // folds back. Recorded alone it needs nothing an earlier act left behind.
      { click: true, target: ["twisty", "03"], act: "subfolders", why: "unfold a folder to reach its subfolders" },

      // HOVER FIRST, and at both levels. It is the cheaper question and the one you would
      // try first: a halo, with nothing hidden and no wedge moved.
      { hover: true, target: ["group", "01"], act: "subfolders", why: "hover a folder to find it on the disc" },
      { hover: true, target: ["sub", "03/People"], act: "subfolders", why: "...and one subfolder inside it" },

      // Then the click, which is the same question answered permanently: highlighting is
      // a SEPARATE axis from visibility -- the whole point of the eye being its own
      // control -- and on a subfolder that owns a sub-wedge it moves as a block rather
      // than only being ringed. Hover haloes; a click also pushes. Shown back to back so
      // the difference is visible rather than asserted.
      { click: true, target: ["sub", "03/People"], act: "subfolders", why: "click it instead: haloed AND pushed out" },
      { settle: true, act: "subfolders", why: "let the sub-wedge push out" },
      { click: true, target: ["sub", "03/People"], act: "subfolders", why: "...and let it back down" },
      { settle: true, act: "subfolders", why: "let it settle back" },

      { click: true, target: ["twisty", "03"], act: "subfolders", why: "fold the subfolders away again" },

      /* --- 9. subfolder colours ------------------------------------------ */
      // The same right-click menu the top-level colours act uses (see there), on a
      // SUBFOLDER row instead of a folder's own -- which does not exist until its
      // twisty is open, the same precondition the subfolders act above needs.
      // SELF-CONTAINED for isolated recording, for the same reason "subfolders" is.
      { click: true, target: ["twisty", "03"], act: "subfoldercolor", why: "unfold a folder to reach its subfolders" },
      { settle: true, act: "subfoldercolor", why: "let the subfolder rows land" },
      { rightclick: true, target: ["sub", "03/People"], act: "subfoldercolor",
        why: "right-click a subfolder for its own colour menu" },
      { settle: true, act: "subfoldercolor", why: "let the menu open" },
      { click: true, target: ["ctxswatch", "g5"], act: "subfoldercolor", why: "give it a colour of its own" },
      { settle: true, act: "subfoldercolor", why: "the disc repaints" },
      { rightclick: true, target: ["sub", "03/People"], act: "subfoldercolor", why: "right-click it again" },
      { settle: true, act: "subfoldercolor", why: "let the menu open" },
      { click: true, target: ["ctxswatch", ""], act: "subfoldercolor", why: "...and put it back to automatic" },
      { settle: true, act: "subfoldercolor", why: "let the tint snap back" },
      { click: true, target: ["twisty", "03"], act: "subfoldercolor", why: "fold the subfolders away again" },

      /* --- 10. the camera ------------------------------------------------ */
      // Zoom in a few notches rather than one. One notch is a fifth now, which is the point
      // -- it is a scroll and not a teleport -- and a single notch on camera looks like
      // nothing happened.
      { wheel: 4, target: ["stage", "0.42,0.40"], act: "camera", why: "zoom in, a fifth per notch" },
      { settle: true, act: "camera", why: "let the last notch land" },

      // Then pan, which is only possible now that the disc is not pinned to the middle. Held
      // button the whole way, or the page sees a click and a release with nothing between.
      { drag: [190, 110], target: ["stage", "0.55,0.45"], act: "camera", why: "drag the disc around" },
      { settle: true, act: "camera", why: "let the pan settle" },

      // Two ways back, both shown, because the button is discoverable and the double-click is
      // faster once you know it.
      { dblclick: true, target: ["stage", "centre"], act: "camera", why: "double-click anywhere to reset" },
      { settle: true, act: "camera", why: "let the view come back" },
      { wheel: 3, target: ["stage", "0.60,0.55"], act: "camera", why: "zoom in again, to have something to reset" },
      { settle: true, act: "camera", why: "let it land" },
      { click: true, target: ["id", "reset"], act: "camera", why: "...and the reset button in the corner" },
      { settle: true, act: "camera", why: "let the view come back" },

      /* --- 11. colours -------------------------------------------------- */
      // LAST regardless: colour is still a preference, and letting every earlier act
      // land on an unmodified palette keeps a re-record of any of them from picking up
      // a colour choice this one made.
      //
      // RIGHT-CLICK, not the gear -- the same twelve swatches, reached at the row
      // itself instead of a settings panel one trip away. The gear panel still exists
      // (see the settings-tab screenshot) but is not what this page leads with any
      // more, on either host: it is slower for the same outcome. Two folders are
      // recoloured rather than one, because one swatch click looks like a highlight
      // and two look like a choice; and the second is a grey, which is the answer to
      // "can a folder recede on purpose" that the archives rule only implies.
      { rightclick: true, target: ["group", "01"], act: "colours", why: "right-click a folder for its own colour menu" },
      { settle: true, act: "colours", why: "let the menu open" },
      { click: true, target: ["ctxswatch", "g8"], act: "colours", why: "give it a colour of its own" },
      { settle: true, act: "colours", why: "the disc repaints -- no relayout, nothing moves" },
      { rightclick: true, target: ["group", "03"], act: "colours", why: "right-click another folder" },
      { settle: true, act: "colours", why: "let the menu open" },
      { click: true, target: ["ctxswatch", "g11"], act: "colours", why: "...and let it go grey" },
      { settle: true, act: "colours", why: "let the second repaint land" },
      { rightclick: true, target: ["group", "01"], act: "colours", why: "right-click the first folder again" },
      { settle: true, act: "colours", why: "let the menu open" },
      { click: true, target: ["ctxswatch", ""], act: "colours", why: "put it back to automatic" },
      { settle: true, act: "colours", why: "let it snap back" },
      { rightclick: true, target: ["group", "03"], act: "colours", why: "...and the second" },
      { settle: true, act: "colours", why: "let the menu open" },
      { click: true, target: ["ctxswatch", ""], act: "colours", why: "put it back to automatic too" },
      { settle: true, act: "colours", why: "let the palette snap back" },

      // Pointer out of the way, so the last frame is the disc rather than a hover state
      // left behind by the last click.
      { park: true, act: "colours", why: "leave the final frame clean" }
    ];
  }

  // ACTS THAT EXIST FOR THEIR OWN PER-FEATURE CLIP, but do not appear in the full run.
  // Just "colours" for now: subfoldercolor already puts the same right-click colour menu
  // on camera one level down, so replaying the identical gesture on a top-level folder
  // too, right at the end of an 85-beat take, cost length without showing anything new.
  var FULL_RUN_EXCLUDES = ["colours"];

  // THE FULL STORYBOARD, with FULL_RUN_EXCLUDES filtered back out of demoMode()'s single
  // list -- what the hero recording actually plays. demoMode()'s own trailing park beat
  // is tagged "colours" and goes with the rest of that act, so whatever act ends up last
  // here gets its own if it does not already have one, the same rule demoAct() applies
  // for an isolated act missing one.
  function demoFullStoryboard() {
    var beats = demoMode().filter(function (b) { return FULL_RUN_EXCLUDES.indexOf(b.act) === -1; });
    if (!beats[beats.length - 1].park) {
      beats = beats.concat([{ park: true, act: beats[beats.length - 1].act, why: "leave the final frame clean" }]);
    }
    return beats;
  }

  // ONE ACT IN ISOLATION, for a per-feature clip instead of the whole storyboard. `name`
  // is one of the ten `act:` tags above (`intro`, `note`, `pin`, `timeline`, `heatmap`,
  // `folders`, `subfolders`, `subfoldercolor`, `camera`, `colours`) -- the same names
  // the section comments in demoMode() already carry, so tagging a beat and naming it
  // here is one decision, not two.
  //
  // Isolated acts don't need the full storyboard's ordering rules (folders-before-
  // subfolders): under `?demo` the page comes up at rest with no filter applied and
  // the gear closed (see the `?demo` branch near the end of this file), which is
  // exactly the state every act other than "intro" already assumes as ITS starting
  // point in the combined run too. A leading `settle` is enough; nothing needs a reset
  // click first. `park` is appended rather than duplicated from the full
  // storyboard's tail, so a recording of any single act ends on a clean frame the same
  // way the full one does.
  //
  // Unknown name -> empty array with a loud warning, so a typo surfaces immediately in the
  // recorder's log instead of quietly producing a beat-less, near-instant "recording".
  function demoAct(name) {
    var beats = demoMode().filter(function (b) { return b.act === name; });
    if (!beats.length) {
      console.warn("demo: no beats tagged act=\"" + name + "\" -- check the name against demoMode()");
      return [];
    }
    if (name === "intro") return beats;   // already starts with its own settle beat
    // "colours" already ends on its own {park} -- that beat WAS the full storyboard's
    // final beat, just tagged into the act whose section comment it happened to sit under.
    // Appending a second one is exactly the double-park a manual --act run caught: two
    // "leave the final frame clean" beats in a row when this went untested.
    var out = [{ settle: true, act: name, why: "start from a disc at rest" }].concat(beats);
    if (!beats[beats.length - 1].park) {
      out = out.concat([{ park: true, act: name, why: "leave the final frame clean" }]);
    }
    return out;
  }

  // Everything the driver needs, and nothing it does not.
  var demoApi = {
    on: demoOn,
    doneTitle: DEMO_DONE_TITLE,
    storyboard: demoFullStoryboard,
    act: demoAct,
    busy: demoBusy,
    /**
     * WHICH of the five things busy() ors together is still running.
     *
     * busy() answers "is anything moving", which is the right question for a driver deciding
     * whether to act. It is the wrong question for a driver that has GIVEN UP waiting: then
     * the only useful thing to know is what it was waiting for, and a boolean cannot say.
     * Every "settle timed out" before this was a guess between five candidates.
     */
    busyWhy: function () {
      return { play: !!play, cascade: !!cascadeRun, anim: !!anim,
               hover: !!hoverRaf, highlight: !!hlRaf };
    },
    where: demoWhere,
    // The recorder's visible pointer -- see demoCursorAt for why it lives in the page
    // rather than at the OS level.
    cursorAt: demoCursorAt,
    cursorHide: demoCursorHide,
    // What is hovered right now. The driver compares this against a target's `expect`
    // after a hover beat: aiming at a dot is only as good as the hit-test agreeing, and
    // a silent miss puts the wrong note's NAME on camera.
    hovered: function () { return state.hovered; },
    // Called by the driver when the last beat lands. The title is the signal on
    // purpose: a screen recorder outside the browser can poll a window title with no
    // debugging port of its own, no extension and nothing injected.
    finish: function (ms, trace) {
      window.__vgDemoDone = { ms: ms, trace: trace || [] };
      DOC.title = DEMO_DONE_TITLE;
      return true;
    }
  };

  /* ---- END: demo automation + debug API ---- */

  /* ------------------------------------------------------------------ go */

  WIN.setTimeout(function () {
    makeRenderer();
    // Debug handle: lets a test page inspect live layout state from outside.
    API = window.__vg = { graph: graph,
                    // Re-read the palette from CSS. Called once at init, and again by a
                    // host whose theme changed: THEME is a snapshot, so without this a
                    // theme flip restyles the DOM and leaves every canvas colour behind.
                    readTheme: readTheme, get renderer() { return renderer; },
                    // Logo internals: placeLogo has to be callable directly, because
                    // refresh() only schedules a render and a tab that is not being
                    // composited never runs one -- so testing the mark through the
                    // renderer silently measures a stale DOM.
                    placeLogo: placeLogo,
                    // Folder colours, for the two settings UIs and for the suite. The
                    // setter repaints rather than rebuilds -- colour is not an input to
                    // the layout, and an override must not be able to move a node.
                    palette: paletteInfo,
                    groupOrder: function () { return (order[state.dim] || []).slice(); },
                    groupCount: function (g) { return counts[g] || 0; },
                    // The slot a group is ON, which is not derivable from its position any
                    // more: archives are skipped in the rotation, and sit on no slot at
                    // all. "" means exactly that.
                    slotOf: function (g) { return groupSlot[g] || ""; },
                    // The slot a group would be on with no override at all -- unlike
                    // slotOf, never affected by folderColors. "" for the same reason.
                    // Kept alongside slotOf (not with the rest of the debug API below,
                    // which the plugin build strips) because plugin/main.js's own
                    // settings tab calls it to mark the automatic swatch even once a
                    // folder has been given an explicit colour.
                    autoSlotOf: function (g) { return groupAutoSlot[g] || ""; },
                    setFolderColors: applyFolderColors,
                    setSubfolderColors: applySubfolderColors,
                    setFolderShown: applyFolderShown,
                    // The saved default, applied live. Mirrors setFolderShown: the host owns
                    // the store and this owns the camera.
                    setPanEnabled: function (v) { return setPan(v !== false, false); },
                    // Same shape as setPanEnabled, for the plugin's View-settings toggle
                    // (github#23). Kept alongside setPanEnabled (not with the rest of the
                    // debug API below, which the plugin build strips) because plugin/main.js's
                    // own settings tab calls it to live-update an already-open view -- it was
                    // misplaced inside the stripped region for a while, which meant toggling
                    // "Compact date axis" in the plugin saved correctly but silently never
                    // updated a view already open, unlike every sibling toggle here.
                    setCompactAxis: function (v) { return setCompactAxis(v !== false, false); },
                    // Push the defaults into the live filter and repaint. This is the
                    // "and now show it" half, kept separate so loading saved settings at
                    // boot cannot be confused with a person clicking an eye.
                    applyHiddenDefaults: function () {
                      seedHidden();
                      buildLegend();
                      cascade(null, { colToggle: true });
                    },
                    // The band, for the same reason placeLogo is exposed: it paints
                    // from afterRender, so a tab that is not being composited never
                    // repaints it and testing through the renderer measures a stale
                    // canvas.
                    heatBuild: heatBuild,
                    // PLAN PARITY. The cascade must animate between the static
                    // planner's own outputs, or it walks between packings nothing else
                    // renders -- which is every jump chased on 2026-08-22. This
                    // compares, for the CURRENT visibility state, the plan the static
                    // path builds against the one the cascade would end on. Per-cell
                    // row counts and maxR must match exactly. Run it with a folder
                    // hidden, not just at full vault, since the REPACK_BELOW flag is
                    // what used to differ.
                    checkPlanParity: function () {
                      var shown = 0;
                      graph.forEachNode(function (id) { if (visible(id)) shown++; });
                      var ov = true;
                      var stat = buildWedgePlan(ov, function (id) { return visible(id) ? 1 : 0; });
                      var live = buildWedgePlan(ov, function (id) { return alpha[id] || 0; });
                      var diffs = {};
                      var rows = function (p) { var m = {}; p.cells.forEach(function (c) { m[c.k] = c.rows; }); return m; };
                      var rs = rows(stat), rl = rows(live);
                      Object.keys(rs).concat(Object.keys(rl)).forEach(function (k) {
                        if (rs[k] !== rl[k]) diffs[k] = { staticPlan: rs[k], livePlan: rl[k] };
                      });
                      var out = {
                        shown: shown, threshold: Math.round(graph.order * REPACK_BELOW), onlyVisible: ov,
                        staticMaxR: Math.round(stat.maxR), liveMaxR: Math.round(live.maxR),
                        maxRMatches: Math.round(stat.maxR) === Math.round(live.maxR),
                        cellsStatic: stat.cells.length, cellsLive: live.cells.length,
                        rowDiffs: diffs, parityOK: Object.keys(diffs).length === 0 &&
                          Math.round(stat.maxR) === Math.round(live.maxR)
                      };
                      return out;   // the caller is a console; it prints this itself
                    },
                    // Focus-web check (issue #2): select the best-connected note, composite
                    // the canvases in stacking order, sample the lit curves where they pass
                    // inside a NON-focus disc -- dimAtGaps must be 0. A sample the hovers
                    // canvas painted opaque and not blue is under the focus note's own label
                    // pill (or a lit neighbour's disc over a dim one), which sit above the web
                    // on purpose; those count apart as underLabel. The web is thin and sampled
                    // at one device pixel, so a miss looks one pixel around before it counts.
                    checkFocusWeb: function () {
                      var best = null, bd = -1;
                      // Skip anything the current filter/opacity has hidden, or picking the
                      // graph's highest-degree note regardless of what's actually on screen
                      // can land on one with zero visible edges -- geomGaps stays 0 and the
                      // check reports "nothing to measure" without having exercised the fix.
                      graph.forEachNode(function (id) {
                        var d = renderer.getNodeDisplayData(id);
                        if (!d || d.hidden) return;
                        if (graph.degree(id) > bd) { bd = graph.degree(id); best = id; }
                      });
                      var keepSel = state.selected, keepHov = state.hovered;
                      state.selected = best; state.hovered = null;
                      renderer.refresh({ skipIndexation: true }); renderer.render();
                      var cv = renderer.getCanvases();
                      var order = ["edges", "nodes", "edgeLabels", "labels", "hovers", "hoverNodes"];
                      var W = cv.nodes.width, H = cv.nodes.height, dpr = W / renderer.getDimensions().width;
                      var off = DOC.createElement("canvas"); off.width = W; off.height = H;
                      var ctx = off.getContext("2d");
                      ctx.fillStyle = css("--surface-1"); ctx.fillRect(0, 0, W, H);
                      order.forEach(function (k) { if (cv[k]) ctx.drawImage(cv[k], 0, 0); });
                      var img = ctx.getImageData(0, 0, W, H).data;
                      var hov = DOC.createElement("canvas"); hov.width = W; hov.height = H;
                      var hctx = hov.getContext("2d"); hctx.drawImage(cv.hovers, 0, 0);
                      var himg = hctx.getImageData(0, 0, W, H).data;
                      var at = function (data, x, y) {
                        var X = Math.round(x * dpr), Y = Math.round(y * dpr);
                        if (X < 0 || Y < 0 || X >= W || Y >= H) return null;
                        var i = (Y * W + X) * 4;
                        return [data[i], data[i + 1], data[i + 2], data[i + 3]];
                      };
                      var hi = toRgb(THEME.edgeHi), dm = toRgb(THEME.dim);
                      var dist = function (c, t) { return c ? Math.abs(c[0] - t[0]) + Math.abs(c[1] - t[1]) + Math.abs(c[2] - t[2]) : 1e9; };
                      var set = focusSet() || {}, dims = [];
                      graph.forEachNode(function (id) {
                        if (set[id]) return;
                        var d = renderer.getNodeDisplayData(id);
                        if (!d || d.hidden) return;
                        var p = renderer.graphToViewport(graph.getNodeAttributes(id));
                        dims.push({ x: p.x, y: p.y, rad: renderer.scaleSize(d.size) });
                      });
                      var res = { node: best, degree: bd, edges: 0, samples: 0, geomGaps: 0,
                                  blueAtGaps: 0, dimAtGaps: 0, underLabel: 0, otherAtGaps: 0 };
                      var seen = Object.create(null);
                      Object.keys(set).forEach(function (n) {
                        graph.forEachEdge(n, function (e, attrs, s, t) {
                          if (seen[e] || !set[s] || !set[t]) return;
                          seen[e] = true;
                          var geo = edgeCurveGeom(e, s, t);
                          if (!geo) return;
                          res.edges++;
                          var ps = geo.ps, pt = geo.pt, cp = geo.cp;
                          for (var u = 0.05; u <= 0.95; u += 0.01) {
                            var x = (1 - u) * (1 - u) * ps.x + 2 * (1 - u) * u * cp.x + u * u * pt.x;
                            var y = (1 - u) * (1 - u) * ps.y + 2 * (1 - u) * u * cp.y + u * u * pt.y;
                            res.samples++;
                            var covered = dims.some(function (d) {
                              var ddx = d.x - x, ddy = d.y - y;
                              return ddx * ddx + ddy * ddy <= (d.rad - 0.5) * (d.rad - 0.5);
                            });
                            if (!covered) continue;
                            res.geomGaps++;
                            var c = at(img, x, y), blue = dist(c, hi) < 60;
                            for (var ox = -1; ox <= 1 && !blue; ox++) {
                              for (var oy = -1; oy <= 1 && !blue; oy++) {
                                if (ox || oy) blue = dist(at(img, x + ox / dpr, y + oy / dpr), hi) < 60;
                              }
                            }
                            var h = at(himg, x, y);
                            if (blue) res.blueAtGaps++;
                            else if (h && h[3] >= 250) res.underLabel++;
                            else if (dist(c, dm) < 60) res.dimAtGaps++;
                            else res.otherAtGaps++;
                          }
                        });
                      });
                      state.selected = keepSel; state.hovered = keepHov; renderer.refresh();
                      res.webOK = res.dimAtGaps === 0;
                      return res;
                    },
                    // EVERYTHING NEEDED TO REPRODUCE WHAT IS ON SCREEN, as one object -- behind
                    // the "Debug" button in both hosts. Kept alongside setPanEnabled/
                    // setCompactAxis (not with the rest of the debug API below, which the
                    // plugin build strips) because that button lives in the ordinary UI, not
                    // behind a dev-only toggle -- it was misplaced inside the stripped region
                    // for a while, which meant clicking "Debug" in the Obsidian plugin threw
                    // (API.debugDump is not a function) before ever reaching the clipboard-
                    // write/file-save fallback, so the button did nothing at all.
                    debugDump: function () {
                      var a0 = renderer ? renderer.graphToViewport({ x: 0, y: 0 }) : null;
                      var b0 = renderer ? renderer.graphToViewport({ x: UNIT, y: 0 }) : null;
                      var pxPerRow = a0 && b0 ? Math.hypot(b0.x - a0.x, b0.y - a0.y) : 0;
                      var perPx = pxPerRow > 0 ? UNIT / pxPerRow : 0;
                      // Radii and drawn sizes of everything on screen, split into bands on the
                      // largest radial gap -- the same split every probe in this session used.
                      var pts = [];
                      graph.forEachNode(function (id, a) {
                        if ((alpha[id] || 0) <= 0.004) return;
                        var d = renderer && renderer.getNodeDisplayData(id);
                        // THROUGH scaleSize. getNodeDisplayData().size is what the reducer
                        // RETURNED, not what gets drawn -- sigma scales it again on the way to
                        // the canvas. Read raw, every radius in this dump was short by that
                        // factor, so it reported clearances that were not there and
                        // overlappingPairs: 0 on states that had sixteen. The same trap this
                        // file's dot measurements hit twice before.
                        pts.push({ r: Math.hypot(a.x, a.y), th: Math.atan2(a.y, a.x),
                                   rad: (d && renderer ? renderer.scaleSize(d.size)
                                                       : 4) * perPx, g: a.folder });
                      });
                      pts.sort(function (x, y) { return x.r - y.r; });
                      var gi = 0, gap = 0;
                      for (var i = 1; i < pts.length; i++) {
                        var gg = pts[i].r - pts[i - 1].r;
                        if (gg > gap) { gap = gg; gi = i; }
                      }
                      var r3 = function (v) { return Math.round(v * 1000) / 1000; };
                      var bandStat = function (arr) {
                        if (!arr.length) return null;
                        var rows = {}, steps = [], clears = [], worst = 1e9;
                        arr.forEach(function (q) {
                          var k = Math.round(q.r / 8) * 8;
                          (rows[k] || (rows[k] = [])).push(q);
                        });
                        Object.keys(rows).forEach(function (k) {
                          var row = rows[k].slice().sort(function (x, y) { return x.th - y.th; });
                          for (var i = 1; i < row.length; i++) {
                            var arc = (row[i].th - row[i - 1].th) * (+k);
                            if (!(arc > 1 && arc < 3000)) continue;
                            steps.push(arc);
                            var cl = arc - row[i].rad - row[i - 1].rad;
                            clears.push(cl);
                            if (cl < worst) worst = cl;
                          }
                        });
                        steps.sort(function (x, y) { return x - y; });
                        var q = function (f) {
                          return steps.length ? Math.round(steps[Math.floor(steps.length * f)]) : 0;
                        };
                        var radii = arr.map(function (x) { return x.rad; }).sort(function (x, y) { return x - y; });
                        return {
                          notes: arr.length, rows: Object.keys(rows).length,
                          inner: Math.round(arr[0].r), outer: Math.round(arr[arr.length - 1].r),
                          step35: q(0.35), step95: q(0.95),
                          channelRatio: q(0.35) ? r3(q(0.95) / q(0.35)) : 0,
                          dotRadius: { min: Math.round(radii[0]),
                                       med: Math.round(radii[Math.floor(radii.length / 2)]),
                                       max: Math.round(radii[radii.length - 1]) },
                          worstPairClearance: worst === 1e9 ? null : Math.round(worst),
                          overlappingPairs: clears.filter(function (c) { return c < 0; }).length,
                        };
                      };
                      var cam = renderer ? renderer.getCamera().getState() : null;
                      var hidden = Object.keys(state.hidden[state.dim] || {}).filter(function (k) {
                        return (state.hidden[state.dim] || {})[k];
                      });
                      return {
                        note: "vault-graph debug dump -- paste this back verbatim",
                        vault: { name: DATA.vault || "", notes: graph.order,
                                 // EDGE_TOTAL, not graph.size: in a budgeted vault the graph
                                 // holds only the resting share, and a dump that said
                                 // "links: 8027" about a 37k-link vault would send whoever
                                 // reads it in the wrong direction. linksShown is the budget.
                                 links: EDGE_TOTAL, linksShown: EDGE_SHOWN, lazyEdges: lazyEdges,
                                 generated: DATA.generated || "" },
                        screen: { win: WIN.innerWidth + "x" + WIN.innerHeight,
                                  dpr: WIN.devicePixelRatio || 1,
                                  stage: $("canvas") ? Math.round($("canvas").clientWidth) + "x" +
                                         Math.round($("canvas").clientHeight) : "",
                                  pxPerRow: r3(pxPerRow) },
                        camera: cam ? { x: r3(cam.x), y: r3(cam.y), ratio: r3(cam.ratio) } : null,
                        filters: { hiddenFolders: hidden,
                                   hiddenSub: Object.keys(state.hiddenSub || {}),
                                   range: rangeLabel(),
                                   from: state.from, to: state.to, heatEnd: state.heatEnd,
                                   timelineUntil: state.until,
                                   markDay: state.markDay, shown: pts.length },
                        // The room each band reports and the arc floor in force -- both feed
                        // POSITIONS now, so a jump investigation needs to see them per frame.
                        room: { i: r3(bandOf("i").room), o: r3(bandOf("o").room) },
                        minArcDeg: r3(lastMinArc * 180 / Math.PI),
                        spacing: { spOuter: r3(bandOf("o").sp),
                                   spInner: r3(bandOf("i").sp),
                                   rowsOuter: bandOf("o").rows,
                                   rowsInner: bandOf("i").rows,
                                   pitchOuterUnits: r3(pitchUnits("o")),
                                   pitchInnerUnits: r3(pitchUnits("i")) },
                        seam: { outerDeg: bandOf("o").gapDeg, innerDeg: bandOf("i").gapDeg,
                                nGOuter: bandOf("o").nG, nGInner: bandOf("i").nG,
                                nSubOuter: bandOf("o").nSub, nSubInner: bandOf("i").nSub,
                                fallOuter: r3(seamFall("o")), fallInner: r3(seamFall("i")) },
                        locked: geomLock ? { r0: r3(geomLock.r0), rOuter: r3(geomLock.rOuter),
                                             maxR: r3(geomLock.maxR), rows: geomLock.rows,
                                             bandTotal: geomLock.bandTotal } : null,
                        bands: { inner: bandStat(pts.slice(0, gi)), outer: bandStat(pts.slice(gi)) },
                        dots: { ofPitch: r3(DOT_OF_PITCH), minPx: DOT_MIN_PX,
                                maxSpread: DOT_MAX_SPREAD, m: r3(bandOf("o").ramp.m),
                                b: r3(bandOf("o").ramp.b), lo: r3(bandOf("o").ramp.lo) },
                      };
                    },
    };
    /* ---- BEGIN: demo automation + debug API -- stripped from the plugin build, see scripts/build-plugin.mjs (stripDemoAndDebug) ---- */
    // Object.assign COPIES A VALUE from each of this literal's own `get`/`set` pairs at
    // the moment it runs -- it does not install the accessor itself, so a plain
    // Object.assign(window.__vg, {...}) would freeze every getter/setter below (heat,
    // folderColors, subfolderColors, panEnabled, heatCell, ...) at whatever they
    // happened to return during this one boot-time call, forever after. Caught by
    // smoke.mjs: __vg.heat stayed the pre-heatBuild() null through the whole run, so
    // the page never looked "ready" no matter how long the suite waited.
    // Object.getOwnPropertyDescriptors + Object.defineProperties copies the ACCESSOR
    // itself, so each one keeps reading live off this closure the way it does for the
    // handful of getters in the host API above, which were never in this object and
    // never had the bug.
    var debugAPI = {
                    state: state,
                    ringsLayout: ringsLayout, visible: visible, groupOf: groupOf,
                    alpha: alpha, cascade: cascade, syncAlpha: syncAlpha,
                    // The lazy-edge seam, exposed for the probes: a test that wants to know
                    // whether hover materialisation works should drive the same function the
                    // pointer does, not re-implement it.
                    syncLazyEdges: syncLazyEdges,
                    get lazyEdges() { return lazyEdges; },
                    // The page's own definition of unlinked. A check that re-derives it from
                    // graph.degree gets a different answer in a budgeted vault, where a note
                    // whose links were all trimmed has degree 0 and is not unlinked at all.
                    isOrphan: isOrphan,
                    // The wedge overlay, and the numbers it draws. wedgeEdges() is the honest
                    // answer to "where is this wedge", in the same screen-angle convention as
                    // a node's atan2 -- a probe that re-derives it from lastStart is working in
                    // sweep space and off by the half-gap rotation.
                    wedgeDebug: wedgeDebug, wedgeEdges: wedgeEdges,
                    // The locked band radii, in graph units -- what the boundary rays are
                    // pinned at, and the number a unit slip in it silently disables.
                    bandRef: function () { return geomLock ? geomLock.bandR : null; },
                    // Every line the overlay draws, with its angle at one radius: the only way
                    // to tell a line that is misplaced from a line that is missing.
                    wedgeTrace: function (rLattice) {
                      DBG.trace = []; DBG.traceR = rLattice;
                      drawWedgeDebug();
                      var out = DBG.trace; DBG.trace = null;
                      return out;
                    },
                    // The raw captured cells, for diagnosing what the overlay drew: one entry
                    // per CELL, which is one per sub-wedge, not one per folder.
                    wedgeCells: function () { return (DBG.cells || []).map(function (c) {
                      return { g: c.g, k: c.k, band: c.inner ? "i" : "o", seams: c.seams,
                               f0: c.f0, f1: c.f1, pLead: c.pLead, pTrail: c.pTrail,
                               n: (c.ids || []).length }; }); },
                    // The two terms a wedge edge is made of, so a probe can decompose a
                    // measured swing instead of guessing which one moved.
                    seamDeg: function (bk) { return bandOf(bk).gapDeg || 0; },
                    seamNB: function (bk) { return (bandOf(bk).nG || 0) + (bandOf(bk).nSub || 0); },
                    clearAlpha: clearAlpha, buildWedgePlan: buildWedgePlan,
                    // Both added after wanting them from a test page: applyLayout to
                    // settle without waiting on rAF (which a hidden tab throttles,
                    // making a working animation look like a no-op), and
                    // isHighlighted to check the predicate directly.
                    applyLayout: applyLayout, isHighlighted: isHighlighted,
                    ringColors: ringColors,
                    colorOf: colorOf,
                    isArchiveGroup: isArchiveGroup,
                    get folderColors() {
                      return Object.assign(Object.create(null), folderColors);
                    },
                    // subfolderColors/setSubfolderColors mirror the pair above and are the
                    // host's actual read/write path -- both shell.html and plugin/main.js's
                    // onSubfolderColors go through these. subColorOf/subSlotOf/subOrderOf/
                    // subCountOf below them are NOT consumed by either host: the Obsidian
                    // settings tab builds its subfolder rows from its own path-derived
                    // allSubfolders() instead of asking the api, since a subfolder's
                    // identity (unlike a top-level folder's) never needs an open view to
                    // resolve. These four exist for manual inspection and the suite, the
                    // same role api.colorOf/api.slotOf/api.groupOrder already play one
                    // level up.
                    get subfolderColors() {
                      return Object.assign(Object.create(null), subfolderColors);
                    },
                    subColorOf: function (folder, sub) {
                      return subShade[folder + "/" + (sub || "")] || colorOf(folder);
                    },
                    subSlotOf: function (folder, sub) {
                      return subSlot[folder + "/" + (sub || "")] || "";
                    },
                    subOrderOf: function (g) { return (subOrder[g] || []).slice(); },
                    subCountOf: function (g, sub) { return subCount[g + "/" + (sub || "")] || 0; },
                    // Visibility DEFAULTS. Setting these does not move the live filter --
                    // the host is expected to apply them, which is what setHiddenDefaults
                    // is for.
                    get folderShown() {
                      return Object.assign(Object.create(null), folderShown);
                    },
                    get panEnabled() { return panEnabled; },
                    get compactAxis() { return compactAxis; },
                    // The pooled-tail rank a split cell's key ends in -- exposed so a check can
                    // derive it instead of hardcoding SUB_SLOTS-1, which is not itself public.
                    get subTailRank() { return SUB_SLOTS - 1; },
                    hiddenByDefault: hiddenByDefault,
                    heatDraw: heatDraw,
                    get heat() { return heat; },
                    // Live, for the same reason radialEase and subGap are: the cell
                    // size trades how legible the mosaic inside one square is against
                    // how much of the stage the band takes from the disc, and that is
                    // a looking-at-it decision, not a derivable one. The cap is what
                    // moves; the floor and the width still decide the actual size, so
                    // a narrow window is unaffected.
                    get heatCell() { return HEAT_CELL_MAX; },
                    set heatCell(v) {
                      HEAT_CELL_MAX = Math.max(HEAT_CELL_MIN, +v || HEAT_CELL_MAX);
                      heatBuild();
                      // No return: a setter returning a value is a TypeError under
                      // "use strict", which this file is.
                    },
                    heatReport: function () {
                      if (!heat) return "not built";
                      heatCompute();
                      var lv = [0, 0, 0, 0, 0], nz = 0, top = null;
                      heat.keys.forEach(function (k) {
                        var d = heat.days[k];
                        if (d.n <= 0.004) return;
                        nz++;
                        lv[heatLevel(d.n)]++;
                        if (!top || d.n > top.n) top = { day: k, n: Math.round(d.n) };
                      });
                      var out = {
                        cell: heat.cell, cols: heat.cols, canvas: heat.w + "x" + heat.h,
                        cuts: heat.cuts, nMax: heat.nMax,
                        daysWithNotes: nz, byLevel: lv,
                        blocksAtBusiest: heat.days[top ? top.day : ""]
                          ? heat.days[top.day].parts.length : 0,
                        pxPerNoteAtBusiest: top
                          ? +((heat.cell * heat.cell) / top.n).toFixed(2) : null,
                        busiest: top, inWindow: heat.keys.reduce(function (a, k) {
                          return a + heat.days[k].ids.length; }, 0),
                        earlier: heat.before, later: heat.after, undated: heat.undated,
                        markDay: state.markDay, hoverDay: state.hoverDay
                      };
                      return out;   // the caller is a console; it prints this itself
                    },
                    bandColors: bandColors, outerPresence: outerPresence,
                    get planMs() { return planMs; },
                    get fullRing() { return fullRing; },
                    set fullRing(v) { fullRing = v; },
                    get timeScale() { return TIME_SCALE; },
                    set timeScale(v) { TIME_SCALE = +v > 0 ? +v : 1; },
                    // Spacing knobs, live. Same motive as timeScale: these three
                    // trade edge collisions against interior ones against how much
                    // whitespace the disc carries, and that balance is only findable
                    // by measuring, not by reasoning. Setting any of them needs
                    // bandLock cleared, since pads feed the row counts.
                    // How far a note closes the gap to its target RADIUS per frame.
                    // 1 = follow exactly. Below 1 it smooths a row-count TICK, which
                    // the code above wrongly assumed no longer existed. Live, so it
                    // can be swept against the probe.
                    get radialEase() { return RADIAL_EASE; },
                    set radialEase(v) { RADIAL_EASE = +v > 0 ? Math.min(1, +v) : 1; },
                    get subGap() { return SUB_GAP; },
                    set subGap(v) { SUB_GAP = +v; },
                    get edgePadArc() { return EDGE_PAD_ARC; },
                    set edgePadArc(v) { EDGE_PAD_ARC = +v; },
                    get edgePadMax() { return EDGE_PAD_MAX; },
                    set edgePadMax(v) { EDGE_PAD_MAX = +v; },
                    // ZERO-WEIGHT INVARIANCE. The strongest of the plan guarantees, and the
                    // one that actually catches this class of bug: **a member with no weight
                    // must not change any output.**
                    //
                    // The cascade and the resting path legitimately disagree on MEMBERSHIP --
                    // a departing note is still in the plan while it fades, and gone once it
                    // has -- so they can never be made identical. What must hold instead is
                    // that the extra zero-weight members cost nothing. They did not: the gap
                    // count counted GROUPS PRESENT rather than weight present, so handing over
                    // from one membership set to the other moved every wedge by one 2-degree
                    // gap. Measured, one isolated frame of 33 graph units / 6 screen px after
                    // the animation had already converged.
                    //
                    // This compares the plan over visible notes against the same plan with
                    // every hidden note added back at weight 0. Every cell's rows and maxR must
                    // be identical.
                    checkZeroWeightInvariance: function () {
                      var W = function (id) { return visible(id) ? 1 : 0; };
                      var save = planKeep;
                      planKeep = function (id) { return visible(id); };
                      var lean = buildWedgePlan(true, W);
                      planKeep = function () { return true; };          // hidden notes seated too
                      var padded = buildWedgePlan(true, W);
                      planKeep = save;
                      var rows = function (p) { var m = {}; p.cells.forEach(function (c) { m[c.k] = c.rows; }); return m; };
                      var a = rows(lean), b = rows(padded), diffs = {};
                      // THE UNION, not the lean plan's keys. A cell that exists only in
                      // the padded plan is exactly the seated zero-weight cell this is
                      // about, and iterating `a` alone never compared it -- which is how
                      // github#5 came back as an empty rowDiffs beside a maxR that
                      // differed by a row. checkPlanParity has always done the union.
                      // Missing counts as ZERO on both sides. A cell whose notes are all
                      // hidden is absent from the lean plan and seated at 0 rows in the
                      // padded one, and those are the same statement -- comparing
                      // undefined against 0 would fail every hidden folder on every
                      // vault, which is exactly what it did when this first went in.
                      Object.keys(a).concat(Object.keys(b)).forEach(function (k) {
                        if ((a[k] || 0) !== (b[k] || 0)) {
                          diffs[k] = { withoutZeros: a[k] || 0, withZeros: b[k] || 0 };
                        }
                      });
                      var out = {
                        leanMaxR: Math.round(lean.maxR), paddedMaxR: Math.round(padded.maxR),
                        maxRMatches: Math.round(lean.maxR) === Math.round(padded.maxR),
                        cellsLean: lean.cells.length, cellsPadded: padded.cells.length,
                        rowDiffs: diffs,
                        invariantOK: Object.keys(diffs).length === 0 &&
                                     Math.round(lean.maxR) === Math.round(padded.maxR)
                      };
                      return out;   // the caller is a console; it prints this itself
                    },
                    // DENSITY. The question github#13 is about: does the disc that is on
                    // screen depend on how many notes are on screen, or on how many the
                    // vault happens to hold? Everything here is measured, not planned --
                    // the plan is what the layout intends, and after a cascade the two
                    // agree while during one they do not.
                    //
                    // pitchPx is the whole point. It is one lattice row in SCREEN pixels,
                    // which is what decides whether two notes in a column touch, and with
                    // the normalisation box pinned and the camera still it is invariant to
                    // note count by construction -- so it reads the same at 1500 notes and
                    // at 500, which is the bug.
                    //
                    // pitchRoot is that made scale-free: if a filtered disc were to refill
                    // its box, area per note would scale as 1/n and pitch as its root, so
                    // pitchPx * sqrt(shown) would hold still across every filter state.
                    // That product is the invariant, and it does not need a second vault to
                    // compare against.
                    densityReport: function () {
                      var shown = 0, lit = 0, shownI = 0, shownO = 0;
                      graph.forEachNode(function (id) {
                        if (visible(id)) {
                          shown++;
                          // PER BAND, because the two are packed independently. See pitchRoot.
                          if (bandLock && bandLock[groupOf(id)]) shownI++; else shownO++;
                        }
                        if ((alpha[id] || 0) > 0.004) lit++;
                      });
                      // One lattice row, mapped through the same camera the notes are drawn
                      // with. graphToViewport is the only honest way to ask: it goes through
                      // the pinned bbox, so it answers in the pixels a person sees.
                      var pitchPx = null, pitchPxI = null, unitPx = null, discPx = null;
                      if (renderer) {
                        var a = renderer.graphToViewport({ x: 0, y: 0 });
                        var u = renderer.graphToViewport({ x: UNIT, y: 0 });
                        unitPx = Math.hypot(u.x - a.x, u.y - a.y);
                        // A ROW, not a lattice unit. These were the same number before the
                        // density solve, and pitchPx is the one that decides whether two
                        // notes in a column touch.
                        var b = renderer.graphToViewport({ x: UNIT * (bandOf("o").sp || 1), y: 0 });
                        pitchPx = Math.hypot(b.x - a.x, b.y - a.y);
                        var bi = renderer.graphToViewport({
                          x: UNIT * (bandOf("i").sp || 1) * bandScale("i"), y: 0 });
                        pitchPxI = Math.hypot(bi.x - a.x, bi.y - a.y);
                        // How far the disc actually reaches on screen, from the live radius
                        // rather than the locked one -- the gap between them IS the empty
                        // margin the notes no longer fill.
                        var e = renderer.graphToViewport({ x: (lastMaxR || 0) * UNIT, y: 0 });
                        discPx = Math.hypot(e.x - a.x, e.y - a.y);
                      }
                      // Drawn sizes, as the reducer leaves them -- sizeScale included, which
                      // is the multiplier NODE_MAX does not clamp.
                      var sizes = [];
                      if (renderer) {
                        graph.forEachNode(function (id) {
                          if (!visible(id)) return;
                          var d = renderer.getNodeDisplayData(id);
                          if (d && d.size > 0) sizes.push(d.size);
                        });
                        sizes.sort(function (x, y) { return x - y; });
                      }
                      var med = sizes.length ? sizes[Math.floor(sizes.length / 2)] : null;
                      var r3 = function (v) { return v === null ? null : Math.round(v * 1000) / 1000; };
                      return {
                        shown: shown, lit: lit, total: graph.order,
                        // The locked geometry, and how much of it the notes reach.
                        lockedMaxR: geomLock ? Math.round(geomLock.maxR) : null,
                        liveMaxR: Math.round(lastMaxR || 0),
                        reach: geomLock && geomLock.maxR
                          ? r3((lastMaxR || 0) / geomLock.maxR) : null,
                        r0: geomLock ? r3(geomLock.r0) : null,
                        // The hole as a SHARE of what is drawn. The r0 formula exists to hold
                        // this constant; pinning r0 while the disc shrinks is what breaks it.
                        holeShare: lastMaxR ? r3((geomLock ? geomLock.r0 : 0) / lastMaxR) : null,
                        sp: r3(bandOf("o").sp),
                        unitPx: r3(unitPx),
                        pitchPx: r3(pitchPx),
                        // PITCH TIMES THE ROOT OF THE NOTE COUNT, and both PER BAND.
                        //
                        // The quantity that is conserved is sqrt(area): a band of fixed area
                        // holding n notes on a square lattice has pitch sqrt(area/n), so
                        // pitch * sqrt(n) is the band's own constant. Multiplying the OUTER
                        // band's pitch by the WHOLE disc's note count -- which this reported,
                        // and which was right when one spacing served both rings -- mixes two
                        // bands, so hiding folders from one of them moves it for a reason that
                        // is not a density change. The bands were made independent because a
                        // single spacing made each ring answer for the other's filtering, which
                        // was a reported bug; this is the same correction applied to its
                        // measurement.
                        shownInner: shownI, shownOuter: shownO,
                        pitchPxInner: r3(pitchPxI),
                        pitchRoot: pitchPx ? r3(pitchPx * Math.sqrt(Math.max(1, shown))) : null,
                        pitchRootOuter: pitchPx
                          ? r3(pitchPx * Math.sqrt(Math.max(1, shownO))) : null,
                        pitchRootInner: pitchPxI
                          ? r3(pitchPxI * Math.sqrt(Math.max(1, shownI))) : null,
                        sizeScale: r3(sizeScale),
                        sizeMedian: r3(med),
                        sizeMin: r3(sizes.length ? sizes[0] : null),
                        sizeMax: r3(sizes.length ? sizes[sizes.length - 1] : null),
                        cameraRatio: r3(renderer ? renderer.getCamera().ratio : null)
                      };
                    },
                    // Record each band's radial extent per animated frame. probe(true)
                    // then toggle, then probeReport() -- it names the biggest single
                    // frame step per band, which is what "a jump" actually is.
                    probe: function (on) {
                      // THE SET IS FIXED WHEN THE PROBE STARTS. The band extents below are a
                      // max over notes, and which notes are in that max decided whether the
                      // number meant anything: filtered to present notes it COLLAPSED the frame
                      // a range applied, because the outermost notes are frequently the ones a
                      // range excludes; unfiltered it was pinned by the stale coordinates of
                      // notes that are not drawn at all, which made a build that repositions
                      // departing notes look like a regression against one that strands them
                      // (measured: outerPath 2016 against 0, while the VISIBLE extent of both
                      // moved identically -- worst frame 183 units, total travel 3202 and 3210).
                      // A set fixed at probe start is neither: it cannot collapse, because
                      // membership does not change, and it cannot be pinned by notes nobody can
                      // see, because it only ever contained notes that were drawn.
                      probe = (on === false) ? null
                        : { t0: NOW(), samples: [], prevAng: null, prevR: null,
                            set: (function () {
                              var m = Object.create(null);
                              graph.forEachNode(function (id) {
                                if ((alpha[id] || 0) >= 0.999) m[id] = 1;
                              });
                              return m;
                            })(),
                            watch: arguments.length > 1 ? String(arguments[1]) : null,
                            watched: null, watchSeries: [] };
                      return probe ? "recording" : "off";
                    },
                    probeReport: function () {
                      if (!probe || !probe.samples.length) return "nothing recorded -- call __vg.probe(true) first";
                      var s = probe.samples, worst = { inner: 0, outer: 0 }, at = { inner: 0, outer: 0 };
                      var tanWorst = 0, tanAt = 0, tanWho = null, ngWorst = 0, ngAt = 0;
                      var startWorst = 0, startAt = 0, startG = null, overWorst = 0;
                      for (var i = 1; i < s.length; i++) {
                        var di = Math.abs(s[i].innerMax - s[i - 1].innerMax);
                        var doo = Math.abs(s[i].outerMax - s[i - 1].outerMax);
                        if (di > worst.inner) { worst.inner = di; at.inner = s[i].ms; }
                        if (doo > worst.outer) { worst.outer = doo; at.outer = s[i].ms; }
                        if (s[i].tanStep > tanWorst) { tanWorst = s[i].tanStep; tanAt = s[i].ms; tanWho = s[i].tanId; }
                        var ds = 0, dsG = null;
                        Object.keys(s[i].starts || {}).forEach(function (g) {
                          var was = (s[i - 1].starts || {})[g];
                          if (was === undefined) return;
                          var dd = Math.abs(s[i].starts[g] - was);
                          if (dd > 180) dd = 360 - dd;
                          if (dd > ds) { ds = dd; dsG = g; }
                        });
                        if (ds > startWorst) { startWorst = ds; startAt = s[i].ms; startG = dsG; }
                        var dng = Math.max(Math.abs(s[i].ngO - s[i - 1].ngO), Math.abs(s[i].ngI - s[i - 1].ngI));
                        if (dng > ngWorst) { ngWorst = dng; ngAt = s[i].ms; }
                      }
                      var out = {
                        frames: s.length,
                        spanMs: s[s.length - 1].ms,
                        // The per-note radial worst, and the mean note's move. This is the
                        // radial counterpart of tanMaxStep and the number to judge a jump by;
                        // the band extents below are kept for context but are a max over a
                        // churning set, so their step is not a step in the disc.
                        radMaxStep: (function () {
                          var w = 0, who = null, when = 0;
                          for (var j = 0; j < s.length; j++) {
                            if (s[j].radStep > w) { w = s[j].radStep; who = s[j].radId; when = s[j].ms; }
                          }
                          return { step: w, node: who, atMs: when };
                        })(),
                        radMeanStep: (function () {
                          var t = 0, k = 0;
                          for (var j = 0; j < s.length; j++) { t += s[j].radMean || 0; k++; }
                          return Math.round(k ? t / k : 0);
                        })(),
                        innerMaxStep: worst.inner, innerStepAtMs: at.inner,
                        outerMaxStep: worst.outer, outerStepAtMs: at.outer,
                        // HOW FAR EACH BAND WENT IN TOTAL. A per-frame step means nothing
                        // on its own: a smooth animation over a long distance and a snap
                        // over a short one produce the same number. Reported so a caller
                        // can ask the only question that scales -- is this frame's move a
                        // reasonable multiple of the average frame's share of the trip.
                        // Needed once the lattice spacing began following the visible count
                        // (github#13), which made a range cascade travel much further.
                        innerTravel: Math.abs(s[s.length - 1].innerMax - s[0].innerMax),
                        outerTravel: Math.abs(s[s.length - 1].outerMax - s[0].outerMax),
                        // AND THE PATH, which is the honest denominator. Travel is net, so a
                        // band that moves out and part-way back reports less than it went --
                        // and comparing a frame's step against a net figure then flags a
                        // smooth animation whose target was moving. The path is the sum of
                        // the steps, so path / frames is the mean frame, and a frame can be
                        // judged as a multiple of that.
                        innerPath: (function () {
                          var t = 0;
                          for (var j = 1; j < s.length; j++) t += Math.abs(s[j].innerMax - s[j - 1].innerMax);
                          return Math.round(t);
                        })(),
                        outerPath: (function () {
                          var t = 0;
                          for (var j = 1; j < s.length; j++) t += Math.abs(s[j].outerMax - s[j - 1].outerMax);
                          return Math.round(t);
                        })(),
                        // The tangential jump, and the gap reservation behind it.
                        tanMaxStep: tanWorst, tanStepAtMs: tanAt, tanStepNode: tanWho,
                        // The handover frame, called out on its own: settle() replacing
                        // the interpolation with a fresh rest computation.
                        settleStep: (function () {
                          for (var j = 1; j < s.length; j++) {
                            if (s[j].tag === "settled") {
                              return { tan: s[j].tanStep, over: s[j].tanOver,
                                       mean: s[j].tanMean,
                                       ngBefore: s[j - 1].ngO, ngAfter: s[j].ngO,
                                       startsMoved: (function () {
                                         var m = 0, g = null, a = s[j].starts || {}, b = s[j - 1].starts || {};
                                         Object.keys(a).forEach(function (k) {
                                           if (b[k] === undefined) return;
                                           var d = Math.abs(a[k] - b[k]);
                                           if (d > 180) d = 360 - d;
                                           if (d > m) { m = d; g = k; }
                                         });
                                         return { deg: Math.round(m * 1000) / 1000, group: g };
                                       })() };
                            }
                          }
                          return null;
                        })(),
                        // A wedge boundary moving in one step IS the gap jumping.
                        startMaxStep: Math.round(startWorst * 1000) / 1000,
                        startStepAtMs: startAt, startStepGroup: startG,
                        ngMaxStep: Math.round(ngWorst * 1000) / 1000, ngStepAtMs: ngAt,
                        first: s[0], last: s[s.length - 1], samples: s,
                        watch: probe.watch, watchSeries: probe.watchSeries
                      };
                      return out;   // the caller is a console; it prints this itself
                    },
                    // What is ACTUALLY pushed and haloed right now, grouped by full
                    // path, plus the highlight keys that are set. Reading the code was
                    // not enough to settle whether a depth-2 selection pushes: every
                    // write path stores the clicked path and isPushed only reads
                    // pathKey(a,1), yet the movement is visible. Click the row, then
                    // run this -- if PUSHED is 0 the movement is coming from somewhere
                    // other than the highlight, and the paths tell us where.
                    pushReport: function () {
                      var pushed = [], haloed = [];
                      graph.forEachNode(function (id) {
                        if (isPushed(id)) pushed.push(id);
                        if (isHighlighted(id)) haloed.push(id);
                      });
                      var byPath = function (ids) {
                        var m = Object.create(null);
                        ids.forEach(function (id) {
                          var a = graph.getNodeAttributes(id);
                          var k = a.folder + "/" + (a.dirs || []).join("/");
                          m[k] = (m[k] || 0) + 1;
                        });
                        return m;
                      };
                      var out = {
                        highlightSubKeys: Object.keys(state.highlightSub),
                        highlightGroups: Object.keys(state.highlight),
                        pushedCount: pushed.length,
                        pushedByPath: byPath(pushed),
                        haloedCount: haloed.length,
                        haloedByPath: byPath(haloed)
                      };
                      return out;   // the caller is a console; it prints this itself
                    },
                    // The demo, as data plus two questions about the page. The driver
                    // lives in scripts/demo.mjs and does the input through CDP; nothing
                    // in here clicks anything.
                    demo: demoApi,
                    // The hover tween, for measuring it. Everything visible about a
                    // hover is a function of hoverT, so a ramp that is wrong is only
                    // findable by sampling it -- reading the reducers proves nothing.
                    get hoverT() { return hoverT; },
                    get hoverBusy() { return !!hoverRaf; },
                    // The highlight ramp, per note. Same motive as hoverT: the size and
                    // the ring are functions of it, so a ramp that is wrong is only
                    // findable by sampling it.
                    hl: hl,
                    get hlBusy() { return !!hlRaf; },
                    // Re-derive the locked geometry, then settle. Needed after any of
                    // the above, because r0/rOuter/band membership are locked at load.
                    // The date range, for the suite and the shooter.
                    get dateSpan() { return dateSpan; },
                    setRange: function (fromISO, toISO) {
                      state.from = fromISO ? heatParse(fromISO) : null;
                      state.to = toISO ? heatParse(toISO) : null;
                      applyRange();
                      heatDraw();
                    },
                    setHeatEnd: function (iso) {
                      state.heatEnd = iso ? heatParse(iso) : null;
                      heatBuild(); drawDateUI(); heatDraw();
                    },
                    // SPIKE (github#12, concept E): the pin gesture, for the shooter.
                    // The hub, for the suite and the shooter.
                    pin: function (id) { togglePin(id); },
                    pinned: function () { return state.pinned.slice(); },
                    clearPins: function () { state.pinned = []; hubChanged(false); },
                    lastCascade: function () { return lastCascade; },
                    // The gap the LAST layout pass actually spent, per band. The probe
                    // reports this per frame during an animation; a resting disc has no
                    // frames, and "do two rest states agree about the gap" is the whole
                    // question behind a jump at the end of one.
                    // Where the strip puts a date, for checking the year buttons line up.
                    ribbonXOf: function (ms) { return ribbonX(ms, ribbonW()); },
                    /**
                     * The two ends the strip is DRAWING, and where they are on it.
                     *
                     * Not state.from/state.to: a drag and the intro's sweep are both previews
                     * that deliberately leave state alone, so state cannot answer "where is
                     * the handle". This is brushEnds() -- the one thing drawRibbon reads --
                     * so a check of the sweep is a check of the pixels rather than of a
                     * variable that happens to be nearby.
                     */
                    brushNow: function () {
                      if (!dateSpan) return null;
                      var w = ribbonW(), e = brushEnds();
                      return { from: e[0], to: e[1], fromISO: isoDay(e[0]), toISO: isoDay(e[1]),
                               x0: ribbonX(e[0], w), x1: ribbonX(e[1], w), w: w,
                               sweeping: brushSweep !== null };
                    },
                    lastGap: function () {
                      return { ngI: bandOf("i").nG, ngO: bandOf("o").nG,
                               gapDegI: bandOf("i").gapDeg, gapDegO: bandOf("o").gapDeg };
                    },
                    rangeReport: function () {
                      var lit = 0, dated = 0;
                      // Notes per calendar year. A fixture's date SPREAD is not visible from
                      // its note count or its span -- a vault can claim ten years and hold
                      // nine of them in one -- and every year-scale control on the page reads
                      // this distribution rather than the total.
                      var byYear = Object.create(null);
                      graph.forEachNode(function (id) {
                        if ((alpha[id] || 0) > 0.004) lit++;
                        if (tlMs[id] !== undefined) {
                          dated++;
                          var y = new Date(tlMs[id]).getUTCFullYear();
                          byYear[y] = (byYear[y] || 0) + 1;
                        }
                      });
                      return { byYear: byYear,
                               from: state.from, to: state.to, heatEnd: state.heatEnd,
                               lit: lit, dated: dated,
                               total: graph.order, label: rangeLabel() };
                    },
                    relayout: function () {
                      // CANCEL BEFORE RESETTING, or a still-running cascade's next animation
                      // frame lands after this snap and silently overwrites it -- the same
                      // gap once found and fixed here for setSubwedgeGate (github#31), lost
                      // again when that toggle was removed and this treatment reverted with
                      // it. Clearing pinnedPlan/planKeep too, not just cancelling the frame:
                      // a cascade holds them for its own duration and clears them when it
                      // lands, so a layout after a cancelled-but-not-cleared run would reuse
                      // a plan pinned for a run that no longer exists (see the same teardown
                      // in playTimeline, a few hundred lines up, for the full account).
                      stopPlay();
                      if (cascadeRun) {
                        WIN.cancelAnimationFrame(cascadeRun.raf);
                        WIN.clearTimeout(cascadeRun.guard);
                        cascadeRun = null;
                      }
                      if (anim) { WIN.cancelAnimationFrame(anim); anim = null; }
                      if (animGuard) { WIN.clearTimeout(animGuard); animGuard = null; }
                      pinnedPlan = null; planKeep = null;
                      roomNow = null; cellNow = null; edgeNow = null; colWalk = null;
                      posSrc = posDst = null;
                      bandLock = null; geomLock = null;
                      regroup();
                      applyLayout(false);
                      renderer.refresh();
                    },
    };
    Object.defineProperties(window.__vg, Object.getOwnPropertyDescriptors(debugAPI));
    /* ---- END: demo automation + debug API ---- */
    seedPins();
    buildTimeline();
    buildSearch(); buildTools(); buildStats();
    // Inlined at build time by build-graph.mjs; absent if logo-mask.png was missing,
    // in which case the element simply stays display:none.
    if (LOGO_MASK) {
      var mu = 'url("' + LOGO_MASK + '")';
      $("logo").style.webkitMaskImage = mu;
      $("logo").style.maskImage = mu;
      // The inner layer masks by the logo AND a radial fade, so it occupies the middle
      // of the mark only. mask-size: contain applies to both images, which is what
      // keeps the radial concentric with the art.
      var fade = "radial-gradient(circle at 50% 50%, #000 " + LOGO_INNER_FADE.split(",")[0].trim() +
                 ", transparent " + LOGO_INNER_FADE.split(",")[1].trim() + ")";
      var eli = $("logoInner");
      eli.style.webkitMaskImage = mu + ", " + fade;
      eli.style.maskImage = mu + ", " + fade;
      logoMaskReady = true;
      // Also decoded as an Image, because Save PNG has to composite the mask onto a
      // canvas by hand -- a CSS-masked element cannot be drawImage'd.
      logoMaskImg = new Image();
      logoMaskImg.src = LOGO_MASK;
    }
    regroup();
    // After regroup, because the band paints with nodeColor() and the sub-shade
    // ladder does not exist until buildColors() has run. Before playTimeline(),
    // so the band grows with the disc instead of appearing fully lit.
    buildHeatmapUI();
    heatBuild();
    buildDateUI();
    fit();                 // frame the FULL disc first, so the camera holds still
    syncSizeScale();       // ...then size the dots to the pitch this window gives us
    // ...then grow the vault from its first note to now. This used to be
    // clearAlpha() + cascade(), which on an empty screen means DRAW mode: the pie
    // arriving clockwise in wedges. The timeline tells the truer story -- the disc
    // densifies in note order, so the shape of the vault's own history is the
    // first thing you see -- and it means load, Refresh and Play are one
    // animation instead of two that were easy to mistake for a bug.
    // UNDER ?demo THE INTRO DOES NOT PLAY HERE. A recorder cannot start before the page
    // does -- ffmpeg needs Chrome's window to exist and its rect to be readable first,
    // measured at ~2.8s -- so an intro that begins on load is most of the way through by
    // the time the first frame lands. Measured on a take: one second in, the disc was
    // already at 250 of 449 notes.
    //
    // So under ?demo the page comes up AT REST and the storyboard replays the intro
    // itself, through the Refresh button, once the recording is rolling.
    // timelineFrame(true) is the resting full disc: `until` is null by default, so every
    // note is present and the layout is derived once, with no animation.
    // AND A HIDDEN TAB COMES UP AT REST TOO, which is a correctness fix and not a saving.
    //
    // playTimeline() reveals notes one at a time over about five seconds, and the disc is
    // legitimately sparse while it does -- rows hold a fraction of their notes, so the room
    // those rows report is genuinely wide and the channels drawn from it are genuinely wide.
    // That is what an unfinished intro looks like, and it is fine, because it finishes.
    //
    // In a BACKGROUND tab it does not finish. requestAnimationFrame is throttled to roughly a
    // frame a second there, and the stall watchdog that would force the cascade to complete is
    // itself driven by rAF, so it never fires either. The tab is left parked on an early frame
    // for as long as it stays hidden -- and switching to it does not restart anything, so it
    // just sits there. Reported as "load three vaults and only the visible one renders
    // correctly", with gaps several times the row spacing in the other two.
    //
    // Nothing is lost by skipping it: the intro is an arrival animation for a person watching,
    // and nobody is watching a hidden tab. If the page is revealed later it is already at rest,
    // which is where the intro was going to leave it.
    var hidden = DOC
      ? (typeof DOC.visibilityState === "string"
          ? DOC.visibilityState === "hidden" : !!DOC.hidden)
      : false;
    // DEFERRED, NOT CANCELLED. A tab that starts hidden comes up at rest so it is never parked
    // on an unfinished intro -- and it remembers that it owes one, so the animation plays the
    // first time the tab is actually looked at. Coming up at rest and leaving it there was the
    // first fix, and it swapped one complaint for another: three vaults opened at once, and the
    // two in the background had simply skipped their intro for good.
    if (hidden && !demoOn() && !restOn()) introOwed = true;
    if (demoOn() || restOn() || hidden) {
      timelineFrame(true);
      // No log. scripts/demo.mjs prints every beat to the terminal as it drives them, so
      // this said the same thing twice -- and console noise is a guideline the linter
      // enforces.
    } else {
      playTimeline();
    }
    $("busy").hidden = true;
  }, 20);
  // A LIVE HANDLE, not the API itself.
  //
  // Init runs inside a setTimeout so the browser can paint "Laying out graph..." first, so
  // API does not exist yet when this returns. Returning it directly returned null -- and
  // null is indistinguishable from "no api" to every `if (this.api)` guard on the plugin
  // side, which is how the theme repaint, the renderer teardown and the diagnostics all
  // became silent no-ops at once. Measured: the canvas held #e8e7e1 while its own CSS token
  // said #333330, and a manual readTheme() + refresh() fixed it instantly.
  //
  // A getter cannot go stale, and it cannot be mistaken for a value that never arrived.
  return { get api() { return API; }, get ready() { return API !== null; } };
}

export { mountVaultGraph };
