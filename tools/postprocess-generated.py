#!/usr/bin/env python3
"""Apply security-sensitive traits that schema generators cannot express.

The JSON schemas remain authoritative.  Our Rust schema generator derives `Debug` for every
generated type, including write-only values and short-lived bearer capabilities.  Run this
postprocessor after regenerating `hand.rs` or `session.rs`; `--check` is suitable for CI.

The expected match counts deliberately make schema/generator drift fail closed.
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys


ROOT = pathlib.Path(__file__).resolve().parents[1]

TARGETS = {
    ROOT / "crates/brain-protocol/src/hand.rs": {
        "BundleFetch": 2,
        "BundleFetchHeadersValue": 1,
        "BundleFetchUrl": 1,
        "ObjectTransferAuthority": 2,
        "ObjectTransferAuthorityHeadersValue": 1,
        "ObjectTransferAuthorityUrl": 1,
        "PrepareSessionRequest": 2,
        "SandboxCopyRequest": 2,
        "SandboxFileWriteRequest": 2,
        "SandboxFileWriteSource": 1,
        "SecretCapability": 2,
        "SecretDeliveryRequest": 2,
    },
    ROOT / "crates/brain-protocol/src/session.rs": {
        "CreateSessionRequest": 2,
        "ModelConfig": 2,
        "ModelConfigApiKey": 1,
        "ToolBundle": 2,
        "ToolBundleContentBase64": 1,
    },
}


def remove_debug(body: str) -> str:
    body = re.sub(r"(?m)^[ \t]*Debug,[ \t]*\r?\n", "", body)
    body = re.sub(r",\s*Debug(?=\s*(?:,|$))", "", body)
    body = re.sub(r"\bDebug\s*,\s*", "", body)
    if re.search(r"\bDebug\b", body):
        raise RuntimeError("could not remove Debug from generated derive")
    return body


def transform(source: str, targets: dict[str, int], path: pathlib.Path) -> str:
    for name, expected in targets.items():
        pattern = re.compile(
            r"(?P<prefix>#\[derive\()(?P<body>[^)]*)(?P<suffix>\)\])"
            r"(?P<middle>\s*(?:#\[[^\n]+\]\s*)*)"
            rf"(?P<item>pub (?:struct|enum) {re.escape(name)}\b)",
            re.MULTILINE,
        )

        matches = list(pattern.finditer(source))
        if len(matches) != expected:
            raise RuntimeError(
                f"{path}: expected {expected} generated {name} definitions, found {len(matches)}"
            )

        def rewrite(match: re.Match[str]) -> str:
            body = match.group("body")
            if not re.search(r"\bDebug\b", body):
                return match.group(0)
            return (
                match.group("prefix")
                + remove_debug(body)
                + match.group("suffix")
                + match.group("middle")
                + match.group("item")
            )

        source = pattern.sub(rewrite, source)
    return source


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    changed: list[pathlib.Path] = []
    for path, targets in TARGETS.items():
        original = path.read_text(encoding="utf-8")
        processed = transform(original, targets, path)
        if processed != original:
            changed.append(path)
            if not args.check:
                path.write_text(processed, encoding="utf-8")

    if args.check and changed:
        for path in changed:
            print(f"generated redaction postprocess required: {path.relative_to(ROOT)}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
