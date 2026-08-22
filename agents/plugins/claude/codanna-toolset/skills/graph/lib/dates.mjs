// Per-file dates for the timeline and heatmap. `created` is the day the file first
// appeared in git history (one `git log` pass, newest-first, so the LAST sighting of a
// path is its oldest add). `touched` is the file's mtime day, which is what "mark
// today" asks.
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";

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

export function touchedDay(absPath) {
  try { return localDay(fs.statSync(absPath).mtime); } catch { return ""; }
}

export const absolute = (workingDir, file) => (path.isAbsolute(file) ? file : path.join(workingDir, file));
