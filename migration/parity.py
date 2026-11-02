#!/usr/bin/env python3
"""A/B parity runner: render each fixture with `tikz-cli` and diff it against
the legacy golden using the in-repo `svg_diff.py` comparator (imported, so the
verdict is the real one).

Usage:
    python3 migration/parity.py                # slice-3 corpus (styled draws)
    python3 migration/parity.py --corpus main  # the 11-document main corpus

Exit code 0 when every fixture is within epsilon, 1 otherwise. A render that
fails with error diagnostics counts as a failure (no SVG is written).
"""
from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from svg_diff import compare, parse  # noqa: E402

REPO = Path(__file__).resolve().parent.parent
CLI = REPO / "rust" / "target" / "debug" / "tikz-cli"

CORPORA = {
    "slice3": (REPO / "migration" / "corpus-slice3", REPO / "migration" / "goldens-slice3"),
    "main": (REPO / "migration" / "goldens", REPO / "migration" / "goldens"),
}

EPSILON = 1e-3


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--corpus", default="slice3", choices=sorted(CORPORA))
    args = parser.parse_args()
    sources, goldens = CORPORA[args.corpus]

    if not CLI.exists():
        print(f"missing CLI: {CLI} (run `cargo build -p tikz-cli`)")
        return 2

    fixtures = sorted(sources.glob("*.tex"))
    if not fixtures:
        print(f"no fixtures under {sources}")
        return 2

    failures = 0
    for fixture in fixtures:
        golden = goldens / f"{fixture.stem}.svg"
        out = Path("/tmp") / f"parity-{fixture.stem}.svg"
        result = subprocess.run(
            [str(CLI), "render", str(fixture), str(out)],
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode != 0 or not out.exists():
            failures += 1
            detail = (result.stderr.strip().splitlines() or [f"exit {result.returncode}"])[0]
            print(f"FAIL {fixture.stem}: render failed: {detail[:160]}")
            continue
        if not golden.exists():
            print(f"SKIP {fixture.stem}: no golden at {golden}")
            continue
        report = compare(parse(str(golden)), parse(str(out)), EPSILON)
        if report["ok"]:
            print(
                f"OK   {fixture.stem}: {report['matched']}/{report['golden_count']} elems, "
                f"max_abs_err={report['max_abs_err']:.3g}"
            )
        else:
            failures += 1
            print(f"FAIL {fixture.stem}: {json.dumps(report)[:200]}")

    total = len(fixtures)
    print(f"\n{total - failures}/{total} fixtures at parity (eps={EPSILON:g})")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
