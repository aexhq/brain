#!/usr/bin/env python3
"""Regenerate the Rust protocol views from their authoritative JSON Schemas.

Requires cargo-typify 0.7.0. Generated files are then passed through the repository's
security postprocessor, and the Hand schema's RFC 8785 identity is refreshed. The schemas use
ASCII property names and integer numbers only, which makes Python's compact sorted JSON encoding
identical to RFC 8785 for this input; assertions below make that assumption fail closed.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import pathlib
import subprocess
import sys
from collections.abc import Iterator
from typing import Any


ROOT = pathlib.Path(__file__).resolve().parents[1]
SCHEMAS = {
    "hand": (
        ROOT / "contracts/hand/contract.json",
        ROOT / "crates/brain-protocol/src/hand.rs",
    ),
    "session": (
        ROOT / "contracts/session/v1/schemas.json",
        ROOT / "crates/brain-protocol/src/session.rs",
    ),
}


def walk(value: Any) -> Iterator[Any]:
    yield value
    if isinstance(value, dict):
        for key, child in value.items():
            if not key.isascii():
                raise RuntimeError("protocol schema property names must stay ASCII for digest generation")
            yield from walk(child)
    elif isinstance(value, list):
        for child in value:
            yield from walk(child)


def hand_digest(schema_path: pathlib.Path) -> str:
    value = json.loads(schema_path.read_text(encoding="utf-8"))
    if any(isinstance(item, float) for item in walk(value)):
        raise RuntimeError("protocol schema contains a floating-point number; use a full JCS encoder")
    canonical = json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
        allow_nan=False,
    ).encode("utf-8")
    return hashlib.sha256(canonical).hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("target", choices=["all", *SCHEMAS], default="all", nargs="?")
    args = parser.parse_args()

    version = subprocess.run(
        ["cargo", "typify", "--version"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    ).stdout.strip()
    if version != "cargo-typify 0.7.0":
        raise RuntimeError(f"expected cargo-typify 0.7.0, found {version!r}")

    selected = SCHEMAS if args.target == "all" else {args.target: SCHEMAS[args.target]}
    for schema, output in selected.values():
        subprocess.run(
            ["cargo", "typify", "--output", str(output), str(schema)],
            cwd=ROOT,
            check=True,
        )

    subprocess.run(
        [sys.executable, str(ROOT / "tools/postprocess-generated.py")],
        cwd=ROOT,
        check=True,
    )
    if "hand" in selected:
        digest_path = ROOT / "contracts/hand/contract.digest"
        digest_path.write_text(hand_digest(SCHEMAS["hand"][0]) + "\n", encoding="ascii")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
