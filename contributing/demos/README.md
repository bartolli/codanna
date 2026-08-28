# Committed demo scenes

Each scene regenerates one README asset: `node contributing/demos/<name>.mjs` writes
`assets/readme/<name>.webp`, or exits non-zero naming the failing step and writes
nothing. A stale demo is therefore a failing command, never a quiet lie.

The driver library is the `record-demo` skill (authored in the skills workspace,
shipped in the `codanna-dev` plugin). Scenes resolve it from `RECORD_DEMO_SKILL` or the
dev link at `~/.claude/skills/record-demo`; `skills link record-demo` creates the link.

Fixtures are declared per scene, never ambient: a scene that reads this repo's
self-index says so and fails when the index is missing. Recording rules (hit-tested
input, in-page cursor, oracle settling, format budgets) live in the skill's SKILL.md.
