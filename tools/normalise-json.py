"""Rewrites the given JSON files in the repository's canonical formatting (2-space indent, key
order preserved, LF, trailing newline). Idempotent; used by tools/gen.sh so schema diffs stay small."""
import json
import sys

for path in sys.argv[1:]:
    with open(path, encoding="utf-8") as f:
        data = json.load(f)
    with open(path, "w", encoding="utf-8", newline="\n") as f:
        f.write(json.dumps(data, indent=2, ensure_ascii=False) + "\n")
