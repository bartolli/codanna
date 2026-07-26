#!/usr/bin/env python3
"""Corpus battery: controlled edge-dump pairs over fresh clones at pinned commits.

Enforces the measurement protocol invariants that prose could not:
  - fixture pristineness (clean checkout before cloning from it)
  - clone integrity (populated working tree; poisoned partial clones rejected)
  - pin verification (corpus HEAD equals the requested pin)
  - fresh workspace per leg (never reused; stale indexed_paths double-index)
  - verified semantic toggle (written to settings.toml, then read back AND
    confirmed against the index log line)
  - sorted dumps, optional two-run determinism, union sets for scatter corpora
  - diff mechanics (comm order encapsulated; dropped/gained labeled)
  - incremental lane parity (--incremental N): the fresh dump is the oracle;
    re-indexing touched files must not change the edge set. Without this leg
    the battery only ever measured fresh indexes, and an 8.8% inbound-edge
    loss on the incremental lane survived every run.
  - touch shape (--touch-shape, default prepend): the touch SHIFTS ranges, so
    line-sensitive identity paths are exercised. An append-only touch reported
    parity on three corpora while a prepend still dropped edges.
  - binary identity: each leg records the commit its index was stamped with
    and two legs may not share one. `--version` does not separate builds
    between releases, so a pre/post pair over one binary otherwise diffs it
    against itself and reports a confident empty result.

Usage:
  e2e-battery.py run  --binary PATH --label NAME --dump PATH \
                      --corpus NAME=FIXTURE_PATH@PIN [...] \
                      --out DIR [--runs N] [--semantic on|off] [--incremental N]
  e2e-battery.py diff --out DIR --corpus NAME --old LABEL --new LABEL

Pins are REQUIRED per corpus (numbers rot; the caller states them).
No cargo invocations here: binaries are built by the caller.
"""

import argparse
import json
import re
import subprocess
from pathlib import Path


def run(cmd, cwd=None, check=True, capture=True):
    """Absolute-path, checked subprocess. No shell, no cwd persistence."""
    result = subprocess.run(
        cmd, cwd=cwd, capture_output=capture, text=True, check=False
    )
    if check and result.returncode != 0:
        raise SystemExit(
            f"FAIL rc={result.returncode}: {' '.join(map(str, cmd))}\n"
            f"{(result.stderr or result.stdout or '')[-2000:]}"
        )
    return result


def parse_corpus_spec(spec):
    m = re.fullmatch(r"([A-Za-z0-9_.-]+)=(.+)@([0-9a-fA-F]{7,40})", spec)
    if not m:
        raise SystemExit(
            f"bad --corpus spec '{spec}' (want NAME=FIXTURE_PATH@PIN)"
        )
    name, fixture, pin = m.groups()
    return name, Path(fixture).resolve(), pin


def ensure_corpus(out, name, fixture, pin):
    """Clone once per corpus dir; verify integrity and pin every time."""
    corpus = out / name / "corpus"
    if not corpus.exists():
        status = run(
            ["git", "-C", str(fixture), "status", "--porcelain"]
        ).stdout.strip()
        if status:
            raise SystemExit(
                f"{name}: fixture {fixture} is not a clean checkout:\n{status}"
            )
        corpus.parent.mkdir(parents=True, exist_ok=True)
        run(["git", "clone", "-q", "--no-hardlinks", str(fixture), str(corpus)])
        run(["git", "-C", str(corpus), "checkout", "-q", pin])
    # Integrity: a poisoned partial clone (deleted-cwd class) has .git but an
    # unpopulated working tree - rev-parse works, every file reads deleted.
    if run(
        ["git", "-C", str(corpus), "diff", "--quiet"], check=False
    ).returncode != 0:
        raise SystemExit(
            f"{name}: corpus working tree is dirty or unpopulated "
            f"(poisoned partial clone?). Delete {corpus} and rerun."
        )
    head = run(["git", "-C", str(corpus), "rev-parse", "HEAD"]).stdout.strip()
    if not head.startswith(pin):
        raise SystemExit(f"{name}: corpus HEAD {head[:12]} != pin {pin}")
    return corpus


def builder_commit(workspace):
    """Commit stamped into the index by the binary that wrote it.

    `None` for a binary built without a work tree (release tarballs) or an
    index written before the stamp existed. Either way the caller decides
    what an unknown means; this only reports.
    """
    meta = workspace / ".codanna" / "index" / "index.meta"
    if not meta.exists():
        return None
    try:
        return json.loads(meta.read_text()).get("builder_commit")
    except (json.JSONDecodeError, OSError):
        return None


def leg_family(label):
    """`pre.run1` / `pre.run2` are re-runs of ONE leg; sharing a binary
    there is the design. The guard exists for cross-family reuse
    (pre vs post over one build)."""
    return re.sub(r"\.run\d+$", "", label)


def record_leg_binary(out_dir, label, commit):
    """Record this leg's binary identity and reject a re-used binary.

    `--binary` is the one input the protocol cannot verify from the corpus:
    two legs pointed at the same build produce a confident empty diff that
    reads exactly like "the change had no effect". Comparing the stamps
    turns that into an error.
    """
    ledger_path = out_dir / "binaries.json"
    ledger = {}
    if ledger_path.exists():
        try:
            ledger = json.loads(ledger_path.read_text())
        except (json.JSONDecodeError, OSError):
            ledger = {}

    if commit is None:
        print(
            f"   binary {label}: no commit stamp (tarball build, or an index "
            f"written before the stamp existed) - legs unverifiable"
        )
    else:
        clash = [
            other
            for other, seen in ledger.items()
            if seen == commit and leg_family(other) != leg_family(label)
        ]
        if clash:
            raise SystemExit(
                f"{out_dir.name}: leg '{label}' and leg(s) {clash} were built "
                f"from the same binary ({commit}). A pre/post pair over one "
                f"build diffs a binary against itself; rebuild the other leg."
            )
        if commit.endswith("-dirty"):
            print(
                f"   binary {label}: {commit} - built from a modified tree, "
                f"so the commit does not identify the code that ran"
            )
        else:
            print(f"   binary {label}: {commit}")

    ledger[label] = commit
    ledger_path.write_text(json.dumps(ledger, indent=2, sort_keys=True) + "\n")


def set_semantic(workspace, enabled):
    """Flip [semantic_search] enabled in settings.toml and read it back."""
    settings = workspace / ".codanna" / "settings.toml"
    text = settings.read_text()
    value = "true" if enabled else "false"
    new, count = re.subn(
        r"(\[semantic_search\][^\[]*?enabled\s*=\s*)(true|false)",
        rf"\g<1>{value}",
        text,
        count=1,
        flags=re.DOTALL,
    )
    if count != 1:
        raise SystemExit(f"semantic_search.enabled not found in {settings}")
    settings.write_text(new)
    written = re.search(
        r"\[semantic_search\][^\[]*?enabled\s*=\s*(true|false)",
        settings.read_text(),
        flags=re.DOTALL,
    )
    if written is None or written.group(1) != value:
        raise SystemExit(f"semantic toggle did not persist in {settings}")


def verify_semantic_in_log(log_text, want_enabled, context):
    """The index log is the proof the setting took at run time."""
    saw_enabled = "Semantic search enabled" in log_text
    if want_enabled != saw_enabled:
        state = "enabled" if saw_enabled else "disabled"
        raise SystemExit(
            f"{context}: protocol wanted semantic "
            f"{'on' if want_enabled else 'off'} but the index ran {state}. "
            f"A protocol step whose success you did not verify did not happen."
        )


def leg(binary, dump, corpus, out_dir, label, suffix, semantic_on):
    ws = out_dir / f"ws-{label}{suffix}"
    if ws.exists():
        raise SystemExit(f"workspace {ws} already exists; legs never reuse")
    ws.mkdir(parents=True)
    run([str(binary), "init"], cwd=ws)
    set_semantic(ws, semantic_on)
    log = run([str(binary), "index", str(corpus)], cwd=ws)
    log_text = (log.stdout or "") + (log.stderr or "")
    (out_dir / f"index-{label}{suffix}.log").write_text(log_text)
    verify_semantic_in_log(log_text, semantic_on, f"{out_dir.name} {label}{suffix}")
    tantivy = ws / ".codanna" / "index" / "tantivy"
    if not tantivy.is_dir():
        raise SystemExit(f"{out_dir.name} {label}{suffix}: no tantivy dir after index")
    edges = run([str(dump), str(tantivy)]).stdout.splitlines()
    edge_file = out_dir / f"{label}{suffix}.edges"
    edge_file.write_text("\n".join(sorted(edges)) + ("\n" if edges else ""))
    record_leg_binary(out_dir, f"{label}{suffix}", builder_commit(ws))
    run(["rm", "-rf", str(ws)])
    print(f"EDGES {out_dir.name} {label}{suffix}: {len(edges)}")
    return edge_file


def edge_endpoint_file(field):
    """File path out of a dump endpoint `name@/abs/path.go:236/Method`."""
    at = field.rfind("@")
    if at < 0:
        return None
    rest = field[at + 1 :]
    colon = rest.rfind(":")
    return rest[:colon] if colon > 0 else None


def touch_targets(edge_lines, count):
    """Files carrying the most CROSS-FILE inbound edges, most first.

    That population is the one at risk: a file's own inbound edges are
    re-derived by its own re-parse, so only edges owned by OTHER files can
    go missing. Ties break on path so the touch set is reproducible.
    """
    inbound = {}
    for line in edge_lines:
        parts = line.split("\t")
        if len(parts) < 3:
            continue
        src, dst = edge_endpoint_file(parts[1]), edge_endpoint_file(parts[2])
        if not src or not dst or src == dst:
            continue
        inbound[dst] = inbound.get(dst, 0) + 1
    ranked = sorted(inbound.items(), key=lambda kv: (-kv[1], kv[0]))
    return [Path(p) for p, _ in ranked[:count]]


def strip_lines(edge_line):
    """Edge identity with every line number removed.

    Dump identity is `name@file:line/kind` and the call line is its own
    column, so a touch that SHIFTS ranges relabels every row of the touched
    files even when the edge set is unchanged. Comparing shifted dumps
    verbatim reports that relabeling as a wall of drops and gains. Line-free
    identity is the only comparison that isolates real movement under a
    shifting edit.
    """
    parts = edge_line.split("\t")
    if len(parts) < 3:
        return edge_line
    endpoints = [re.sub(r":\d+/", "/", p) for p in parts[1:3]]
    # parts[3] is the call line; it shifts with the symbol.
    tail = parts[4:]
    return "\t".join([parts[0], *endpoints, *tail])


def incremental_leg(
    binary, dump, corpus, out_dir, label, semantic_on, touch_n, touch_shape
):
    """Fresh index, then touch N files and re-index WITHOUT force.

    The fresh dump is the oracle: re-indexing files whose content is
    semantically unchanged must leave the edge set identical. Any diff is
    the finding. This is the leg that makes load-bearing invariant 12
    ("the lanes must agree") checkable at corpus scale.
    """
    ws = out_dir / f"ws-{label}.incr"
    if ws.exists():
        raise SystemExit(f"workspace {ws} already exists; legs never reuse")
    ws.mkdir(parents=True)
    run([str(binary), "init"], cwd=ws)
    set_semantic(ws, semantic_on)

    log = run([str(binary), "index", str(corpus)], cwd=ws)
    log_text = (log.stdout or "") + (log.stderr or "")
    verify_semantic_in_log(log_text, semantic_on, f"{out_dir.name} {label}.incr fresh")
    tantivy = ws / ".codanna" / "index" / "tantivy"
    if not tantivy.is_dir():
        raise SystemExit(f"{out_dir.name} {label}.incr: no tantivy dir after index")
    fresh = sorted(run([str(dump), str(tantivy)]).stdout.splitlines())
    fresh_file = out_dir / f"{label}.fresh.edges"
    fresh_file.write_text("\n".join(fresh) + ("\n" if fresh else ""))
    record_leg_binary(out_dir, label, builder_commit(ws))

    targets = touch_targets(fresh, touch_n)
    if not targets:
        raise SystemExit(
            f"{out_dir.name}: no cross-file inbound edges in the fresh dump; "
            f"the incremental leg would prove nothing"
        )
    try:
        for path in targets:
            if touch_shape == "append":
                with open(path, "a", encoding="utf-8") as fh:
                    fh.write("\n")
            else:
                # Prepend: every symbol below shifts, which is what ordinary
                # editing does. An append leaves all start lines intact and so
                # cannot exercise any line-sensitive identity path.
                text = path.read_text(encoding="utf-8", errors="surrogateescape")
                path.write_text(
                    "\n" + text, encoding="utf-8", errors="surrogateescape"
                )
        # Bare `index` drives the incremental lane over the registered
        # indexed paths -- the same entry point as production.
        run([str(binary), "index"], cwd=ws)
        incr = sorted(run([str(dump), str(tantivy)]).stdout.splitlines())
    finally:
        # The corpus dir is reused by later legs, and ensure_corpus rejects a
        # dirty tree. Restoring here keeps the touch invisible to the protocol.
        run(["git", "-C", str(corpus), "checkout", "--", "."])

    incr_file = out_dir / f"{label}.incr.edges"
    incr_file.write_text("\n".join(incr) + ("\n" if incr else ""))
    run(["rm", "-rf", str(ws)])

    # A shifting touch relabels rows of the touched files; compare on
    # line-free identity so the diff shows movement, not renumbering.
    if touch_shape == "append":
        fresh_set, incr_set = set(fresh), set(incr)
    else:
        fresh_set = {strip_lines(line) for line in fresh}
        incr_set = {strip_lines(line) for line in incr}
    dropped = sorted(fresh_set - incr_set)
    gained = sorted(incr_set - fresh_set)
    (out_dir / f"drop-{label}.fresh-incr.txt").write_text(
        "\n".join(dropped) + ("\n" if dropped else "")
    )
    (out_dir / f"gain-{label}.fresh-incr.txt").write_text(
        "\n".join(gained) + ("\n" if gained else "")
    )
    touched = ", ".join(p.name for p in targets)
    print(
        f"INCREMENTAL {out_dir.name} {label} [{touch_shape}]: touched {len(targets)} "
        f"({touched}); fresh {len(fresh_set)} -> incremental {len(incr_set)}: "
        f"dropped {len(dropped)}, gained {len(gained)}"
    )
    if dropped or gained:
        print(
            f"   LANE PARITY BROKEN - the fresh lane is the oracle\n"
            f"   {out_dir / f'drop-{label}.fresh-incr.txt'}\n"
            f"   {out_dir / f'gain-{label}.fresh-incr.txt'}"
        )
    return not (dropped or gained)


def cmd_run(args):
    out = Path(args.out).resolve()
    binary = Path(args.binary).resolve()
    dump = Path(args.dump).resolve()
    for p, what in [(binary, "--binary"), (dump, "--dump")]:
        if not p.is_file():
            raise SystemExit(f"{what} {p} is not a file")
    semantic_on = args.semantic == "on"
    if args.incremental and args.runs != 1:
        # The incremental leg replaces the plain leg entirely; silently
        # ignoring --runs would report single-run dumps as union sets.
        raise SystemExit(
            "--incremental and --runs N are separate legs: run them as two invocations"
        )
    parity_ok = True
    for spec in args.corpus:
        name, fixture, pin = parse_corpus_spec(spec)
        corpus = ensure_corpus(out, name, fixture, pin)
        out_dir = out / name
        print(f"== {name} @ {pin[:9]} binary={args.label} semantic={args.semantic}")
        if args.incremental:
            parity_ok &= incremental_leg(
                binary,
                dump,
                corpus,
                out_dir,
                args.label,
                semantic_on,
                args.incremental,
                args.touch_shape,
            )
            continue
        if args.runs == 1:
            leg(binary, dump, corpus, out_dir, args.label, "", semantic_on)
        else:
            files = [
                leg(binary, dump, corpus, out_dir, args.label, f".run{i+1}", semantic_on)
                for i in range(args.runs)
            ]
            lines = set()
            for f in files:
                lines.update(f.read_text().splitlines())
            union = out_dir / f"{args.label}.edges"
            union.write_text("\n".join(sorted(lines)) + ("\n" if lines else ""))
            texts = [f.read_text() for f in files]
            if all(t == texts[0] for t in texts[1:]):
                print(f"DETERMINISM {name} {args.label}: byte-identical x{args.runs}")
            else:
                print(
                    f"SCATTER {name} {args.label}: runs differ; union "
                    f"{len(lines)} rows (apply the corpus's documented exclusions)"
                )
    if not parity_ok:
        raise SystemExit("incremental leg: lane parity broken (see diffs above)")


def leg_dump(out: Path, label: str) -> list[str]:
    # Plain legs write {label}.edges (union under --runs N); incremental
    # legs write {label}.fresh.edges. Prefer the plain form, fall back to
    # the incremental leg's fresh dump rather than crashing on it.
    plain = out / f"{label}.edges"
    fresh = out / f"{label}.fresh.edges"
    target = plain if plain.is_file() else fresh
    if not target.is_file():
        raise SystemExit(f"no dump for label {label!r} in {out} (looked for {plain.name}, {fresh.name})")
    return target.read_text().splitlines()


def cmd_diff(args):
    out = Path(args.out).resolve() / args.corpus
    old = leg_dump(out, args.old)
    new = leg_dump(out, args.new)
    old_set, new_set = set(old), set(new)
    dropped = sorted(old_set - new_set)
    gained = sorted(new_set - old_set)
    drop_f = out / f"drop-{args.old}-{args.new}.txt"
    gain_f = out / f"gain-{args.old}-{args.new}.txt"
    drop_f.write_text("\n".join(dropped) + ("\n" if dropped else ""))
    gain_f.write_text("\n".join(gained) + ("\n" if gained else ""))
    print(
        f"== {args.corpus} {args.old}({len(old_set)}) -> {args.new}({len(new_set)}): "
        f"dropped {len(dropped)}, gained {len(gained)}"
    )
    print(f"   {drop_f}\n   {gain_f}")


def main():
    p = argparse.ArgumentParser(description=__doc__)
    sub = p.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run", help="index corpora and capture sorted edge dumps")
    r.add_argument("--binary", required=True, help="release codanna binary")
    r.add_argument("--label", required=True, help="leg label (pre/mid/post/...)")
    r.add_argument("--dump", required=True, help="dump_edges example binary")
    r.add_argument(
        "--corpus",
        action="append",
        required=True,
        help="NAME=FIXTURE_PATH@PIN (repeatable)",
    )
    r.add_argument("--out", required=True, help="battery output root")
    r.add_argument("--runs", type=int, default=1, help="runs per leg (2 for scatter corpora)")
    r.add_argument("--semantic", choices=["on", "off"], default="off")
    r.add_argument(
        "--incremental",
        type=int,
        metavar="N",
        help="incremental lane leg: index fresh, touch the N files carrying the "
        "most cross-file inbound edges, re-index without force, and diff against "
        "the fresh dump (the oracle). Exits non-zero on any diff.",
    )
    r.add_argument(
        "--touch-shape",
        choices=["prepend", "append"],
        default="prepend",
        help="how --incremental edits a file. prepend (default) shifts every "
        "symbol below it, which is what ordinary editing does and what "
        "exercises line-sensitive identity paths; append leaves all start "
        "lines intact and is the control.",
    )
    r.set_defaults(func=cmd_run)
    d = sub.add_parser("diff", help="dropped/gained between two captured legs")
    d.add_argument("--out", required=True)
    d.add_argument("--corpus", required=True)
    d.add_argument("--old", required=True)
    d.add_argument("--new", required=True)
    d.set_defaults(func=cmd_diff)
    args = p.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
