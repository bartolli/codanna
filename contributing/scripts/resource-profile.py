#!/usr/bin/env python3
"""Resource profile of one `codanna index` run: wall, CPU, peak RSS, threads.

Fresh workspace per run (init, then index the corpus), release binary only.
Thread states are sampled once per second from `ps`; peak RSS and CPU come
from the platform `time` binary. Writes `time.txt`, `threads.txt`, `index.log`
and `summary.txt` under --out and prints the summary.

    resource-profile.py --binary target/release/codanna --corpus /path/to/src \
        --out /private/tmp/profile-main [--embedding-threads 3] [--label main]
"""

import argparse
import os
import platform
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

MACOS = platform.system() == "Darwin"
TIME_FLAGS = ["-l"] if MACOS else ["-v"]
RSS_PATTERN = (
    re.compile(r"^\s*(\d+)\s+maximum resident set size", re.M)
    if MACOS
    else re.compile(r"Maximum resident set size \(kbytes\):\s*(\d+)")
)


def sample_threads(pid):
    """Return (total, running) thread counts for pid, or None once it is gone."""
    if MACOS:
        proc = subprocess.run(["ps", "-M", "-p", str(pid)], capture_output=True, text=True)
        lines = proc.stdout.splitlines()[1:]
        running = sum(1 for line in lines if re.search(r"\sR[+\w]*\s", line))
    else:
        proc = subprocess.run(
            ["ps", "-L", "-o", "stat=", "-p", str(pid)], capture_output=True, text=True
        )
        lines = [line for line in proc.stdout.splitlines() if line.strip()]
        running = sum(1 for line in lines if line.strip().startswith("R"))
    if proc.returncode != 0 or not lines:
        return None
    return len(lines), running


def read_setting(settings, key):
    for line in settings.read_text().splitlines():
        if line.startswith(f"{key} = "):
            return line.split("=", 1)[1].strip()
    return None


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--binary", required=True, type=Path)
    ap.add_argument("--corpus", required=True, type=Path)
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--embedding-threads", type=int)
    ap.add_argument("--label", default="run")
    args = ap.parse_args()

    binary = args.binary.resolve()
    corpus = args.corpus.resolve()
    out = args.out.resolve()
    if out.exists():
        shutil.rmtree(out)
    workspace = out / "ws"
    workspace.mkdir(parents=True)

    subprocess.run([binary, "init"], cwd=workspace, check=True, capture_output=True)
    settings = workspace / ".codanna" / "settings.toml"
    if args.embedding_threads is not None:
        text = settings.read_text()
        text, n = re.subn(
            r"^embedding_threads = .*$",
            f"embedding_threads = {args.embedding_threads}",
            text,
            flags=re.M,
        )
        if n != 1:
            sys.exit("embedding_threads line not found in settings.toml")
        settings.write_text(text)
    threads_setting = read_setting(settings, "embedding_threads")
    version = subprocess.run([binary, "--version"], capture_output=True, text=True).stdout.strip()

    time_bin = "/usr/bin/time"
    with open(out / "index.log", "w") as log, open(out / "time.txt", "w") as timing:
        proc = subprocess.Popen(
            [time_bin, *TIME_FLAGS, binary, "index", str(corpus)],
            cwd=workspace,
            stdout=log,
            stderr=timing,
        )
        child = None
        samples = []
        while proc.poll() is None:
            if child is None:
                pg = subprocess.run(
                    ["pgrep", "-P", str(proc.pid)], capture_output=True, text=True
                ).stdout.split()
                child = int(pg[0]) if pg else None
            if child is not None:
                sample = sample_threads(child)
                if sample:
                    samples.append(sample)
            time.sleep(1)

    (out / "threads.txt").write_text("".join(f"{t} {r}\n" for t, r in samples))
    timing_text = (out / "time.txt").read_text()
    rss = RSS_PATTERN.search(timing_text)
    rss_bytes = int(rss.group(1)) * (1 if MACOS else 1024) if rss else None
    real = re.search(r"([\d.]+)\s+real", timing_text) or re.search(r"Elapsed.*?([\d:.]+)$", timing_text, re.M)
    user = re.search(r"([\d.]+)\s+user", timing_text) or re.search(r"User time \(seconds\):\s*([\d.]+)", timing_text)
    totals = [t for t, _ in samples] or [0]
    running = [r for _, r in samples] or [0]

    summary = "\n".join(
        [
            f"label: {args.label}",
            f"binary: {version}",
            f"corpus: {corpus}",
            f"embedding_threads: {threads_setting}",
            f"exit: {proc.returncode}",
            f"wall_s: {real.group(1) if real else '?'}",
            f"cpu_user_s: {user.group(1) if user else '?'}",
            f"peak_rss_bytes: {rss_bytes if rss_bytes is not None else '?'}",
            f"samples: {len(samples)}",
            f"threads_total max/avg: {max(totals)} / {sum(totals) / len(totals):.0f}",
            f"threads_running max/avg: {max(running)} / {sum(running) / len(running):.0f}",
        ]
    )
    (out / "summary.txt").write_text(summary + "\n")
    print(summary)
    return proc.returncode


if __name__ == "__main__":
    sys.exit(main())
