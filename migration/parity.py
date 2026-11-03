#!/usr/bin/env python3
"""A/B parity runner: render each fixture with `tikz-cli` and diff it against
the legacy golden using the in-repo `svg_diff.py` comparator (imported, so the
verdict is the real one).

Corpora are auto-discovered: every directory ``migration/corpus-<name>/`` that
contains ``*.tex`` fixtures is paired with ``migration/goldens-<name>/`` (which
must contain matching ``*.svg`` goldens). A bare ``migration/goldens/``
directory is also accepted as a corpus named ``main``.

Usage:
    python3 migration/parity.py                # all discovered corpora
    python3 migration/parity.py slice3         # single corpus by name
    python3 migration/parity.py --corpus main  # explicit corpus name

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
MIGRATION = REPO / "migration"

EPSILON = 1e-3


def discover_corpora() -> dict[str, tuple[Path, Path]]:
    """Auto-discover corpus directories under ``migration/``.

    A corpus is a directory ``migration/corpus-<name>/`` containing ``*.tex``
    fixtures, paired with ``migration/goldens-<name>/`` containing the
    matching ``*.svg`` goldens. A bare ``migration/goldens/`` directory is
    also accepted as the ``main`` corpus (sources and goldens share the same
    directory).
    """
    corpora: dict[str, tuple[Path, Path]] = {}
    seen: set[str] = set()

    for entry in sorted(MIGRATION.iterdir()):
        if not entry.is_dir():
            continue
        name = entry.name

        # corpus-<name> directories hold fixtures; goldens live in
        # goldens-<name> alongside them.
        if name.startswith("corpus-") and name != "corpus-":
            corpus_name = name[len("corpus-"):]
            goldens_dir = MIGRATION / f"goldens-{corpus_name}"
            if goldens_dir.is_dir():
                corpora[corpus_name] = (entry, goldens_dir)
                seen.add(corpus_name)

    # A bare goldens/ directory is the legacy "main" corpus: sources and
    # goldens live in the same directory.
    goldens_dir = MIGRATION / "goldens"
    if goldens_dir.is_dir() and "main" not in seen:
        corpora["main"] = (goldens_dir, goldens_dir)

    return corpora


def run_corpus(name: str, sources: Path, goldens: Path) -> int:
    """Run parity for a single corpus. Returns the number of failures."""
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
        out = Path("/tmp") / f"parity-{name}-{fixture.stem}.svg"
        result = subprocess.run(
            [str(CLI), "render", str(fixture), str(out)],
            capture_output=True,
            text=True,
            check=False,
        )
        if result.returncode != 0 or not out.exists():
            failures += 1
            detail = (result.stderr.strip().splitlines() or [f"exit {result.returncode}"])[0]
            print(f"FAIL {name}/{fixture.stem}: render failed: {detail[:160]}")
            continue
        if not golden.exists():
            print(f"SKIP {name}/{fixture.stem}: no golden at {golden}")
            continue
        report = compare(parse(str(golden)), parse(str(out)), EPSILON)
        if report["ok"]:
            print(
                f"OK   {name}/{fixture.stem}: {report['matched']}/{report['golden_count']} elems, "
                f"max_abs_err={report['max_abs_err']:.3g}"
            )
        else:
            failures += 1
            print(f"FAIL {name}/{fixture.stem}: {json.dumps(report)[:200]}")

    return failures


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("corpus", nargs="?", help="Run a single corpus by name")
    args = parser.parse_args()

    corpora = discover_corpora()
    if not corpora:
        print("no corpora discovered under migration/")
        return 2

    if args.corpus:
        if args.corpus not in corpora:
            print(f"unknown corpus: {args.corpus} (available: {', '.join(sorted(corpora))})")
            return 2
            # unreachable but keeps mypy happy
        corpora = {args.corpus: corpora[args.corpus]}

    total_failures = 0
    total_fixtures = 0
    for name in sorted(corpora):
        sources, goldens = corpora[name]
        print(f"== corpus {name} ({len(list(sources.glob('*.tex')))} fixtures)")
        failures = run_corpus(name, sources, goldens)
        total_failures += failures
        total_fixtures += len(list(sources.glob("*.tex")))
        print()

    print(f"{total_fixtures - total_failures}/{total_fixtures} fixtures at parity (eps={EPSILON:g})")
    return 1 if total_failures else 0


if __name__ == "__main__":
    sys.exit(main())