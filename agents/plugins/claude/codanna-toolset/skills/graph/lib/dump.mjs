// Reader for `codanna dump` (JSON Lines envelope stream: begin / result* / summary).
// Returns symbols (id -> record), the relationship rows, and the summary data.
import fs from "node:fs";
import { execSync } from "node:child_process";

export function readDump({ from, binary = "codanna", workingDir = process.cwd() }) {
  let text;
  if (from) {
    text = fs.readFileSync(from, "utf8");
  } else {
    try {
      text = execSync(`${binary} dump`, { cwd: workingDir, encoding: "utf8", maxBuffer: 1 << 30, stdio: ["pipe", "pipe", "pipe"] });
    } catch (e) {
      const err = (e.stderr || e.message || "").toString().trim().split("\n").slice(0, 3).join(" ");
      throw new Error(`\`${binary} dump\` failed in ${workingDir}: ${err}\n` +
        "codanna dump needs codanna >= 0.14; point --binary at one, or run codanna index first.");
    }
  }
  const symbols = new Map();
  const relationships = [];
  let summary = null;
  for (const line of text.split("\n")) {
    if (!line) continue;
    const env = JSON.parse(line);
    if (env.type === "summary") { summary = env.data; continue; }
    if (env.type !== "result") continue;
    const d = env.data;
    const entity = env.meta && env.meta.entity_type;
    if (entity === "symbol") {
      symbols.set(d.id, {
        id: d.id,
        name: d.name,
        kind: d.kind,
        file: d.file_path || "",
        line: (d.range && d.range.start_line) || 0,
        endLine: (d.range && d.range.end_line) || 0,
        module: d.module_path || "",
        signature: d.signature || "",
        visibility: d.visibility || "",
        language: d.language_id || "",
      });
    } else if (entity === "relationship") {
      relationships.push({ relation: d.relation, from: d.from.id, to: d.to.id, fromKind: d.from.kind });
    }
  }
  if (!summary) throw new Error("dump stream has no summary line (truncated?)");
  return { symbols, relationships, summary };
}
