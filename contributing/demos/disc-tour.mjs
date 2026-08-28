#!/usr/bin/env node
// Scene: a tour of the scoped disc (crate::indexing) -- the intro cascade, a hover
// sweep that lights focus webs, legend eyes (hide unlinked, close and reopen a module
// wedge), search to a symbol with a multi-line signature, two hops, Escape, a year-chip
// range and back, fit. Regenerates assets/readme/disc-tour.webp.
//
// Fixture: this repo's self-index scoped to crate::indexing, built through the
// committed graph skill. Requires `codanna` on PATH and an indexed workspace here.

import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, mkdirSync, rmSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const SKILL = process.env.RECORD_DEMO_SKILL || join(homedir(), ".claude", "skills", "record-demo");
if (!existsSync(join(SKILL, "lib", "record.mjs"))) {
  console.error("record-demo skill not found at " + SKILL + " -- run `skills link record-demo` or set RECORD_DEMO_SKILL");
  process.exit(1);
}
const { launch, settle, DISC, cursor, click, glide, key, type, screencast } =
  await import(pathToFileURL(join(SKILL, "lib", "record.mjs")).href);

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const OUT = join(REPO, "assets", "readme", "disc-tour.webp");
const GRAPH = join(REPO, "agents", "plugins", "claude", "codanna-toolset", "skills", "graph", "graph.mjs");
const FOCUS = "make_context";   // pipeline hub: degree 32, six-line signature

const work = mkdtempSync(join(tmpdir(), "disc-tour-"));
const page_html = join(work, "disc.html");
const build = spawnSync("node", [GRAPH, "--root", "crate::indexing", "--no-open", "--out", page_html],
  { cwd: REPO, stdio: ["ignore", "pipe", "pipe"] });
if (build.status !== 0 || !existsSync(page_html)) {
  console.error("disc build failed:\n" + build.stderr.toString().slice(0, 500));
  rmSync(work, { recursive: true, force: true });
  process.exit(1);
}

const session = await launch({ url: pathToFileURL(page_html).href, port: 9366, width: 1280, height: 800 });
const { page } = session;
try {
  // Roll BEFORE the intro finishes: the disc growing from its first symbol is the
  // opening beat, so the cast starts as soon as the page exists.
  await settle(page, { readyExpr: "!!window.__vg" });
  const move = await cursor(page);
  const cast = screencast(page, { maxFps: 20 });
  await move(640, 430);
  await cast.start();
  await settle(page, DISC);                                 // the intro plays into the cast
  await sleep(600);

  // Hover sweep through three hubs in different modules: focus webs light and fade
  // under a real pointer.
  const pts = await page.eval(`(function(){
    var g = __vg.graph, r = __vg.renderer, best = {};
    g.forEachNode(function(n){
      var f = g.getNodeAttribute(n, "folder"), d = g.degree(n);
      if (f === "(unlinked)") return;
      if (!best[f] || d > best[f].d) best[f] = { n: n, d: d };
    });
    var box = document.querySelector("#vg-canvas").getBoundingClientRect();
    var out = [];
    Object.keys(best).slice(0, 3).forEach(function(f){
      var p = r.graphToViewport(g.getNodeAttributes(best[f].n));
      out.push({ x: box.left + p.x, y: box.top + p.y });
    });
    return out; })()`);
  let at = { x: 640, y: 430 };
  for (const p of pts) {
    await glide(page, at, p, { steps: 22, gap: 40, moveCursor: move });
    at = p;
    await sleep(500);
  }

  // Legend: hide the unlinked group (the rest regrow into a full circle), then close
  // and reopen one module wedge.
  at = await click(page, '#vg-legend .eye[data-eye="(unlinked)"]', { moveCursor: move, from: at });
  await sleep(1600);
  at = await click(page, '#vg-legend .eye[data-eye="facade"]', { moveCursor: move, from: at });
  await sleep(1500);
  at = await click(page, '#vg-legend .eye[data-eye="facade"]', { moveCursor: move, from: at });
  await sleep(1600);

  // Search to the hub with the rich signature; the camera flies, the card opens with
  // the highlighted six-line block. Hold long enough to read it.
  at = await click(page, "#vg-q", { moveCursor: move, from: at });
  await type(page, FOCUS);
  await page.eval(`document.querySelector("#vg-q").dispatchEvent(new Event("input", {bubbles:true})); void 0`);
  await sleep(600);
  at = await click(page, "#vg-hits button", { moveCursor: move, from: at });
  await sleep(2200);

  // Two hops, preferring neighbours that also carry multi-line signatures.
  const seen = [];
  for (let h = 0; h < 2; h++) {
    const pick = await page.eval(`(function(){
      var seen = ${JSON.stringify(seen)};
      var marked = document.querySelector("#vg-detail [data-rec-target]");
      if (marked) marked.removeAttribute("data-rec-target");
      var g = __vg.graph;
      var btns = document.querySelectorAll("#vg-detail [data-go]");
      var el = null, fallback = null;
      for (var i = 0; i < btns.length; i++) {
        var id = btns[i].getAttribute("data-go");
        if (seen.indexOf(id) >= 0) continue;
        if (!fallback) fallback = btns[i];
        var sig = g.getNodeAttribute(id, "sig") || "";
        if (sig.indexOf(String.fromCharCode(10)) >= 0) { el = btns[i]; break; }
      }
      el = el || fallback;
      if (!el) return null;
      el.setAttribute("data-rec-target", "1");
      return el.getAttribute("data-go"); })()`);
    if (!pick) throw new Error("hop " + (h + 1) + ": no [data-go] targets in the panel");
    seen.push(pick);
    at = await click(page, "#vg-detail [data-rec-target]", { moveCursor: move, from: at });
    await sleep(1800);
  }
  // Back to the whole disc while the card is still up, THEN dismiss it -- fit and
  // Escape in the other order left a beat of dead air between two small clicks. The
  // card yields above the camera cluster, so fit is reachable with it open.
  at = await click(page, "#vg-reset", { moveCursor: move, from: at });
  await sleep(1100);
  await key(page, "Escape");
  await sleep(500);

  // The timeline: a year chip slices the disc to that year, All dates regrows it.
  at = await click(page, "#vg-years button", { moveCursor: move, from: at });
  await sleep(1900);
  at = await click(page, "#vg-rangeall", { moveCursor: move, from: at });
  await sleep(1900);

  // Rest, and a breath before the loop restarts.
  await settle(page, DISC);
  await sleep(1000);
  await cast.stop();

  mkdirSync(dirname(OUT), { recursive: true });
  const r = cast.assemble(OUT, { q: 60, budget: 6 * 1024 * 1024 });
  console.log("wrote " + OUT + " (" + Math.round(r.size / 1024) + "KB, " + r.frames + " frames)");
} finally {
  await session.close();
  rmSync(work, { recursive: true, force: true });
}
