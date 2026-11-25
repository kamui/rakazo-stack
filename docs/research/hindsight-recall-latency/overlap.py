"""Compare top-K recall results between two runs, per query (first occurrence of each query).

usage: overlap.py RESULTS_DIR K BASE_LABEL:BUDGET OTHER_LABEL:BUDGET [...]
"""
import glob
import json
import sys

d, K, base, others = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4:]
runs: dict[str, dict[str, list[str]]] = {}
for f in glob.glob(f"{d}/*.log"):
    for line in open(f):
        if line.startswith("RUN "):
            r = json.loads(line[4:])
            runs.setdefault(f"{r['label']}:{r['budget']}", {}).setdefault(r["q"], r["ids"])

for o in others:
    A, B = runs[base], runs[o]
    qs = [q for q in A if q in B]
    ov = sum(len(set(A[q][:K]) & set(B[q][:K])) / K for q in qs) / len(qs)
    top1 = sum(A[q][:1] == B[q][:1] for q in qs)
    print(f"{o:28s} vs {base}: overlap@{K}={ov:.0%} top1-same={top1}/{len(qs)}")
