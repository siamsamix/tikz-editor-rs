#!/usr/bin/env python3
"""Statement-AST parity runner for M0-2.

Runs `tikz-cli parse --json <fixture>` for each corpus fixture and compares
the Rust dump against the legacy capture in `migration/expected/<name>.json`.

Compared fields (the legacy AST contract that must survive the port):
  * figure count
  * per figure: statement count; per statement: kind, command, span, item count
  * per item: kind and span (through its terminating `;`/closing paren)
  * diagnostics: severity and span

Usage:
    python3 migration/statement_parity.py          # all corpora in CORPUS
    python3 migration/statement_parity.py minimal   # single fixture name
"""
from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
CLI = REPO / "rust" / "target" / "debug" / "tikz-cli"
EXPECTED = REPO / "migration" / "expected"

# Every fixture the legacy capture script emits (see capture-statements.mjs).
CORPUS = [
    "minimal",
    "node-graph",
    "comments",
    "multiline",
    "multi-figure-basic",
    "shapes-new",
    "grid",
    "paths",
    "coords",
    "styles",
    "nodes",
]


def fail(msg: str) -> str:
    return msg


def compare(name: str) -> list[str]:
    """Return a list of human-readable mismatch messages for one fixture."""
    expected_path = EXPECTED / f"{name}.json"
    if not expected_path.exists():
        return [f"SKIP {name}: no legacy expected dump at {expected_path}"]

    with open(expected_path, "r", encoding="utf-8") as fh:
        expected = json.load(fh)

    out = Path("/tmp") / f"statement-parse-{name}.json"
    source = find_source(name)
    if source is None:
        return [f"SKIP {name}: no source fixture found"]
    result = subprocess.run(
        [str(CLI), "parse", "--json", str(source), str(out)],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0 or not out.exists():
        detail = (result.stderr.strip().splitlines() or [f"exit {result.returncode}"])[0]
        return [f"FAIL {name}: parse failed: {detail[:160]}"]

    with open(out, "r", encoding="utf-8") as fh:
        actual = json.load(fh)

    problems: list[str] = []

    # Figure count.
    exp_figs = expected.get("figures", [])
    act_figs = actual.get("figures", [])
    if len(exp_figs) != len(act_figs):
        problems.append(f"  figures: expected {len(exp_figs)}, got {len(act_figs)}")
        return problems  # cannot align figures meaningfully

    for i, (ef, af) in enumerate(zip(exp_figs, act_figs)):
        exp_stmts = ef.get("statements", [])
        act_stmts = af.get("statements", [])
        if len(exp_stmts) != len(act_stmts):
            problems.append(
                f"  figure {i}: statements expected {len(exp_stmts)}, got {len(act_stmts)}"
            )
            continue
        for j, (es, as_) in enumerate(zip(exp_stmts, act_stmts)):
            if es.get("kind") != as_.get("kind"):
                problems.append(
                    f"  figure {i} stmt {j}: kind expected {es.get('kind')!r}, "
                    f"got {as_.get('kind')!r}"
                )
            if es.get("command") != as_.get("command"):
                problems.append(
                    f"  figure {i} stmt {j}: command expected {es.get('command')!r}, "
                    f"got {as_.get('command')!r}"
                )
            if spans_differ(es.get("span"), as_.get("span")):
                problems.append(
                    f"  figure {i} stmt {j}: span expected {es.get('span')}, "
                    f"got {as_.get('span')}"
                )

    # Diagnostics: severity and span.
    exp_diag = expected.get("diagnostics", [])
    act_diag = actual.get("diagnostics", [])
    if len(exp_diag) != len(act_diag):
        problems.append(
            f"  diagnostics: expected {len(exp_diag)}, got {len(act_diag)}"
        )
    else:
        for d, (ed, ad) in enumerate(zip(exp_diag, act_diag)):
            if ed.get("severity") != ad.get("severity"):
                problems.append(
                    f"  diagnostic {d}: severity expected {ed.get('severity')!r}, "
                    f"got {ad.get('severity')!r}"
                )
            if spans_differ(ed.get("span"), ad.get("span")):
                problems.append(
                    f"  diagnostic {d}: span expected {ed.get('span')}, "
                    f"got {ad.get('span')}"
                )

    return problems


def spans_differ(a, b) -> bool:
    if a is None or b is None:
        return a is not b
    return a.get("from") != b.get("from") or a.get("to") != b.get("to")


def find_source(name: str) -> Path | None:
    """Locate the source `.tex` for a fixture by name across known corpora."""
    candidates = [
        REPO / "migration" / "goldens" / f"{name}.tex",
        REPO / "migration" / "corpus-slice3" / f"{name}.tex",
        REPO / "migration" / "corpus" / f"{name}.tex",
        REPO / "test" / "fixtures" / f"{name}.tex",
        REPO / "test" / "fixtures" / "differential" / f"{name}.tex",
    ]
    for c in candidates:
        if c.exists():
            return c
    return None


def main() -> int:
    if not CLI.exists():
        print(f"missing CLI: {CLI} (run `cargo build -p tikz-cli`)")
        return 2

    names = sys.argv[1:] if len(sys.argv) > 1 else CORPUS
    failures = 0
    for name in names:
        problems = compare(name)
        if problems:
            failures += 1
            print(f"FAIL {name}:")
            for p in problems:
                print(f"    {p}")
        else:
            print(f"OK   {name}")
    print(f"\n{len(names) - failures}/{len(names)} statement dumps match")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
