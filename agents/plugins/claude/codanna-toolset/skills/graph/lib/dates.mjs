// Per-file dates for the timeline and heatmap. `blameDates` gives an author day per
// LINE (`git blame -M` over the working tree, so line numbers match what the index
// saw); the adapter dates each symbol by the oldest surviving line of its span.
// `firstCommitDates` is the cheap fallback: the day the file first appeared in git
// history (one `git log` pass, newest-first, so the LAST sighting of a path is its
// oldest add). `touched` is the file's mtime day, which is what "mark today" asks.
import fs from "node:fs";
import path from "node:path";
import { createHash } from "node:crypto";
import { spawn, spawnSync } from "node:child_process";

const p2 = (n) => String(n).padStart(2, "0");
export const localDay = (d) => `${d.getFullYear()}-${p2(d.getMonth() + 1)}-${p2(d.getDate())}`;

export function firstCommitDates(workingDir) {
  const r = spawnSync("git", ["-C", workingDir, "log", "--diff-filter=A", "--name-only", "--format=@@%as"],
                      { encoding: "utf8", maxBuffer: 1 << 28 });
  if (r.status !== 0) return null;
  const out = new Map();
  let day = "";
  for (const line of r.stdout.split("\n")) {
    if (!line) continue;
    if (line.startsWith("@@")) { day = line.slice(2); continue; }
    out.set(line, day);   // overwritten by older sightings: last write = oldest add
  }
  return out;
}

// file -> [day per line] from `git blame -M --line-porcelain` on the working tree,
// cached as JSON keyed by content hash: blame(file) only changes when the file's
// content does (append-only history assumed -- a rebase that keeps a file byte-equal
// leaves stale days; deleting the cache file is the reset). A warm run spawns no
// blame at all. Per-file blame failure (untracked file) means undated, not abort;
// only "not a git repo" aborts to the caller's fallback.
export async function blameDates(workingDir, files, cachePath, concurrency = 8) {
  const gate = spawnSync("git", ["-C", workingDir, "rev-parse", "--git-dir"], { encoding: "utf8" });
  if (gate.status !== 0) return null;
  let cache = { v: 1, files: {} };
  try {
    const c = JSON.parse(fs.readFileSync(cachePath, "utf8"));
    if (c && c.v === 1 && c.files) cache = c;
  } catch { /* cold cache */ }
  const days = new Map();
  const queue = [];
  for (const f of files) {
    let content;
    try { content = fs.readFileSync(absolute(workingDir, f)); } catch { continue; }   // gone: undated
    const key = createHash("sha1").update(content).digest("hex");
    const hit = cache.files[f];
    if (hit && hit.k === key) days.set(f, rleExpand(hit.d));
    else queue.push([f, key]);
  }
  const worker = async () => {
    while (queue.length) {
      const [f, key] = queue.pop();
      const d = await blameFile(workingDir, f);
      if (!d) continue;                       // untracked or unreadable: undated
      days.set(f, d);
      cache.files[f] = { k: key, d: rleCompress(d) };
    }
  };
  await Promise.all(Array.from({ length: Math.max(1, Math.min(concurrency, queue.length)) }, worker));
  try {
    fs.mkdirSync(path.dirname(cachePath), { recursive: true });
    const tmp = cachePath + ".tmp";
    fs.writeFileSync(tmp, JSON.stringify(cache));
    fs.renameSync(tmp, cachePath);            // stage-and-rename, never rewrite in place
  } catch { /* cache is an accelerator; failing to write one is not an error */ }
  return days;
}

function blameFile(workingDir, file) {
  return new Promise((done) => {
    const p = spawn("git", ["-C", workingDir, "blame", "-M", "--line-porcelain", "--", file]);
    const chunks = [];
    p.stdout.on("data", (c) => chunks.push(c));
    p.on("error", () => done(null));
    p.on("close", (code) => {
      if (code !== 0) return done(null);
      const out = [];
      let t = 0, tz = 0;
      for (const line of Buffer.concat(chunks).toString("utf8").split("\n")) {
        if (line.startsWith("author-time ")) t = Number(line.slice(12));
        else if (line.startsWith("author-tz ")) {
          const z = line.slice(10);
          tz = (z[0] === "-" ? -1 : 1) * (Number(z.slice(1, 3)) * 3600 + Number(z.slice(3, 5)) * 60);
        } else if (line[0] === "\t") {
          // the author's own calendar day, like `git log --format=%as`
          const d = new Date((t + tz) * 1000);
          out.push(`${d.getUTCFullYear()}-${p2(d.getUTCMonth() + 1)}-${p2(d.getUTCDate())}`);
        }
      }
      done(out);
    });
  });
}

const rleCompress = (d) => { const out = []; for (const day of d) { const last = out[out.length - 1]; if (last && last[0] === day) last[1]++; else out.push([day, 1]); } return out; };
const rleExpand = (r) => { const out = []; for (const [day, n] of r) for (let i = 0; i < n; i++) out.push(day); return out; };

export function touchedDay(absPath) {
  try { return localDay(fs.statSync(absPath).mtime); } catch { return ""; }
}

export const absolute = (workingDir, file) => (path.isAbsolute(file) ? file : path.join(workingDir, file));
