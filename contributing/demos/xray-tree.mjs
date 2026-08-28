#!/usr/bin/env node
// Scene: the x-ray collapsible tree (crate::indexing) -- expand two levels into the
// structure, open a symbol's detail panel (themed signature, lit root path), hop a
// relation row with the crumb trail on camera, Backspace back, Escape, reset.
// Regenerates assets/readme/xray-tree.webp.
//
// Fixture: this repo's self-index dumped once to a temp workspace, rendered through
// the committed x-ray skill. Requires `codanna` on PATH and an indexed workspace here.

import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, mkdirSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const SKILL = process.env.RECORD_DEMO_SKILL || join(homedir(), ".claude", "skills", "record-demo");
if (!existsSync(join(SKILL, "lib", "record.mjs"))) {
  console.error("record-demo skill not found at " + SKILL + " -- run `skills link record-demo` or set RECORD_DEMO_SKILL");
  process.exit(1);
}
const { launch, settle, XRAY, cursor, click, key, screencast } =
  await import(pathToFileURL(join(SKILL, "lib", "record.mjs")).href);

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const OUT = join(REPO, "assets", "readme", "xray-tree.webp");
const TREE = join(REPO, "agents", "plugins", "claude", "codanna-toolset", "skills", "x-ray", "visualize-tree.js");

const work = mkdtempSync(join(tmpdir(), "xray-tree-"));
const fail = (msg) => {
  console.error(msg);
  rmSync(work, { recursive: true, force: true });
  process.exit(1);
};

// One dump from the self-index; the page builds from it in the temp workspace so
// nothing writes into the repo during a take.
const dump = spawnSync("codanna", ["dump"], { cwd: REPO, encoding: "utf8", maxBuffer: 1 << 30 });
if (dump.status !== 0 || !dump.stdout) fail("codanna dump failed:\n" + String(dump.stderr).slice(0, 500));
const dumpPath = join(work, "dump.jsonl");
writeFileSync(dumpPath, dump.stdout);
const build = spawnSync("node", [TREE, "--root", "crate::indexing::pipeline", "--from", dumpPath, "--no-open"],
  { cwd: work, env: { ...process.env, CLAUDE_PROJECT_DIR: work }, stdio: ["ignore", "pipe", "pipe"] });
const vizDir = join(work, ".codanna", "visualizations");
const page_html = existsSync(vizDir)
  ? readdirSync(vizDir).filter((f) => f.startsWith("graph-tree-")).map((f) => join(vizDir, f))[0]
  : null;
if (build.status !== 0 || !page_html) fail("tree build failed:\n" + String(build.stderr).slice(0, 500));

// Candidate pickers run in the page. Every pick is filtered to the live viewport --
// the click that follows is hit-tested, so an off-screen choice must be rejected
// here, not discovered as a dead take.
const PICK_EXPAND = `(function(){
  var old = document.querySelector('[data-rec-target]');
  if (old) old.removeAttribute('data-rec-target');
  var gs = document.querySelectorAll('g[cursor="pointer"] > g');
  // Moderate subtrees read on camera; a max-children pick opens a wall of
  // labels. Score peaks near 15 children inside [6..24], falls off outside.
  var best = null, bestS = -1;
  for (var i = 0; i < gs.length; i++) {
    var d = gs[i].__data__;
    if (!d || !d._children || d.children) continue;
    var r = gs[i].querySelector('circle').getBoundingClientRect();
    if (r.left < 30 || r.right > window.innerWidth - 240 || r.top < 60 || r.bottom > window.innerHeight - 30) continue;
    var n = d._children.length;
    var s = (n >= 6 && n <= 24) ? 1000 - Math.abs(15 - n) : n;
    if (s > bestS) { bestS = s; best = gs[i]; }
  }
  if (!best) return null;
  best.setAttribute('data-rec-target', '1');
  return best.__data__.data.name; })()`;

const PICK_LEAF = `(function(){
  var old = document.querySelector('[data-rec-target]');
  if (old) old.removeAttribute('data-rec-target');
  var gs = document.querySelectorAll('g[cursor="pointer"] > g');
  var best = null, score = -1;
  for (var i = 0; i < gs.length; i++) {
    var d = gs[i].__data__;
    if (!d || d.children || d._children || d.data.sid == null) continue;
    var r = gs[i].querySelector('circle').getBoundingClientRect();
    if (r.left < 30 || r.right > window.innerWidth - 260 || r.top < 60 || r.bottom > window.innerHeight - 30) continue;
    var det = (typeof DETAILS !== 'undefined' && DETAILS[d.data.sid]) || null;
    if (!det) continue;
    var rels = det.rels || {}, refs = 0;
    Object.keys(rels).forEach(function(k){ (rels[k]||[]).forEach(function(it){ if (it.ref != null) refs++; }); });
    if (!refs) continue;
    var s = refs + (String(det.sig||'').indexOf(String.fromCharCode(10)) >= 0 ? 100 : 0);
    if (s > score) { score = s; best = gs[i]; }
  }
  if (!best) return null;
  best.setAttribute('data-rec-target', '1');
  return best.__data__.data.name; })()`;

const PICK_GO = `(function(){
  var old = document.querySelector('#detail [data-rec-target]');
  if (old) old.removeAttribute('data-rec-target');
  var btns = document.querySelectorAll('#detail [data-go]');
  // A target already on screen hops with a gentle pan; an unexpanded one
  // relayouts the whole tree under the panel swap and reads as a glitch.
  var rendered = {};
  var gs = document.querySelectorAll('g[cursor="pointer"] > g');
  for (var i = 0; i < gs.length; i++) {
    var d = gs[i].__data__;
    if (d && d.data.sid != null) rendered[d.data.sid] = true;
  }
  var el = null, fallback = null;
  for (var i = 0; i < btns.length; i++) {
    var ref = btns[i].getAttribute('data-go');
    if (String(ref).charAt(0) === 'n') { fallback = fallback || btns[i]; continue; }
    var sid = +ref;
    if (rendered[sid]) { el = btns[i]; break; }
    if (!fallback && typeof allNodes !== 'undefined' && allNodes.some(function(n){ return n.data.sid === sid; })) fallback = btns[i];
  }
  el = el || fallback;
  if (!el) return null;
  el.setAttribute('data-rec-target', '1');
  return el.textContent; })()`;

const session = await launch({ url: pathToFileURL(page_html).href, port: 9367, width: 1280, height: 800 });
const { page } = session;
try {
  await settle(page, XRAY);                       // ready + the initial layout at rest
  const move = await cursor(page);
  const cast = screencast(page, { maxFps: 20 });
  await move(640, 400);
  await cast.start();
  await sleep(700);

  // Two expansions, largest visible collapsed subtree each time.
  let at = { x: 640, y: 400 };
  for (let e = 0; e < 2; e++) {
    const name = await page.eval(PICK_EXPAND);
    if (!name) throw new Error("expand " + (e + 1) + ": no visible collapsed node");
    at = await click(page, '[data-rec-target]', { moveCursor: move, from: at, steps: 18, gap: 24 });
    await settle(page, XRAY);
    await sleep(900);
  }

  // A symbol leaf with go-able relations; the panel opens with the themed
  // signature and the root-to-node path lit.
  const leaf = await page.eval(PICK_LEAF);
  if (!leaf) throw new Error("leaf: no visible symbol with go-able relations");
  at = await click(page, '[data-rec-target]', { moveCursor: move, from: at, steps: 24, gap: 28 });
  await settle(page, XRAY);
  await sleep(2500);

  // Hop a relation row: the camera pans to the target, a crumb appears.
  const hop = await page.eval(PICK_GO);
  if (!hop) throw new Error("hop: no in-tree [data-go] row in the panel");
  at = await click(page, '#detail [data-rec-target]', { moveCursor: move, from: at, steps: 26, gap: 30 });
  await settle(page, XRAY);
  await sleep(2100);

  // Back along the trail, close the panel, home the camera.
  await key(page, "Backspace");
  await settle(page, XRAY);
  await sleep(1100);
  await key(page, "Escape");
  await settle(page, XRAY);
  await sleep(600);
  at = await click(page, "#reset-btn", { moveCursor: move, from: at, steps: 20, gap: 24 });
  await settle(page, XRAY);
  await sleep(900);
  await cast.stop();

  mkdirSync(dirname(OUT), { recursive: true });
  const r = cast.assemble(OUT, { q: 60, budget: 6 * 1024 * 1024 });
  console.log("wrote " + OUT + " (" + Math.round(r.size / 1024) + "KB, " + r.frames + " frames)");
} finally {
  await session.close();
  rmSync(work, { recursive: true, force: true });
}
