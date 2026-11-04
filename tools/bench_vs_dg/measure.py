"""Run a command; record wall time, exit code, peak single-process RSS and peak process-tree RSS sum.

usage: measure.py OUT.json LOG -- CMD...   (stdout+stderr of CMD go to LOG)
Tree RSS sums VmRSS over the process and all descendants every 0.2 s, so forked workers count (shared
pages count once per process: an upper bound).
"""
import json
import os
import subprocess
import sys
import time


def children_map():
    kids = {}
    for pid in os.listdir("/proc"):
        if not pid.isdigit():
            continue
        try:
            with open(f"/proc/{pid}/stat") as f:
                ppid = int(f.read().rsplit(")", 1)[1].split()[1])
        except OSError:
            continue
        kids.setdefault(ppid, []).append(int(pid))
    return kids


def rss_threads(pid):
    rss = threads = 0
    try:
        with open(f"/proc/{pid}/status") as f:
            for line in f:
                if line.startswith("VmRSS:"):
                    rss = int(line.split()[1])
                elif line.startswith("Threads:"):
                    threads = int(line.split()[1])
    except OSError:
        pass
    return rss, threads


def tree(root):
    kids, out, todo = children_map(), [], [root]
    while todo:
        p = todo.pop()
        out.append(p)
        todo.extend(kids.get(p, []))
    return out


def main():
    out, log, sep, *cmd = sys.argv[1:]
    assert sep == "--" and cmd, __doc__
    t0 = time.monotonic()
    with open(log, "wb") as lf:
        proc = subprocess.Popen(cmd, stdout=lf, stderr=subprocess.STDOUT)
        peak_tree = peak_one = peak_threads = peak_procs = 0
        while proc.poll() is None:
            st = [rss_threads(p) for p in tree(proc.pid)]
            rss = [s[0] for s in st]
            peak_tree, peak_one = max(peak_tree, sum(rss)), max(peak_one, max(rss, default=0))
            peak_threads, peak_procs = max(peak_threads, sum(s[1] for s in st)), max(peak_procs, len(st))
            time.sleep(0.2)
    rec = {"cmd": cmd, "exit": proc.returncode, "wall_s": round(time.monotonic() - t0, 2),
           "peak_tree_rss_mb": round(peak_tree / 1024), "peak_proc_rss_mb": round(peak_one / 1024),
           "peak_threads": peak_threads, "peak_procs": peak_procs}
    with open(out, "w") as f:
        json.dump(rec, f)
    print(json.dumps(rec))


if __name__ == "__main__":
    main()
