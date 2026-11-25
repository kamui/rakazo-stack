"""Per-turn churn of Hindsight facts for one bot's appended conversation document.

usage: python3 -I reextract.py <snapshots-dir> <tag> <bank-substring>
Reads <tag>-NN.tsv written by replay.sh (bank, document, chunk, type, id, created, consolidated, text).
For each turn: conversation facts kept / deleted / added, how many added facts re-state a deleted one
(same text), observations total, and how many facts were consolidated during the turn.
"""
import glob
import os
import sys

snap_dir, tag, bank = sys.argv[1], sys.argv[2], sys.argv[3]


def load(path):
    rows = []
    with open(path) as f:
        for line in f:
            parts = line.rstrip("\n").split("\t")
            if len(parts) < 8 or bank not in parts[0]:
                continue
            b, doc, chunk, ftype, uid, created, consolidated, text = parts[:8]
            rows.append(dict(doc=doc, chunk=chunk, type=ftype, id=uid, consolidated=consolidated, text=text))
    return rows


files = sorted(glob.glob(os.path.join(snap_dir, f"{tag}-*.tsv")))
prev = []
print("turn\tconv_facts\tkept\tdeleted\tadded\tadded_same_text_as_deleted\tchunks\tobservations\tsaved_facts\tconsolidated_this_turn")
for path in files:
    turn = path.rsplit("-", 1)[1].split(".")[0]
    rows = load(path)
    conv = {r["id"]: r for r in rows if r["doc"].startswith("rakazo-") and r["type"] != "observation"}
    pconv = {r["id"]: r for r in prev if r["doc"].startswith("rakazo-") and r["type"] != "observation"}
    kept = conv.keys() & pconv.keys()
    deleted = pconv.keys() - conv.keys()
    added = conv.keys() - pconv.keys()
    deleted_texts = {pconv[i]["text"] for i in deleted}
    same = sum(1 for i in added if conv[i]["text"] in deleted_texts)
    chunks = len({r["chunk"] for r in conv.values()})
    obs = sum(1 for r in rows if r["type"] == "observation")
    saved = sum(1 for r in rows if not r["doc"].startswith("rakazo-") and r["type"] != "observation" and r["doc"] != "-")
    prev_cons = {r["id"] for r in prev if r["consolidated"]}
    newly = sum(1 for r in rows if r["type"] != "observation" and r["consolidated"] and r["id"] not in prev_cons)
    print(f"{turn}\t{len(conv)}\t{len(kept)}\t{len(deleted)}\t{len(added)}\t{same}\t{chunks}\t{obs}\t{saved}\t{newly}")
    prev = rows
