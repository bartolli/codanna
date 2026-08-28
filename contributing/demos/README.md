# Demo scenes

Each scene regenerates one README asset: `node contributing/demos/<name>.mjs` writes `assets/readme/<name>.webp`, or exits non-zero naming the failing step and writes nothing.

| Asset | Scene |
|---|---|
| `assets/readme/disc-tour.webp` | `disc-tour.mjs` |
| `assets/readme/hero.webp` | `hero.mjs` (`--list` prints the storyboard without launching Chrome) |
| `assets/readme/xray-tree.webp` | `xray-tree.mjs` |

Prerequisites: `codanna` on PATH with this repo indexed, Google Chrome (a throwaway instance is spawned per take), and `img2webp` (`brew install webp`).

The driver library is the `record-demo` skill (codanna-dev plugin, not released yet), resolved from `RECORD_DEMO_SKILL` or `~/.claude/skills/record-demo`.

Fixtures are declared per scene. Reads the repo's self-index, and fails when the index is missing. 
Recording rules (hit-tested input, in-page cursor, oracle settling, format budgets) live in the skill's SKILL.md.
