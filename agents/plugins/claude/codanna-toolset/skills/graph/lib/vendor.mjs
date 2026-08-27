/**
 * vendor.mjs -- read a vendored library, with its unreachable network paths removed.
 *
 * WHY THIS EXISTS. The Obsidian directory's automated review reads the shipped main.js and
 * reports, under "Disclosures", how many network request calls it contains: "All network
 * requests should be necessary and disclosed to users." Ours said **2 network calls**, and
 * a plugin that draws a picture of the vault has no business making any -- github#1.
 *
 * Both are Sigma.js's `loadSVGImage`, which fetches an SVG so a NODE IMAGE can be drawn
 * from it. Nothing here draws image nodes: the page registers exactly two programs,
 * `EdgeCurveProgram` and `createNodeBorderProgram` (src/page.js, the Sigma constructor).
 * So the calls were unreachable, and shipping them meant asking every user to take on
 * trust that a code path they can see is one we never take.
 *
 * They cannot be tree-shaken away. `vendor/` holds pre-built UMD bundles committed rather
 * than installed -- no package manager, no network at build time -- and a UMD bundle is
 * one opaque expression to any bundler. So the removal is textual, and it happens HERE,
 * once, for both consumers:
 *
 *   src/build-graph.mjs      inlines the libraries into the standalone HTML file
 *   scripts/build-plugin.mjs bundles them into the plugin's main.js
 *
 * `vendor/` itself stays byte-identical to upstream, which is the point of doing it at
 * read time rather than editing the file: the committed bundle can still be diffed against
 * the release it came from. The modification travels with the build instead, and
 * vendor/NOTICE.md records it, as MIT redistribution asks.
 *
 * IT IS NOT ONLY ABOUT THE NETWORK ANY MORE. The second transform below fixes an upstream
 * defect in Sigma's own hover bookkeeping (github#7). Same mechanism, same reasoning: a
 * counted, documented replacement at read time, so `vendor/` can still be diffed against
 * the release it came from and the change travels with the build.
 *
 * THE COUNT IS THE GATE. Each file declares how many calls it is expected to contain, and
 * a mismatch is a hard error rather than a silent strip. An upstream update that adds a
 * network call should stop the build and be read, not be quietly neutered.
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";

/* How many `fetch(` calls each vendored bundle contains, and what they are. Update these
 * in the same commit as the bundle, having read what the new call does. */
export const EXPECTED_FETCHES = {
  // Both inside loadSVGImage(): one with `credentials: "include"`, one without.
  "sigma.min.js": 2,
  "graphology.umd.min.js": 0,
};

// A bare `fetch(` -- not `.fetch(`, which would be a method on somebody's object and none
// of our business. The leading character is captured so it can be put back.
const FETCH_CALL = /(^|[^.\w$])fetch\s*\(/g;

// What replaces it. A thrower rather than a rejected promise, because the throw lands
// inside Sigma's own try/catch (loadImage falls back to a plain `new Image()` for the SVG
// case) and, if the path ever did become reachable, a stack trace naming this file is a
// better outcome than an image that quietly never appears. Written as one self-contained
// expression so no helper has to be injected into either bundle's scope.
const THROWER =
  '(function(){throw new Error("vault-graph: this build makes no network requests")})';

/* ------------------------------------------------------- sigma's leave bug --
 * Sigma forgets which node it thought was hovered when the pointer leaves the container.
 *
 * `handleLeave` emits `leaveNode` and never clears `this.hoveredNode` -- unlike
 * `handleMove`, which does exactly that two lines earlier in the same file. So after the
 * pointer leaves the canvas Sigma still believes it is on that node, and `handleMove`'s
 * re-entry test is `hoveredNode !== nodeAtPosition`, which is now false. Moving back onto
 * THE SAME NOTE therefore emits nothing at all, and the hover never comes back.
 *
 * Measured, on the 450-note vault:
 *
 *   hover a note              enterNode 1, state.hovered "206"
 *   move outside the canvas   leaveNode 1, state.hovered null      <- looks fine
 *   move back onto the note   NO EVENT,   state.hovered null       <- stuck
 *   move to empty canvas      leaveNode 2 (clears it at last)
 *   move back onto the note   enterNode 2, state.hovered "206"
 *
 * That is a real defect for a person -- glance at the sidebar, come back to the note you
 * were reading, no highlight -- and it is also what made the invariant suite flaky
 * (github#7): the hover checks miss whenever anything earlier moved the pointer off the
 * canvas, which is 39 times in 40 when the sequence is repeated. Not the settle, which is
 * where the diagnosis started; the disc had not moved 0.3px.
 *
 * `hoveredEdge` has the identical bug on the line below and is fixed with it, though
 * nothing here enables edge events.
 *
 * The emit stays first and the clear goes after it, so the event payload is unchanged --
 * it carries the node explicitly, and no listener can tell the difference.
 */
const LEAVE_BUG = [
  [
    'this.hoveredNode&&(this.emit("leaveNode",{...i,node:this.hoveredNode}),' +
    "this.scheduleHighlightedNodesRender())",
    'this.hoveredNode&&(this.emit("leaveNode",{...i,node:this.hoveredNode}),' +
    "this.hoveredNode=null,this.scheduleHighlightedNodesRender())",
  ],
  [
    'this.hoveredEdge&&(this.emit("leaveEdge",{...i,edge:this.hoveredEdge}),' +
    "this.scheduleHighlightedNodesRender())",
    'this.hoveredEdge&&(this.emit("leaveEdge",{...i,edge:this.hoveredEdge}),' +
    "this.hoveredEdge=null,this.scheduleHighlightedNodesRender())",
  ],
];

/* Which files carry the leave bug, and how many of its two halves each one has. */
export const EXPECTED_LEAVE_FIXES = {
  "sigma.min.js": 2,
  "graphology.umd.min.js": 0,
};

/* Everything that would make a request. Names, not call shapes, so a check over the
 * shipped bundle catches an assignment or an alias as well as a direct call. */
export const NETWORK_PRIMITIVES = [
  ["fetch(", FETCH_CALL],
  ["XMLHttpRequest", /\bXMLHttpRequest\b/g],
  ["WebSocket", /\bWebSocket\b/g],
  ["EventSource", /\bEventSource\b/g],
  ["sendBeacon", /\bsendBeacon\b/g],
  ["importScripts", /\bimportScripts\b/g],
  // Obsidian's own HTTP helper. Nothing here should reach for it either, and it is the one
  // a reader of the plugin API would think to use.
  ["requestUrl", /\brequestUrl\b/g],
];

/** Every network primitive in `text`, as `[{ name, count }]`. Empty is the good answer. */
export function findNetworkPrimitives(text) {
  const hits = [];
  for (const [name, re] of NETWORK_PRIMITIVES) {
    re.lastIndex = 0;
    const count = (text.match(re) || []).length;
    if (count) hits.push({ name, count });
  }
  return hits;
}

/**
 * Read `vendor/<file>` and hand back its source with the network calls replaced.
 *
 * Throws if the number of calls is not the number declared above, or if anything else
 * that makes a request survives -- either means the bundle changed and a person has to
 * look at it.
 */
export function readVendorSource(root, file) {
  const expected = EXPECTED_FETCHES[file];
  if (expected === undefined) {
    throw new Error(
      "vendor.mjs: no expected-call count declared for vendor/" + file + ".\n" +
      "Add one to EXPECTED_FETCHES after reading what the bundle's network calls do."
    );
  }

  const raw = readFileSync(join(root, "vendor", file), "utf8");
  let found = 0;
  let out = raw.replace(FETCH_CALL, (_m, lead) => { found++; return lead + THROWER + "("; });

  if (found !== expected) {
    throw new Error(
      "vendor.mjs: vendor/" + file + " has " + found + " fetch call(s), expected " +
      expected + ".\n" +
      "The bundle changed. Read what the new call does before touching EXPECTED_FETCHES:\n" +
      "a call that is genuinely needed has to be disclosed to users, not stripped, and a\n" +
      "call that is not needed is one more reason this transform exists."
    );
  }

  // Sigma's leave bug, fixed in the same pass. Counted the same way and for the same
  // reason: a bundle whose shape has moved is a thing to go and read, not to patch blind.
  let fixes = 0;
  for (const [from, to] of LEAVE_BUG) {
    const n = out.split(from).length - 1;
    if (n) { fixes += n; out = out.split(from).join(to); }
  }
  const wantFixes = EXPECTED_LEAVE_FIXES[file] || 0;
  if (fixes !== wantFixes) {
    throw new Error(
      "vendor.mjs: vendor/" + file + " -- applied " + fixes + " of the expected " +
      wantFixes + " hover-leave fix(es).\n" +
      "Either upstream fixed it (drop the transform and the count) or the bundle changed\n" +
      "shape. Check handleLeave: it must clear hoveredNode, not only emit leaveNode.\n" +
      "scripts/smoke.mjs asserts the behaviour, so run that before deciding."
    );
  }

  const left = findNetworkPrimitives(out);
  if (left.length) {
    throw new Error(
      "vendor.mjs: vendor/" + file + " still reaches the network after stripping: " +
      left.map((h) => h.name + " x" + h.count).join(", ") + ".\n" +
      "Only fetch() is handled here. Read the new call and decide deliberately."
    );
  }

  return out;
}
