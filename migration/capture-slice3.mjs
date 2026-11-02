#!/usr/bin/env node
/**
 * Slice-3 golden capture: renders the styled/geometry fixtures through the
 * legacy TypeScript pipeline and writes goldens next to the source, for the
 * Rust A/B parity check (`migration/svg_diff.py`).
 *
 * These fixtures deliberately contain only constructs the Rust port claims:
 * plain/dashed/styled paths, shapes (rectangle/circle/ellipse/arc/grid),
 * relative and polar coordinates and frame transforms. Text, nodes, arrows
 * and libraries stay in the main corpus and are excluded here.
 *
 * Usage: node migration/capture-slice3.mjs
 */
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const repoRoot = resolve(new URL("..", import.meta.url).pathname);
const fixtureDir = join(repoRoot, "migration", "corpus-slice3");
const outDir = join(repoRoot, "migration", "goldens-slice3");

const CORPUS = ["styled", "geometry", "gridlines"];

async function main() {
  let renderTikzToSvgAsync;
  try {
    ({ renderTikzToSvgAsync } = await import("@tikz-editor/core"));
  } catch {
    ({ renderTikzToSvgAsync } = await import("@tikz-editor/core/render/index.js"));
  }

  mkdirSync(outDir, { recursive: true });
  for (const name of CORPUS) {
    const source = readFileSync(join(fixtureDir, `${name}.tex`), "utf8");
    const result = await renderTikzToSvgAsync(source);
    writeFileSync(join(outDir, `${name}.svg`), result.svg.svg);
    writeFileSync(join(outDir, `${name}.tex`), source);
    console.log(
      `golden ${name}: ${result.svg.svg.length}B, diag ${result.renderDiagnostics.length}`,
    );
    for (const diagnostic of result.renderDiagnostics) {
      console.log(`  diag: ${diagnostic.code ?? "?"} ${diagnostic.message ?? ""}`);
    }
  }
}

await main();
