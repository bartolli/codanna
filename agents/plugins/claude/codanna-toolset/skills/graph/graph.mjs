#!/usr/bin/env node
/**
 * graph.mjs -- the codanna index as one self-contained disc: wedges per module, hubs at
 * the centre, one HTML file. Renders through the vault-graph template (MIT, see
 * vendor/LICENSE-vault-graph and UPSTREAM.md) fed by `codanna dump` instead of a vault.
 *
 * Usage: node graph.mjs [--group module|language|kind[/module|kind|visibility]]
 *                       [--root PREFIX] [--kinds a,b] [--relation calls,uses]
 *                       [--dates git|none] [--from graph.jsonl] [--binary PATH]
 *                       [--unlinked include|drop] [--palette auto|fixed|generated]
 *                       [--out FILE] [--name NAME] [--light] [--no-open]
 */
import { readFileSync, writeFileSync, mkdirSync, existsSync } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { execSync } from "node:child_process";
import { readDump } from "./lib/dump.mjs";
import { buildData, parseGroup, DEFAULT_KINDS, RELATIONS } from "./lib/adapter.mjs";
import { documentedSlots, generatePalette } from "./lib/palette.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const argv = process.argv.slice(2);
const flag = (n) => argv.includes("--" + n);
const opt = (n, d) => { const i = argv.indexOf("--" + n); return i >= 0 && argv[i + 1] !== undefined ? argv[i + 1] : d; };
if (flag("help") || flag("h")) {
  console.log(readFileSync(fileURLToPath(import.meta.url), "utf8").split("\n").slice(1, 10).join("\n"));
  process.exit(0);
}

const workingDir = process.env.CLAUDE_PROJECT_DIR || process.cwd();
const csv = (v) => String(v).split(",").map((s) => s.trim()).filter(Boolean);
const kinds = opt("kinds") ? csv(opt("kinds")).map((k) => k[0].toUpperCase() + k.slice(1).toLowerCase()) : DEFAULT_KINDS;
const relations = opt("relation") ? csv(opt("relation")).map((r) => r.toLowerCase()) : ["calls"];
for (const r of relations) {
  if (!RELATIONS.includes(r)) { console.error(`unknown relation '${r}'; one of ${RELATIONS.join(", ")}`); process.exit(2); }
}
const dates = opt("dates", "git");
if (dates !== "git" && dates !== "none") { console.error("--dates takes git|none"); process.exit(2); }
const unlinked = opt("unlinked", "include");
if (unlinked !== "include" && unlinked !== "drop") { console.error("--unlinked takes include|drop"); process.exit(2); }
let group;
try { group = parseGroup(opt("group", "module")); } catch (e) { console.error(e.message); process.exit(2); }
const palette = opt("palette", "auto");
if (!["auto", "fixed", "generated"].includes(palette)) { console.error("--palette takes auto|fixed|generated"); process.exit(2); }

let dump;
try { dump = readDump({ from: opt("from"), binary: opt("binary", "codanna"), workingDir }); }
catch (e) { console.error(e.message); process.exit(1); }
const data = buildData(dump, { kinds, relations, root: opt("root"), dates, unlinked, group, workingDir, name: opt("name") });

const LIB_NOTICE = `<!--
  Rendered by the codanna graph skill through the vault-graph template
    vault-graph (c) Lukas Proprentner, MIT -- https://github.com/luke321/vault-graph
  which inlines two MIT-licensed libraries:
    graphology  (c) Guillaume Plique and graphology contributors
                https://github.com/graphology/graphology
    Sigma.js    (c) Alexis Jacomy, Guillaume Plique and Sigma.js contributors
                https://github.com/jacomyal/sigma.js
  Full licence texts: vendor/ in the skill directory.
-->`;
const libs = LIB_NOTICE + "\n" + ["graphology.umd.min.js", "sigma.min.js"]
  .map((f) => `<script>\n${readFileSync(join(HERE, "vendor", f), "utf8")}\n</script>`).join("\n");
const mask = (() => {
  try {
    const svg = readFileSync(join(HERE, "assets", "logo-mask.svg"), "utf8");
    return "data:image/svg+xml;base64," + Buffer.from(svg).toString("base64");
  } catch { return ""; }
})();
let html = readFileSync(join(HERE, "template.html"), "utf8");

// Wedge colours: the template has ten documented hue slots and falls back to three
// neutrals past them. An index usually has more top-level modules than that, so by
// default a palette sized to the group count is generated in the same family
// (--palette fixed keeps the upstream ten; generated forces one even for few groups).
const groups = new Set(data.nodes.filter((n) => n.deg > 0).map((n) => n.folder));
if (data.stats.orphans && unlinked !== "drop") groups.add("(unlinked)");
const slots = documentedSlots(html);
const wantGenerated = palette === "generated" || (palette === "auto" && groups.size > 10);
const generated = wantGenerated
  ? { dark: generatePalette(groups.size, slots.dark), light: generatePalette(groups.size, slots.light) }
  : null;
data.codanna.palette = generated ? `generated (${groups.size} groups)` : "documented slots";

const assets = `<script>window.VAULT_LOGO_MASK=${JSON.stringify(mask)};</script>` +
  (generated ? `\n<script>window.VAULT_PALETTE=${JSON.stringify(generated)};</script>` : "");

if (flag("light")) html = html.replace('<html lang="en" data-theme="dark">', '<html lang="en" data-theme="light">');
html = html
  .replace("<!--LIBS-->", () => libs)
  .replace("<!--ASSETS-->", () => assets)
  .replace("<!--DATA-->", () => `<script>window.VAULT_DATA=${JSON.stringify(data)};</script>`);

const stamp = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
const OUT = opt("out") ? resolve(process.cwd(), opt("out")) : join(workingDir, ".codanna", "visualizations", `graph-disc-${stamp}.html`);
if (!existsSync(dirname(OUT))) mkdirSync(dirname(OUT), { recursive: true });
writeFileSync(OUT, html, "utf8");

const s = data.stats, c = data.codanna;
console.log(`codanna-graph: ${s.nodes} symbols (of ${c.symbolsTotal}), ${s.edges} edges [${c.relations.join(",")}], ` +
            (s.unlinkedDropped ? `${s.unlinkedDropped} unlinked dropped` : `${s.orphans} unlinked`) +
            `, ${s.files} files, ${groups.size} groups by ${c.group} (${c.palette})` + (c.root ? `, root ${c.root}` : ""));
console.log(`wrote ${OUT} (${(Buffer.byteLength(html) / 1024).toFixed(0)} KB)`);
// Measured: the disc stays smooth to a few thousand symbols; at ~10,000 the hover ramp
// re-runs the reducers over every node per frame and drops to ~10 fps.
if (s.nodes > 4000) console.log(`note: ${s.nodes} symbols -- hover and filters get sluggish above ~4000; scope with --root PREFIX or narrow --kinds`);
if (!flag("no-open")) {
  const opener = process.platform === "darwin" ? "open" : process.platform === "win32" ? "start" : "xdg-open";
  try { execSync(`${opener} "${OUT}"`, { stdio: "ignore" }); } catch { /* no browser, fine */ }
}
