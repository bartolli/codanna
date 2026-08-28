#!/usr/bin/env node
// Scene: the README hero -- the indexing engine's disc (facade, pipeline, walker as
// wedges) growing from its first symbol, a hover sweep that lights call webs, then
// the working-set arc: search to a hub, walk its call graph by card hops, and collect
// three symbols into the hub (fit wide, pin, fly to the next) so the loop ends on the
// disc holding the viewer's working set. Regenerates assets/readme/hero.webp through
// the continuous screencast capture (motion as motion, per-frame capture timestamps).
//
// Fixture: this repo's self-index scoped to crate::indexing, built through the
// committed graph skill. Requires `codanna` on PATH and an indexed workspace here.
//
// Every beat settles by the disc's own busy oracle before the next one runs -- no
// fixed inter-beat sleeps. A `hold` on a beat is presentation time (how long the
// viewer gets to read what just landed), not settling, and is declared per beat.
//
// `--list` prints the storyboard and runs the vocabulary guard without launching
// Chrome or writing anything.

import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, mkdirSync, rmSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const OUT = join(REPO, "assets", "readme", "hero.webp");
const GRAPH = join(REPO, "agents", "plugins", "claude", "codanna-toolset", "skills", "graph", "graph.mjs");

/* --- the storyboard ------------------------------------------------------- */
// Beats are data. Verbs the player understands:
//   {settle}                     wait for the disc's busy oracle alone
//   {click: sel}                 hit-tested click on a selector
//   {clicknode}                  click the picked hub symbol on the stage (card oracle)
//   {hovernodes: n}              glide across n hub symbols; call webs light and fade
//   {search: name}               type into search, click the first hit; the card opens
//   {hop}                        click a neighbour in the card; the trail grows a crumb
//   {pin}                        the card's Pin-to-hub; the symbol flies into the hub
//   {zoom: n}                    n clicks on the zoom-in control, a stepped push
//   {hoverpins}                  glide across the pinned hub symbols; webs light
//   {key: k}                     one key through the browser's input path
//   {drag: [kind, arg, dx, dy]}  press a ribbon handle, glide it, release
//   {park}                       pointer to a quiet corner, cursor hidden
// `why` is the caption: it names what the viewer is watching, in codanna vocabulary.
const STORYBOARD = [
  /* --- 1. the codebase, growing (the cast rolls before the intro finishes,
     so the growth plays ON camera exactly once) ---------------------------- */
  { act: "intro", why: "the indexing engine grows from its first symbol to today; the range end sweeps with it", settle: true, hold: 900 },

  /* --- 2. the hover web --------------------------------------------------- */
  { act: "hoverweb", why: "glide across two hub symbols in different modules; each call web lights and fades", hovernodes: 2 },

  /* --- 3. navigate by relationships ---------------------------------------
     The camera's own select zoom lands on sparse rim space at whole-index
     scale, so every read happens back at fit with the card open; the zoomed
     moment is only the flight. */
  { act: "navigate", why: "search a resolver hub by name; the camera flies and the card opens", search: "resolve_one", hold: 800 },
  { act: "navigate", why: "fit the whole codebase with the card open -- signature, neighbours by relation", click: "#vg-reset", hold: 1700 },

  /* --- 4. collect a working set into the hub ------------------------------ */
  { act: "collect", why: "pin the symbol; it flies across the disc into the hub", pin: true, hold: 1400 },
  { act: "collect", why: "hop to a callee on the call graph; the trail grows a crumb", hop: true, hold: 800 },
  { act: "collect", why: "back out to the whole disc, card still open", click: "#vg-reset", hold: 1300 },
  { act: "collect", why: "pin the second symbol into the hub", pin: true, hold: 1400 },
  { act: "collect", why: "one more hop along the call graph", hop: true, hold: 800 },
  { act: "collect", why: "fit the disc", click: "#vg-reset", hold: 1200 },
  { act: "collect", why: "pin the third symbol; the hub now holds the working set", pin: true, hold: 1500 },

  /* --- 5. the payoff ------------------------------------------------------- */
  { act: "payoff", why: "close the card; three symbols stay seated in the hub", key: "Escape", hold: 400 },
  { act: "payoff", why: "push into the hub", zoom: 8, hold: 500 },
  { act: "payoff", why: "sweep the working set; each symbol's call web lights with its name in the tip", hoverpins: true, hold: 2000 },
];

// The take speaks codanna -- symbols, calls, modules, commits. A vault word in a
// caption is an inherited beat that slipped through the vocabulary pass.
const VAULT_VOCAB = /\b(vault|note|notes|folder|folders|daily|weekly|obsidian)\b/i;
function checkVocabulary(beats) {
  for (const b of beats) {
    const hit = VAULT_VOCAB.exec(b.why || "");
    if (hit) throw new Error('vocabulary: beat "' + b.why + '" (act ' + b.act + ') says "' + hit[0] + '"');
  }
}

checkVocabulary(STORYBOARD);
if (process.argv.includes("--list")) {
  for (const b of STORYBOARD) console.log(b.act.padEnd(9) + " " + b.why);
  console.log(STORYBOARD.length + " beats, vocabulary clean");
  process.exit(0);
}

/* --- the player ------------------------------------------------------------ */

const SKILL = process.env.RECORD_DEMO_SKILL || join(homedir(), ".claude", "skills", "record-demo");
if (!existsSync(join(SKILL, "lib", "record.mjs"))) {
  console.error("record-demo skill not found at " + SKILL + " -- run `skills link record-demo` or set RECORD_DEMO_SKILL");
  process.exit(1);
}
const { launch, settle, DISC, cursor, click, glide, drag, key, type, screencast } =
  await import(pathToFileURL(join(SKILL, "lib", "record.mjs")).href);

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
// Per-beat settling is the busy oracle ALONE: DISC.readyExpr wants `until === null`,
// which is exactly false while the timeline act holds a range.
const REST = { busyExpr: DISC.busyExpr };

const work = mkdtempSync(join(tmpdir(), "hero-"));
const page_html = join(work, "disc.html");
// The indexing engine, connected symbols only: the facade, pipeline, walker,
// file_info and progress segments as wedges -- the components that actually drive
// each other -- at a density where every dot reads at README resolution. The
// whole-index disc was rejected on review: 4.9k dots made nodes illegible.
const build = spawnSync("node", [GRAPH, "--root", "crate::indexing", "--no-open", "--unlinked", "drop", "--out", page_html],
  { cwd: REPO, stdio: ["ignore", "pipe", "pipe"] });
if (build.status !== 0 || !existsSync(page_html)) {
  console.error("disc build failed:\n" + build.stderr.toString().slice(0, 500));
  rmSync(work, { recursive: true, force: true });
  process.exit(1);
}

const session = await launch({ url: pathToFileURL(page_html).href, port: 9367, width: 1280, height: 800 });
const { page } = session;

const beatFail = (b, msg) => { throw new Error('beat "' + b.why + '" (' + b.act + "): " + msg); };
let at = { x: 640, y: 430 };
const seen = [];
let crumbs = 0;

async function play(b, move) {
  console.log("  " + b.act.padEnd(9) + " " + b.why);

  if (b.click) at = await click(page, b.click, { moveCursor: move, from: at });

  if (b.hovernodes) {
    // The top hub per module, distinct modules, like the tour's hover sweep.
    const pts = await page.eval(`(function(){
      var g = __vg.graph, r = __vg.renderer, best = {};
      g.forEachNode(function(n){
        var f = g.getNodeAttribute(n, "folder"), d = g.degree(n);
        if (f === "(unlinked)") return;
        if (!best[f] || d > best[f].d) best[f] = { n: n, d: d };
      });
      var box = document.querySelector("#vg-canvas").getBoundingClientRect();
      var out = [];
      Object.keys(best).slice(0, ${b.hovernodes}).forEach(function(f){
        var p = r.graphToViewport(g.getNodeAttributes(best[f].n));
        out.push({ x: box.left + p.x, y: box.top + p.y });
      });
      return out; })()`);
    if (!pts.length) beatFail(b, "no hub symbols to hover");
    for (const p of pts) {
      await glide(page, at, p, { steps: 22, gap: 40, moveCursor: move });
      at = p;
      await sleep(650);           // presentation: let the web light before moving on
    }
  }

  if (b.search) {
    at = await click(page, "#vg-q", { moveCursor: move, from: at, steps: 16, gap: 24 });
    // Per-character typing with a live input event each key: the hit list filters
    // on camera the way a person sees it.
    for (const ch of b.search) {
      await type(page, ch);
      await page.eval(`document.querySelector("#vg-q").dispatchEvent(new Event("input", {bubbles:true})); void 0`);
      await sleep(70);
    }
    await sleep(500);
    at = await click(page, "#vg-hits button", { moveCursor: move, from: at, steps: 14, gap: 22 });
    await settle(page, {
      readyExpr: `(function(){ var d = document.getElementById("vg-detail");
        return d && !d.hidden && d.textContent.indexOf(${JSON.stringify(b.search)}) >= 0; })()`,
      busyExpr: DISC.busyExpr,
    });
    // The searched symbol joins the visited set, or a later hop walks back to it
    // and the pin toggle UNDOES its pin.
    const sel = await page.eval("__vg.state && __vg.state.selected");
    if (sel) seen.push(sel);
  }

  if (b.pin) {
    at = await click(page, "#vg-detail .pin", { moveCursor: move, from: at, steps: 16, gap: 24 });
    // The button is a toggle; a beat that lands on an already-pinned card would
    // silently unpin it. aria-pressed is the state of record.
    const pressed = await page.eval(`(function(){ var p = document.querySelector('#vg-detail .pin');
      return p ? p.getAttribute('aria-pressed') : null; })()`);
    if (pressed !== "true") beatFail(b, "pin did not stick (aria-pressed=" + pressed + ")");
  }

  if (b.zoom) {
    for (let i = 0; i < b.zoom; i++) {
      at = await click(page, "#vg-zin", { moveCursor: move, from: at, steps: i ? 2 : 14, gap: 20 });
      await sleep(160);
    }
  }

  if (b.hoverpins) {
    const pts = await page.eval(`(function(){
      var ids = (__vg.state && __vg.state.pinned) || [];
      var g = __vg.graph, r = __vg.renderer;
      var box = document.querySelector("#vg-canvas").getBoundingClientRect();
      return ids.filter(function(n){ return g.hasNode(n); }).map(function(n){
        var p = r.graphToViewport(g.getNodeAttributes(n));
        return { x: box.left + p.x, y: box.top + p.y };
      }); })()`);
    if (!pts.length) beatFail(b, "no pinned symbols to sweep");
    for (const p of pts) {
      await glide(page, at, p, { steps: 20, gap: 36, moveCursor: move });
      at = p;
      await sleep(900);
    }
  }

  if (b.clicknode) {
    // The page's own pick: a big, isolated dot on the inner ring, with the id the
    // hit-test must agree on before the press.
    const t = await page.eval(`(function(){
      var w = __vg.demo.where("biginner");
      if (!w) return null;
      return { x: w.x, y: w.y, expect: w.expect,
               label: __vg.graph.getNodeAttribute(w.expect, "label") || "" }; })()`);
    if (!t) beatFail(b, "no inner-ring hub resolvable");
    await glide(page, at, t, { steps: 18, gap: 32, moveCursor: move });
    at = { x: t.x, y: t.y };
    let hovered = null;
    for (let i = 0; i < 15; i++) {
      hovered = await page.eval("__vg.demo.hovered()");
      if (hovered === t.expect) break;
      await sleep(100);
    }
    if (hovered !== t.expect) beatFail(b, "aimed at " + t.label + " but hovering " + (hovered || "nothing"));
    await page.send("Input.dispatchMouseEvent", { type: "mousePressed", x: at.x, y: at.y, button: "left", clickCount: 1 });
    await page.send("Input.dispatchMouseEvent", { type: "mouseReleased", x: at.x, y: at.y, button: "left", clickCount: 1 });
    await settle(page, {
      readyExpr: `(function(){ var d = document.getElementById("vg-detail");
        return d && !d.hidden && d.textContent.indexOf(${JSON.stringify(t.label)}) >= 0; })()`,
      busyExpr: DISC.busyExpr,
    });
  }

  if (b.hop) {
    const pick = await page.eval(`(function(){
      var seen = ${JSON.stringify(seen)};
      var marked = document.querySelector("#vg-detail [data-rec-target]");
      if (marked) marked.removeAttribute("data-rec-target");
      var g = __vg.graph;
      var btns = document.querySelectorAll("#vg-detail [data-go]");
      // Hero picks are API names a reader recognises: short, no test vocabulary.
      var BAD = /test|fails|witness|proof|_closed|_guard/i;
      var el = null, fallback = null, last = null;
      for (var i = 0; i < btns.length; i++) {
        var id = btns[i].getAttribute("data-go");
        if (seen.indexOf(id) >= 0) continue;
        var lb = (btns[i].textContent || "").replace(/\\s*\\u00d7?x?\\d*\\s*$/, "").trim();
        if (!last && lb.length >= 4) last = btns[i];
        if (lb.length < 4 || lb.length > 26 || BAD.test(lb)) continue;
        if (!fallback) fallback = btns[i];
        var sig = g.getNodeAttribute(id, "sig") || "";
        if (sig.indexOf(String.fromCharCode(10)) >= 0) { el = btns[i]; break; }
      }
      el = el || fallback || last;
      if (!el) return null;
      el.setAttribute("data-rec-target", "1");
      return el.getAttribute("data-go"); })()`);
    if (!pick) beatFail(b, "no unvisited neighbour in the card");
    seen.push(pick);
    crumbs++;
    at = await click(page, "#vg-detail [data-rec-target]", { moveCursor: move, from: at, steps: 24, gap: 28 });
    // The trail row holds one [data-tr] per crumb plus the back arrow -- capped at
    // four once the trail collapses to first + ellipsis + last two.
    await settle(page, {
      readyExpr: `document.querySelectorAll('#vg-detail [data-tr]').length === ${crumbs <= 3 ? crumbs + 1 : 4}`,
      busyExpr: DISC.busyExpr,
    });
  }

  if (b.key) {
    await key(page, b.key);
    if (b.key === "Backspace") {
      crumbs--;
      await settle(page, {
        readyExpr: `document.querySelectorAll('#vg-detail [data-tr]').length === ${crumbs === 0 ? 0 : crumbs <= 3 ? crumbs + 1 : 4}`,
        busyExpr: DISC.busyExpr,
      });
    }
    if (b.key === "Escape") {
      await settle(page, {
        readyExpr: `(function(){ var d = document.getElementById("vg-detail"); return !d || d.hidden; })()`,
        busyExpr: DISC.busyExpr,
      });
    }
  }

  if (b.drag) {
    const [kind, arg, dx, dy] = b.drag;
    const h = await page.eval(`__vg.demo.where(${JSON.stringify(kind)}, ${JSON.stringify(arg)})`);
    if (!h) beatFail(b, "target " + kind + " " + arg + " not resolvable");
    await glide(page, at, h, { steps: 14, gap: 28, moveCursor: move });
    const to = { x: h.x + dx, y: h.y + dy };
    await drag(page, h, to, { steps: 20, gap: 28, moveCursor: move });
    at = to;
  }

  if (b.park) {
    const spot = { x: 640, y: 780 };
    await glide(page, at, spot, { steps: 14, gap: 24, moveCursor: move });
    at = spot;
    await page.eval("__vg.demo.cursorHide && __vg.demo.cursorHide(); void 0");
  }

  await settle(page, REST);
  if (b.hold) await sleep(b.hold);
}

try {
  // Roll BEFORE the intro finishes: the disc growing from its first symbol is the
  // opening beat, so the cast starts as soon as the page exists.
  await settle(page, { readyExpr: "!!window.__vg" });
  const move = await cursor(page);
  const cast = screencast(page, { maxFps: 15 });
  await move(at.x, at.y);

  const t0 = Date.now();
  await cast.start();
  for (const b of STORYBOARD) await play(b, move);
  const wall = Date.now() - t0;
  await cast.stop();

  mkdirSync(dirname(OUT), { recursive: true });
  const r = cast.assemble(OUT, { q: 80, budget: 10 * 1024 * 1024 });

  // The take is real time: encoded playback within 5 percent of the driver's wall
  // clock, or no asset.
  const drift = Math.abs(r.duration - wall) / wall;
  if (drift > 0.05) {
    rmSync(OUT);
    throw new Error("duration: take " + r.duration + "ms vs driver " + wall + "ms (" +
      (drift * 100).toFixed(1) + "% drift, 5% allowed); no asset written");
  }
  console.log("wrote " + OUT + " (" + Math.round(r.size / 1024) + "KB, " + r.frames +
    " frames, " + (r.duration / 1000).toFixed(1) + "s vs " + (wall / 1000).toFixed(1) +
    "s wall, " + (drift * 100).toFixed(1) + "% drift)");
} finally {
  await session.close();
  rmSync(work, { recursive: true, force: true });
}
