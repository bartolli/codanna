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

Usage:
  e2e-battery.py run  --binary PATH --label NAME --dump PATH \
                      --corpus NAME=FIXTURE_PATH@PIN [...] \
                      --out DIR [--runs N] [--semantic on|off]
  e2e-battery.py diff --out DIR --corpus NAME --old LABEL --new LABEL

Pins are REQUIRED per corpus (numbers rot; the caller states them).
No cargo invocations here: binaries are built by the caller.
"""

import argparse
import re
import subprocess
import sys
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
    run(["rm", "-rf", str(ws)])
    print(f"EDGES {out_dir.name} {label}{suffix}: {len(edges)}")
    return edge_file


def cmd_run(args):
    out = Path(args.out).resolve()
    binary = Path(args.binary).resolve()
    dump = Path(args.dump).resolve()
    for p, what in [(binary, "--binary"), (dump, "--dump")]:
        if not p.is_file():
            raise SystemExit(f"{what} {p} is not a file")
    semantic_on = args.semantic == "on"
    for spec in args.corpus:
        name, fixture, pin = parse_corpus_spec(spec)
        corpus = ensure_corpus(out, name, fixture, pin)
        out_dir = out / name
        print(f"== {name} @ {pin[:9]} binary={args.label} semantic={args.semantic}")
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


def cmd_diff(args):
    out = Path(args.out).resolve() / args.corpus
    old = (out / f"{args.old}.edges").read_text().splitlines()
    new = (out / f"{args.new}.edges").read_text().splitlines()
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
