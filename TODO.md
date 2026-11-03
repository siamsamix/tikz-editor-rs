# TODO — TypeScript → pure Rust migration

**Status:** living document, ordered work queue.
**Last measured:** 2026-11-02 (baseline verified on `master` @ `b5cbfe71`).
**Audience:** AI agents continuing the port. Read this file top to bottom before touching code.

This file is the single work queue for removing the TypeScript implementation entirely. It is
deliberately concrete: every task names a legacy anchor, a Rust target, and an acceptance test.
`migration/BLUEPRINT.md` remains the architectural reference (audit, byte-level SVG contract,
module map); this file is the *remaining work*, not a re-statement of the blueprint.

---

## 0. Definition of done

The migration is complete only when **all** of these hold:

1. `cd rust && cargo build --workspace --release` produces the desktop app and CLI with no
   Node.js, npm, or TypeScript toolchain installed.
2. Every feature in the legacy capability matrix (118 IDs) is `stable` across
   `parser / semantic / svg / edit` in the Rust registry, or is explicitly listed as a
   deliberate product removal approved by the user.
3. A/B parity is green at `eps=1e-3` with **equal element counts** for every golden corpus
   (main corpus, feature corpora, text corpus, beamer corpus, PGF-manual gallery).
4. The text engine renders glyphs (`data-tex-*`) at parity with the legacy TeX renderer,
   including math, paragraphs, lists, tables, and paragraph line breaking.
5. Beamer documents render and edit at parity, including themes, overlays, builds, and the
   slide manager.
6. The egui app covers the legacy editor feature set: selection/transform, inspector writes,
   undo/redo, menus, files/workspaces, export, presentation, settings.
7. Desktop and web (whichever shipping targets the user approves in D1) build in CI from
   `rust/` only; landing ships without TypeScript.
8. TypeScript sources (`packages/*`, `apps/*/src`) are deleted; `package.json`, `tsconfig*`,
   ESLint, Vitest, Playwright, and the lezer packages are removed; docs/CI point at Cargo.
9. `#![forbid(unsafe_code)]` everywhere, `cargo clippy --workspace --all-targets` has 0
   warnings, `cargo fmt --check` passes, and the coverage target (≥95% branch, or the
   documented `cargo-llvm-cov` exception) is recorded in the telemetry ledger.

Do not declare a phase done because unit tests pass. A phase is done when its **end-to-end
parity** acceptance is green.

---

## 1. Measured baseline (do not trust stale numbers elsewhere)

Re-run these before starting; update this section when a phase lands.

```bash
cd rust
cargo test --workspace --all-targets      # 145 passed, 0 failed (133 core + 10 app + 2 cli)
cargo test --workspace --doc              #  40 passed, 0 failed
cargo clippy --workspace --all-targets    # 0 warnings
cargo build -p tikz-cli
cd ..
python3 migration/parity.py               # 3/3  (slice-3: styled, geometry, gridlines)
python3 migration/parity.py --corpus main # 1/11 (only `minimal`)
```

| Side | Metric | Value |
| :--- | :--- | :--- |
| Rust | crates | 3 (`tikz-core` 9,463 LOC / 37 files, `tikz-app` 1,092 / 6, `tikz-cli` 84 / 1) |
| Rust | tests | 145 unit/integration + 40 doc tests |
| Rust | parity | slice-3 3/3, main corpus 1/11 |
| Legacy | core | 650 files, 205,429 LOC |
| Legacy | app | 265 files, 81,768 LOC |
| Legacy | apps | desktop 10,249 (incl. 7,303 Rust Tauri backend), web 748 TS + 82 e2e/profiling files, landing 5,365 |
| Legacy | tests | 497 spec files, 526 fixtures |
| Legacy | capability rows | 118 features: parser 84 stable / 22 partial / 4 none; semantic 72/19/3; svg 71/40/3; **edit 2/63/53** |
| Legacy | lint/tests | `lint:prod`, Vitest, Playwright web + desktop e2e, tex-adversarial fuzz |

### 1.1 What renders in Rust today

- Commands: `\draw`, `\path`, `\fill`, `\filldraw`, `\shade`, `\shadedraw`. `\clip` warns and is
  ignored; every other statement warns and is skipped.
- Coordinates: Cartesian, polar, `+`/`++` relative offsets, arithmetic `+ - * /` and
  parentheses, units `cm pt mm in bp`.
- Path ops: `--`, `|-`, `-|`, `cycle`, `rectangle`, `circle` (`(r)` and `[radius=]`), `ellipse`,
  `arc`, `grid` (`[step=]`).
- Styles: colour names, `name!pct` mixes, `#rgb`/`#rrggbb`, `color/draw/fill=`, width keywords,
  `line width=`, dash keywords, `dash pattern`/`dash phase`, opacity trio, `transparent`,
  even-odd/nonzero, `scale`/`xscale`/`yscale`, `shift`/`xshift`/`yshift`, `rotate`/
  `rotate around`, `radius`/`x radius`/`y radius`, `start angle`/`end angle`, `step`.
- Scene elements: `ScenePath` and `SceneEllipse` only.
- SVG: viewBox (12 pt pad + half stroke), y-flip, attribute order, ECMAScript `toFixed(4)`
  tie rule, dash/opacity/fill-rule attributes.
- egui shell: New/Open/Save/Save-As, multiline source editor, canvas pan/zoom/pick/hover,
  read-only inspector, diagnostics panel, Copy SVG. **No edit commands, no undo, no menus, no
  watchers, no themes, no export.**

### 1.2 What is missing (the work in this file)

`to`/`edge` are currently drawn as straight lines with a warning; `node`/`pic` warn and skip;
`plot`, `parabola`, `coordinates`, `let`, `svg`, `decorate`, `child`, `graph` are hard errors.
There are **no nodes, no text, no arrows, no patterns/shadings, no clipping, no multiple
figures, no beamer, no edit actions, no undo, and no app feature parity.** The legacy `edit`
layer (33.6K LOC) and the app UI (81.8K LOC) are nearly untouched.

### 1.3 Remaining effort by subsystem (LOC proxy, not schedule)

| Subsystem | Legacy LOC | Rust today | Notes |
| :--- | ---: | ---: | :--- |
| text engine (`text/`) | 63,059 | 0 | Largest single block; parser, Knuth-Plass, math, metrics |
| semantic (`semantic/`) | 49,395 | 2,236 | Paths, nodes, styles, graphs, decorations, pgfmath |
| edit (`edit/`) | 33,644 | 0 | Actions, inspector, writes, snapping |
| beamer (`beamer/`) | 25,967 | 0 | Themes + frame flow + slide manager |
| editing (`editing/`) | 11,223 | 0 | Editor tree/transactions/input |
| svg (`svg/`) | 4,893 | ~450 | Arrows, patterns, shading, text, defs |
| domains/foreach/parser/transform | ~10,800 | ~3,800 | Front-end + statement mapping |
| app UI (`packages/app`) | 81,768 | 1,092 | egui port |
| shells + landing | ~18,400 | 84 (CLI) | Desktop/web/landing |

---

## 2. Rules for every agent working this list

### 2.1 Status conventions

Edit the checkbox in place; do not duplicate tasks.

- `[ ]` todo
- `[~]` in progress — append `Owner: <agent/date>` to the task body
- `[x]` done — append `Done: YYYY-MM-DD, <commit sha>` and update §1 numbers
- `[!]` blocked — append `Blocked by: <task id / decision id>`

One task per commit (or a small vertical slice). Do not start a later phase while its
dependency is open unless the task explicitly says it can run in parallel.

### 2.2 Non-negotiables

1. **The legacy TypeScript is the parity oracle. Do not edit it.** Bug-for-bug behaviour is the
   goal; "fixing" legacy while porting destroys the oracle.
2. **Never silently drop input.** Every unsupported construct must produce a typed diagnostic
   (`Issue` with severity + span) and keep rendering the rest of the document where legacy does.
3. **Check PGF before guessing.** `pgf-docs/` and `pgf-src/` are the semantic authority for
   TikZ behaviour; the legacy code is the authority for *output bytes*.
4. **Use `f64`.** TypeScript numbers are doubles. `f32` breaks 4-decimal rounding and viewBox
   parity.
5. **No regex in the parser.** Hand-written byte scanners, matching the legacy structural
   charset. Regex is allowed only in tests/tooling.
6. **Keep modules ≤400 LOC.** Split rather than grow; the audit flags the legacy Hydra files.
7. **No `unsafe`.** `#![forbid(unsafe_code)]` at every crate root.
8. **Dependency policy:** `tikz-core` currently has zero runtime dependencies. Prefer `std`;
   adding a small, pure-Rust crate (`serde`/`serde_json` for AST parity) is acceptable, but
   record why. Do not pull in a rendering/DOM/JS runtime.
9. **Do not compare partial output to text-bearing goldens.** Until the text engine lands,
   main-corpus element counts cannot match (glyph/wrapper elements). Use text-free corpora for
   draw/semantic parity; treat text-bearing goldens as blocklisted.
10. **Do not commit `target/`, `__pycache__/`, or profiling traces.**
11. The worktree may contain unrelated dirty files kept by the user. Do not revert or clean
    them; touch only files your task needs.

### 2.3 Verification loop (run after every slice)

```bash
cd rust
cargo fmt --check
cargo test --workspace --all-targets
cargo test --workspace --doc
cargo clippy --workspace --all-targets          # gate: 0 warnings
cargo build -p tikz-cli                         # parity uses this binary; it can be stale
cd ..
python3 migration/parity.py                     # slice-3; add corpora as they appear
python3 migration/parity.py --corpus main
python3 migration/statement_parity.py           # once M0-2 lands
```

Log the slice through the migration skill's ledger (append only, never hand-edit JSON):

```bash
python3 /home/cachy/.zcode/skills/migrate-to-rust/scripts/telemetry.py \
  log-test-run --ok <n> --total <n>          # see --help for this environment's flags
```

For a new feature fixture: capture the legacy golden **first**, then implement Rust, then run
`migration/parity.py`. A Rust render that exits non-zero (error diagnostics) is a parity
failure even if the SVG "looks right".

### 2.4 Environment gotchas

- `CARGO_HOME=/mnt/G/Rust_Installations/cargo` (not `~/.cargo`); crate sources live under
  `$CARGO_HOME/registry/src/index.crates.io-*/`.
- eframe/egui is **0.33**: `NativeOptions { viewport: ViewportBuilder, .. }`,
  `run_native(name, opts, Box::new(|_cc| Ok(Box::new(app))))`, `eframe::Result<()>`.
- A cold workspace build/app build can exceed 300 s. Use ≥420 s timeouts.
- The repo lives on `/mnt/G` (network/Drive mount). New files can appear with a delay and the
  shell `cwd` can move between commands; prefer absolute paths.
- `tikz-cli` may be a stale scaffold after another agent's slice — rebuild before parity.
- `migration/parity.py` imports `svg_diff.py`; never shell `svg_diff.py` directly (argument-order
  and stderr traps documented in `migration/parity.py`).

---

## 3. Ordered task board

Phases are dependency-ordered, but each task is a **vertical slice**: parser → semantic → SVG →
parity, not a layer-only rewrite. Several tasks can run in parallel (marked ∥).

### Phase 0 — Harness, guardrails, decisions (start here; ∥)

- [x] **M0-1 · Add Rust CI job** — Done: 2026-11-03, c4d1b6ef27e1
  - Anchor: `.github/workflows/ci.yml` (currently Node-only; `cargo` appears only in Tauri jobs).
  - Do: add a job running `cargo fmt --check`, `cargo test --workspace --all-targets`,
    `cargo test --workspace --doc`, `cargo clippy --workspace --all-targets -- -D warnings`,
    `cargo build -p tikz-cli`, then `python3 migration/parity.py` for each corpus.
  - Done when: a PR touching only `rust/` runs the job; failures block merge.

- [x] **M0-2 · Statement-AST parity runner (`tikz-cli parse --json`)** — Done: 2026-11-03, c4d1b6ef27e1
  - Anchor: `migration/capture-statements.mjs`, `migration/expected/*.json` (11 dumps:
    figures, spans, options, full statement tree, diagnostics).
  - Target: `tikz-cli parse --json <in.tex>` + `migration/statement_parity.py`. Added
    `serde`/`serde_json` (pure Rust) to `tikz-core`/`tikz-cli` per the dep policy.
  - Done when: all 11 expected dumps match on figure count, statement kinds/order, spans, and
    diagnostic severity/span; reruns are deterministic.

- [ ] **M0-3 · Rust capability registry**
  - Anchor: `packages/core/src/capabilities/{feature-ids,matrix,registries,types}.ts` (118 IDs).
  - Target: `rust/crates/tikz-core/src/capabilities.rs` with the same IDs and per-layer status;
    a checked-in JSON export for the drift test.
  - Done when: `cargo test -p tikz-core capabilities` fails on drift; `AGENTS.md` points at the
    Rust files (update the three-file rule to name the Rust registry).

- [ ] **M0-4 · Benchmark harness (`tikz-cli bench`) + stages 2–4**
  - Anchor: `migration/golden-capture.mjs` BENCH line; `migration/BLUEPRINT.md` §5.
  - Target: `tikz-cli bench` printing the identical BENCH shape; safe-Rust, SoA/threaded, and
    PGO+LTO stages measured and logged to `migration/migration_telemetry.json`.
  - Done when: all four stages are present in the ledger with wall/per-render/RSS numbers.

- [ ] **M0-5 · Corpus expansion harness**
  - Anchor: `migration/parity.py` (`CORPORA` dict), `migration/capture-slice3.mjs`,
    `migration/golden-capture.mjs`.
  - Do: one corpus directory per feature phase (`corpus-nodes`, `corpus-arrows`, …), text-free
    where possible, with legacy goldens captured before implementation; make the runner
    auto-discover corpora instead of editing a hard-coded dict.
  - Done when: adding a corpus is data-only (drop `.tex` + golden) and CI runs all corpora.

- [ ] **M0-6 · Property/fuzz suite**
  - Target: proptest seeds for (a) parser never panics on arbitrary token soup and always
    returns diagnostics, (b) `fmt_num` stays within 5e-5 and matches JS ties, (c) coordinate
    parse/format roundtrip, (d) y-flip/bbox invariants.
  - Done when: tests are deterministic, run in CI, and a `corpus` runner renders all 526
    `test/fixtures` without panics.

- [ ] **M0-7 · Decision records D1–D3 (ask the user; do not guess)**
  - D1 shipping targets: (recommended) native desktop via eframe + optional wasm32/egui web;
    vs desktop-only and retire web.
  - D2 assistant panel: port the AI assistant to Rust (HTTP client) or drop it.
  - D3 landing: keep as plain HTML/CSS (no TS) vs Rust static-site generator.
  - Done when: a short `migration/DECISIONS.md` records the answers and this file's Phase 8 is
    updated.

### Phase 1 — Front-end / AST parity (foundation; blocks Phase 2+)

Current state: `tok.rs`, `tex_lex.rs`, `stmt_*` exist, but `semantic::compile_statement` splits
text and `semantic::pathbuild` scans statement text directly. There is no `Figure[]` facade and
no AST-driven path evaluation. Fix that first.

- [x] **M1-1 · `parse_tex(source) -> Figure[]` facade** — Done: 2026-11-03, c4d1b6ef27e1
  - Anchor: `packages/core/src/parser/index.ts` (161 LOC, 3-stage: figure scan → lex → cst-to-ast),
    `figure-scan.ts`, `transform/cst-to-ast.ts`.
  - Target: `tikz-core/src/parser/{mod,statement,unknown}.rs`; keep `Span` as byte offsets
    internally but expose a UTF-16 projection for AST-parity JSON (see trap T17).
  - Done when: `parse_tex` returns figures with inventory/begin/end spans, options span/list,
    statement list, and diagnostics; M0-2 green.

- [ ] **M1-2 · Multi-figure documents**
  - Anchor: `figure-scan.ts`, `parser/index.ts` `activeFigureId`; legacy corpus
    `multi-figure-basic`.
  - Do: remove the "multi-figure documents are not supported yet" error; support
    `activeFigureId` selection like the editor; figure-level options are per figure.
  - Done when: `multi-figure-basic` renders all figures and parity passes.

- [ ] **M1-3 · AST-driven path parsing (replace text scanning)**
  - Anchor: `transform/cst-to-ast.ts`, `ast/types.ts` path-item half, `items_ops.rs` /
    `item_types.rs` payloads (already ported but unused by the evaluator).
  - Do: populate `PathItem` AST nodes for every path keyword; make `semantic/pathbuild.rs`
    consume the AST instead of re-reading source. Keep source spans exact.
  - Done when: semantic tests use AST fixtures, and behavior for existing slice-3 is unchanged
    (parity 3/3 still).

- [ ] **M1-4 · Full statement dispatch**
  - Anchor: `ast/types.ts` 39 kinds; `transform/cst-to-ast.ts`; `domains/statements/parse.ts`.
  - Kinds: `Path`, `Scope`, `Foreach`, `Node`, `TikzStyle`, `TikzSet`, `TikzLibrary`,
    `Pgfkeys`, `PgfMath`, `MacroDefinition`, `MacroCommandDefinition`, `MacroAlias`,
    `Colorlet`, `DefineColor`, `Unknown`; node parts and `\nodepart`.
  - Done when: every kind maps to a Rust enum variant with spans/options, and unknowns become
    `UnknownStatement` + diagnostic (never a dropped statement).

- [ ] **M1-5 · UTF-8 / TeX text in source**
  - Anchor: `parser/tex-lexical.ts`, `shared.ts`; current `tok.rs` treats non-ASCII as
    `StraySymbol` and documents UTF-8 as out of scope.
  - Do: accept UTF-8 inside node text/braces, keep `\{`/`\}`/`\(`/`\)` inert, preserve comment
    masking (including the 1-char lookbehind quirk at `\end`), and define byte↔UTF-16 span
    conversion for parity output.
  - Done when: fixtures with accented/CJK/emoji node text parse with the same statement spans as
    legacy (UTF-16 code units) and no character splitting.

- [ ] **M1-6 · Diagnostics parity**
  - Anchor: `diagnostics/collect.ts`, `diagnostics/types.ts`, `transform/unknown.ts`.
  - Target: stable diagnostic codes, severity, spans; document-level vs figure-level.
  - Done when: M0-2 expected dumps match diagnostics exactly; unsupported constructs are never
    silent.

- [ ] **M1-7 · Incremental parse session** (needed by the editor, blueprint Tier 2)
  - Anchor: `parser/incremental.ts` (1,076 LOC), `semantic/incremental.ts` (1,262).
  - Target: bounded re-lex/statement-anchored reparse with patch metadata.
  - Done when: `test/semantic/incremental*.spec.ts` behaviour is ported and a benchmark shows
    edit latency bounded by the edited statement, not document size.

- [ ] **M1-8 · Port parser/front-end test suites**
  - Anchors: `test/semantic/*` parsing-adjacent specs, `test/differential/*`,
    `test/fixtures/*`.
  - Done when: the Rust corpus runner covers the same fixtures and the differential suites have
    Rust equivalents (or documented drop reasons).

### Phase 2 — Semantic parity (geometry, styles, nodes, graphs)

Order matters: defaults/style → transforms → coordinates → paths → nodes/shapes → foreach/scope
→ definitions/pgfmath → decorations → shadings/patterns/clip → graphs/trees.

- [ ] **M2-1 · Defaults + full style option set**
  - Anchor: `semantic/style/defaults.ts`, `apply-flag.ts`, `apply-kv.ts` (1,420 LOC), `colors.ts`,
    `arrows.ts`; blueprint row 15 (keys are `trim().toLowerCase()` and collisions are by design).
  - Do: port every default constant (`lineWidth 0.4`, `DEFAULT_TEXT_FONT_SIZE 9.96264`, font
    stacks, `tiny…Huge` scales, node inner/outer sep, …), all option flags/keys, `.style`
    expansion, `/tikz/` keys, and scope-local option inheritance.
  - Done when: `styles`, `coords`, `paths` fixtures render (once nodes/text allow counts) and a
    style unit suite matches legacy option application order.

- [ ] **M2-2 · Full affine transforms**
  - Anchor: `transform/`, `semantic/style` transform keys, `test/semantic/transform-frames.spec.ts`,
    `turn-coordinate-frames.spec.ts`.
  - Do: full 6-value matrix, `xslant`/`yslant`, rotate-about, scale/shift ordering, nested
    frames/turn frames.
  - Done when: transform frame tests ported; nested-scope transforms match legacy.

- [ ] **M2-3 · Coordinate systems**
  - Anchor: `coords/` (548), `semantic/coords/` (1,378), `domains/coordinates/parse.ts`,
    `test/semantic/coordinate-pgf-oracle.spec.ts`.
  - Do: absolute/relative, `+(…)`/`++(…)`, named coordinates, `calc`, `let`, intersections,
    polar, `turn`, PGF arithmetic, units/`div`/`mod`, `veclen`, etc.
  - Done when: coordinate oracle specs ported and `coords` fixture reaches element parity.

- [ ] **M2-4 · Full path evaluation**
  - Anchor: `semantic/path/` (30 files, 16,422 LOC), `semantic/path/graph.ts`, `to-operation.ts`,
    `test/semantic/path-*.spec.ts`.
  - Do: `.. controls .. and ..`, `to` with bend/looseness/in/out (PGF fixed-point math, blueprint
    row 26), rounded corners, `parabola`, `sin`/`cos`, `plot`, `svg`, `decorate`, `close`/`cycle`
    corner repair, intersections, shorten.
  - Done when: `paths` fixture parity and path specs ported; no more "drawn as a straight line"
    warning for supported syntax.

- [ ] **M2-5 · Nodes, anchors, shapes**
  - Anchor: `semantic/nodes/` (18 files, 13,653 LOC): `layout.ts`, `shape-geometry.ts`,
    `anchors.ts`, `matrix.ts`, `evaluate.ts`, `multipart.ts`; `test/semantic/node-*`,
    `nodes-shapes`, `path-attached`.
  - Do: plain-text fallback metrics (`charWidth = 0.7·fontSize`, `lineHeight = 1.05·fontSize`,
    `baselineY = −0.28·fontSize`, `midLineY = −0.065·fontSize`), box layout, 30+ shapes
    (rectangle, rounded/chamfered, circle, ellipse, diamond, trapezium, semicircle, triangle,
    kite, dart, sector, cylinder, regular polygon, star, cloud, starburst, signal, tape,
    callouts, single/double arrow, cross/strike-out, splits, magnifying glass), anchors, border
    intersections, multipart, matrix, fit, label/pin adornments.
  - Done when: `nodes`, `shapes-new`, `node-graph` fixtures render with equal element counts and
    node-shape PGF oracle specs ported.

- [ ] **M2-6 · `\foreach` expansion**
  - Anchor: `foreach/` (9 files, 3,504 LOC), `semantic/foreach`, `test/semantic/macros-foreach`.
  - Do: header parsing (already started), list expansion, nested loops, options, path/node
    operations, provenance metadata on generated elements.
  - Done when: foreach fixtures render; expansion order/provenance matches legacy.

- [ ] **M2-7 · Scopes + style inheritance**
  - Anchor: `semantic/evaluate.ts` scope handling, `test/semantic/context-state.spec.ts`,
    `style-chain-sharing`.
  - Do: `\scope`/`\begin{scope}` bodies inherit figure + enclosing scope state; transformations
    compose. (Today figure options are not seen by nested scope bodies because scopes are
    unsupported.)
  - Done when: nested scope fixtures match legacy state and transforms.

- [ ] **M2-8 · Definitions, pics, macros, conditionals**
  - Anchor: `semantic/style/` definitions, `semantic/pics/`, `macros/` (856),
    `conditionals/expand.ts` (446), `test/semantic/pics`, `macro-*`, `symbol-*`,
    `required-tikz-libraries`.
  - Do: `\tikzset`, `.style`, `.pic`, `pics/name/.style`, `\pgfkeys`, `\colorlet`,
    `\definecolor`, macro defs/aliases/commands, `\if…`, library requirements, symbol resolver.
  - Done when: style/definition fixtures and specs pass; pics render.

- [ ] **M2-9 · PGF math**
  - Anchor: `semantic/pgfmath/` (983), `test/semantic/pgfmath.spec.ts`, blueprint row 25.
  - Do: full expression grammar/functions, `\pgfmathsetmacro`, seed/random commands, output
    formatting thresholds (`≤1e-12 → "0"`, near-integer → integer, else `toFixed(12)` stripped).
  - Done when: pgfmath oracle specs ported; foreach steps and dimensions consume the same values.

- [ ] **M2-10 · Decorations**
  - Anchor: `semantic/decorations/engine.ts` (1,563), `test/semantic/decorations.spec.ts`.
  - Do: pathmorphing, pathreplacing, fractals, shape marks, footprints, shape backgrounds.
  - Done when: decoration fixtures at parity.

- [ ] **M2-11 · Shadings, patterns, shadows, clipping, bbox**
  - Anchor: `semantic/style` paint, `svg/patterns/`, `svg/shading`, blueprints rows 22/29;
    feature IDs `path_shading`, `path_patterns`, `path_shadows`, `path_clipping`,
    `use_as_bounding_box`, `backgrounds_library`.
  - Do: real shading/pattern scene representation, clipping chains, `use as bounding box`,
    backgrounds library.
  - Done when: feature fixtures at parity (text-free).

- [ ] **M2-12 · Graphs, edges, trees**
  - Anchor: `semantic/path/graph.ts` (3,789), `semantic/path/to-operation.ts` edge logic,
    `test/semantic/graphs.spec.ts`, `trees.spec.ts`, `test/semantic/deferred-path-node-tree-state`.
  - Do: graph spec parsing/layout, `edge`, `child`, `from parent`, tree layout keys, level/every
    child styles, anchors, auto naming, deferred hooks.
  - Done when: `node-graph` and tree fixtures at parity.

- [ ] **M2-13 · Complete the scene model**
  - Anchor: `semantic/evaluate.ts` scene output, `svg/emit.ts` element kinds.
  - Do: `Text` wrappers, `Group`/transform elements, `Image`, `Clip`, z-order/defs references;
    stop representing `circle` as two-arc path if legacy uses `<path>`/`<ellipse>` differently
    per op.
  - Done when: scene element kinds cover every legacy emitter branch.

- [ ] **M2-14 · SoA mirrors + incremental semantic**
  - Anchor: blueprint §5 stage 3, `semantic/incremental.ts`.
  - Do: `x[]/y[]` path sample layout, affine/bbox vectorization, statement-anchored re-eval.
  - Done when: benchmark shows the SoA win and incremental edits avoid full re-eval.

### Phase 3 — SVG emitter parity

- [ ] **M3-1 · Verify the existing emission contract**
  - Anchor: `svg/emit.ts`, `svg/viewbox.ts`, `svg_numbers.rs`, `svg_emit.rs`,
    blueprint §1.4 rows 19–24.
  - Do: audit every attribute/order/escape/y-flip/edit against the contract; add regression
    tests for ties, negative zero, non-finite, empty scenes.
  - Done when: contract table is covered by unit tests.

- [ ] **M3-2 · All element emitters**
  - Do: paths (lines/cubics/arcs/rounded corners/close), ellipses, text wrappers, images, groups,
    clip paths, defs, `data-source-id` hooks, `data-*` attributes in the exact order.

- [ ] **M3-3 · Arrow tips**
  - Anchor: `svg/arrows/` (`metrics.ts` 368, `shapes.ts` 223, `render.ts` 249, `shorten.ts` 185,
    `preview.ts` 126), blueprint row 30.
  - Do: all families (`latex`, `stealth/triangle`, `kite`, `cm-rightarrow`, `bar/tee-barb`,
    `straight-barb`, hooks, `arc-barb`, `square/circle`, `*-cap`, rays, `implies`), defaults
    (`DEFAULT_ARROW_LENGTH 8`, `DEFAULT_ARROW_WIDTH 6`, `STEALTH_INSET 0.325`), shortening
    (endpoint-only, **end first**, tail tips only shorten the **last subpath**), markup attribute
    order.
  - Done when: arrow fixtures at parity; `npm run compare:arrows` oracle cases ported.

- [ ] **M3-4 · Shading/pattern defs + ID hashing**
  - Do: FNV-1a over **UTF-16 code units** (`Math.imul(hash, 0x01000193) >>> 0`, base36 +
    `-${len.toString(36)}`), shading constants (`PGF_SHADE_SCALE_FACTOR 0.01992528`, canonical
    size 100.375), stop ratios, ball focus, pattern tiles and `patternTransform` phase
    (`vb.y·2 + vb.height`).
  - Done when: defs IDs and geometry byte-match goldens.

- [ ] **M3-5 · Text emitters**
  - Plain `<text>/<tspan>` first (multiline `dy = −((n−1)·1.15)/2em`, then `1.15em`; fonts
    serif/sans/mono stacks), then TeX glyph `<path data-tex-*>` (depends Phase 4).
  - Done when: plain-text node wrappers at parity; glyph parity after Phase 4.

- [ ] **M3-6 · SVG model/patch/diff/namespace** (only if the editor uses incremental SVG)
  - Anchor: `svg/index.ts` model/serialize/diff, `svg/patch.ts`, `svg/namespace.ts`.
  - Done when: Rust emits the same model and patch ops, or D1 records that incremental SVG is
    dropped with a measured replacement.

- [ ] **M3-7 · Corpus gate**
  - Done when: main corpus 11/11 (after text engine) and every feature corpus green.

### Phase 4 — Text engine (largest block; start after M1-1/M1-3; ∥ with Phase 2/3)

- [ ] **M4-1 · Metrics + font provider + OT1 shaper**
  - Anchor: `text/tex/` (186 files, 46,543 LOC) metric providers, `DEFAULT_COMPUTER_MODERN_TEXT_FONTS`.
  - Do: Computer Modern metrics, glyph widths/heights/depths, font stacks, ligatures/kerning.
  - Done when: metric oracle matches legacy on a glyph fixture set.

- [ ] **M4-2 · TeX IR + parser**
  - Anchor: `text/tex/ir.ts` (6,032), parser modules.
  - Do: hlist/vlist/box/math nodes, groups, macros, fonts, spacing primitives.

- [ ] **M4-3 · Paragraph layout + Knuth-Plass + hyphenation**
  - Anchor: `text/knuth-plass/` (20 files, 13,909), `languages/data/hyph-en-us.patterns.ts`.
  - Do: DP line breaking, glue/penalty/badness, hyphenation patterns, alignment (left/right/
    center/justify), paragraph width.
  - Done when: `compare:paragraph-breaks` / paragraph matrix cases match.

- [ ] **M4-4 · Math**
  - Anchor: `text/tex/math/{parser,layout,inline-provider}.ts` (6,750 + 8,005 + 2,482).
  - Do: math parser, layout, inline/display math, aligned environments, scripts, delimiters,
    Dirac notation, math fonts.
  - Done when: `compare:tex-math`, display/inline math fidelity cases match within tolerance.

- [ ] **M4-5 · Inline items, lists, tables, graphics, transforms**
  - Anchor: `layout-inline-items.ts` (2,548), `vlist/spacing.ts` (2,241), tabular/booktabs,
    `includegraphics`, `text_transform_boxes`, scientific notation, preformatted/alltt.
  - Do: inline item flow, lists, tabular, image layout/resolution, box transforms.
  - Done when: text/shape fixtures at parity and `graphicsResolver` ported.

- [ ] **M4-6 · Editor hitmap / carets**
  - Anchor: `text/knuth-plass/editor/hitmap.ts` (4,507), `layout-context.ts`.
  - Do: caret positions, hit testing, selection ranges in rendered text.
  - Done when: caret/hitmap invariants ported (only needed if canvas text editing is kept).

- [ ] **M4-7 · Glyph SVG emission + render scope**
  - Anchor: `svg/emit.ts` glyph branches (`data-tex-font`, `data-tex-glyph`), text render scope,
    cooperative work.
  - Do: glyph outlines → `<path>`, baseline/transform math, text scope semantics (or a documented
    sync simplification if parity holds).
  - Done when: glyph-inclusive goldens match (the only point where main-corpus count parity is
    reachable).

- [ ] **M4-8 · Port text test/oracle scripts**
  - Anchors: `test/tex-shaping.spec.ts`, tex math/paragraph/vlist suites, `scripts/compare-tex-*`.
  - Done when: a representative subset is ported to Rust or kept as an external Node oracle
    until Phase 9 (document which).

### Phase 5 — Editing core (33.6K + 11.2K LOC)

Legacy split: `editing/` is the editor substrate (tree/position/ownership/transaction/
operations/commands/input); `edit/` is the action/property layer.

- [ ] **M5-1 · `editing/` substrate**
  - Anchor: `editing/{tree,position,ownership,transaction,operations,commands,input}`.
  - Do: source-position mapping, ownership resolution, transaction model, command/operation
    types, pointer/input resolution (2,040 LOC `input/resolve.ts`).
  - Done when: substrate unit tests ported; no `unsafe`, no DOM assumptions.

- [ ] **M5-2 · Edit actions**
  - Anchor: `edit/actions.ts` (1,252, dispatcher ~:246, switch ~:274), `edit/actions/` 16 modules
    (~10,199 LOC), `edit/{apply,session,analysis,action-availability}.ts`.
  - Do: all `EditAction` kinds — set-property, move, resize, rotate, reorder, delete,
    group/ungroup, paste/duplicate, repeat, path editing, path-attached nodes, node positioning,
    tree child, matrix structure, adornment; `SourcePatch[]` output; edit analysis/session.
  - Done when: `test/edit/*` (10 specs) behaviour ported; every action has roundtrip tests.

- [ ] **M5-3 · Property system + inspector**
  - Anchor: `edit/property-*.ts` (1,656 + 1,233 + 806 + 650), `option-mutations.ts` (744),
    `styles-cascade.ts` (1,243), `edit/inspector/` (21 files, 5,811).
  - Do: property registry/targets, read/write planners, option mutation, style cascade, inspector
    metadata for every supported element/statement.
  - Done when: inspector property tests ported; writes produce minimal `SourcePatch[]`.

- [ ] **M5-4 · Snapping**
  - Anchor: `edit/snapping/` (9 files, 2,558), public exports in `packages/core/src/index.ts`.
  - Do: grid/object snapping, translation, handle positions, keyboard nudge, snap context.
  - Done when: snapping unit tests ported; canvas uses it (Phase 7).

- [ ] **M5-5 · Source writer / formatting**
  - Anchor: `edit/writer/` (8 files, 910), `source-format.ts` (434), `path-pieces.ts`.
  - Do: exact source rewriting/formatting used by actions and inspector.
  - Done when: write→parse→render roundtrips are stable and diffs are minimal.

- [ ] **M5-6 · Element templates + remaining actions**
  - Anchor: `element-templates.ts` (443), `path-editing.ts`, `statement-ops.ts`,
    `actions/resize-element.ts` (2,183), `rotate-element`, `reorder-elements`,
    `matrix-structure`, `tree-child`, `adornment-*`, `group-ungroup`, `repeat`.
  - Done when: `test/editing/*` (14 specs) and action specs ported.

- [ ] **M5-7 · Undo/redo + history**
  - Anchor: `packages/app/src/store/reducer.ts` (~:1008–1080, 1280, 1318),
    `store/types.ts` (~:83–116), blueprint row 5.
  - Do: snapshot (`sourceBefore`/`sourceAfter`) + forward `SourcePatch[]` + `mergeKey` drag
    coalescing in `tikz-app::history`.
  - Done when: undo/redo/coalescing tests ported; drag produces one history entry.

- [ ] **M5-8 · Incremental integration**
  - Do: wire M1-7/M2-14 into the editor loop; re-render only dirty statements/figures.
  - Done when: measured edit latency on `test/papers/*.tex` is bounded by edited region.

### Phase 6 — Beamer (26K LOC)

- [ ] **M6-1 · Document kind / scan / prepare**
  - Anchor: `beamer/{index,scan,prepare}.ts`, `document/{kind,root-id,masking}.ts`.
  - Do: `\documentclass{beamer}` detection, frame inventory, root IDs.
  - Done when: document-kind/root-id tests ported.

- [ ] **M6-2 · Theme resolution + rendering**
  - Anchor: `beamer/theme/` (24 files, 7,698), `beamer/render.ts` (4,778).
  - Do: theme colors, fonts, inner/outer themes, templates.
  - Done when: theme fixtures at parity.

- [ ] **M6-3 · Frame flow and features**
  - Anchor: feature IDs `beamer_frame_flow`, `beamer_titlepages`, `beamer_list_templates`,
    `beamer_frame_shrink`, `beamer_tcolorbox`, `beamer_hyperlinks`, `beamer_manual_bibliography`,
    `beamer_footnotes`, `beamer_speaker_notes`, `beamer_float_captions`,
    `beamer_frame_continuations`, `beamer_overlay_pictures`, `beamer_source_placeholders`,
    `beamer_direct_resize`.
  - Do: headings/metadata chrome/contents/overlays, titlepages, lists, blocks, boxes, notes,
    bibliography, footnotes, captions, continuations, overlay TikZ pictures, columns/images/
    spacing.
  - Done when: beamer corpus at parity.

- [ ] **M6-4 · Slide manager + Beamer document inspector**
  - Anchor: `beamer_slide_manager`, `beamer_document_inspector`, `beamer_builds`.
  - Do: frame insert/reorder/duplicate/paste/delete, theme/page/header/footer/colors/lists/image
    replace source writes, build/overlay inventory + timing edits.
  - Done when: inspector/source-write tests ported.

- [ ] **M6-5 · Presentation / compiled slide**
  - Anchor: `packages/app/src/presentation/` (14 files, 1,348), `CompiledSlideModal`,
    `apps/desktop/src-tauri/src/presentation.rs`.
  - Do: page rasterization/presentation mode in Rust (or SVG-based renderer), navigation.
  - Done when: presentation smoke test passes (window opens, pages advance).

### Phase 7 — App UI (egui, 81.8K LOC)

Do not try to port every panel before Phase 5 exists. Start with state/menus/files; add panels as
their core features land.

- [ ] **M7-1 · Store / workspace / document sessions**
  - Anchor: `packages/app/src/store/` (9 files, 3,337), `compute*.ts`.
  - Do: document model, active figure, selections, tool mode, canvas prefs, workspace list,
    persistence, reducer-equivalent state transitions.
- [ ] **M7-2 · Menus, commands, shortcuts, context menus**
  - Anchor: `app-menu/` (5, 1,027), `context-menu/` (3, 665), `ui/editor-commands.ts`,
    `editor-command-runtime.ts`, `dock-panel-menus.tsx`.
  - Do: native/app menu bar, command registry, keybindings, context targets.
- [ ] **M7-3 · Source panel**
  - Anchor: `ui/source-panel/` (2,028 + helpers), `lang-tikz` syntax highlighting,
    `completion/` (507), diagnostics.
  - Do: editor with highlighting, completion/hover, error squiggles, search/replace if present.
- [ ] **M7-4 · Canvas**
  - Anchor: `ui/canvas-panel/` (3,672 CanvasPanel, 2,954 text edit session, 2,294 drag controller,
    1,454 selection state, 1,286 view, 1,196 tool interactions, 992 overlays, 955 text machine).
  - Do: selection/multi-select, drag/move, resize/rotate handles, snapping, marquee, overlays,
    inline text editing, context menus, viewport restore/touch rules.
- [ ] **M7-5 · Inspector panels**
  - Anchor: `ui/inspector-panel/` (2,046 + 1,968 helpers), `ColorPicker.tsx` (1,551),
    `color-picker-grayscale`, Beamer inspector.
  - Do: property rows grouped by element/statement, color picker, shape/path-specific controls,
    source-backed writes (Phase 5).
- [ ] **M7-6 · Objects / builds / notes / slide manager / dock layout**
  - Anchor: `ui/{objects-panel,builds-panel,notes-panel,slide-manager,theme-gallery,examples}`,
    `DockLayout.tsx`, `dock-layout-validation.ts`.
- [ ] **M7-7 · Files, workspaces, linked sync, recovery**
  - Anchor: `ui/close-guard.ts`, `store/{workspace-*,document-sessions}`, `linked-file-sync`,
    `apps/desktop/src-tauri` watchers/recents/file associations.
  - Do: open/save/save-as, recents, autosave/recovery, unsaved-changes guard, linked `.tex`
    revision sync, local assets, drag-drop, file associations.
- [ ] **M7-8 · Export + presentation**
  - Anchor: `packages/core/src/export/` (pdf 29, png 29, svg 32, standalone-latex 365),
    `apps/desktop/src-tauri/src/latex_pdf.rs`.
  - Do: SVG/PNG/PDF/standalone-LaTeX/minimal-TikZ artifacts; print; presentation.
  - Note: PDF may shell out to a TeX installation (current Tauri behaviour) — record the chosen
    approach in D1.
- [ ] **M7-9 · Assistant panel** (only if D2 says keep)
  - Anchor: `ui/AssistantPanel.tsx` (1,196), `assistant-*`, `apps/desktop/src-tauri/src/assistant.rs`.
  - Do: Rust HTTP client, session lifecycle, attachments, tool handlers.
- [ ] **M7-10 · Platform abstraction**
  - Anchor: `packages/app/src/platform/{types,current,storage}.ts`, web/desktop adapters.
  - Do: persistence, clipboard (custom text formats/bundles), file dialogs, asset reads,
    arXiv fetch, watches.
- [ ] **M7-11 · Settings, themes, colors, dev/about/updater**
  - Anchor: `settings/`, `colors/`, `ui/theme-gallery`, `AboutModal`, `DevPanel`,
    `apps/desktop/src-tauri/src/{update_preferences,portable,windows_accent,snap_layout}.rs`.
- [ ] **M7-12 · Port `test/web` suites**
  - Anchor: 109 specs in `test/web/` (canvas geometry/drag/text machine, file races, menus,
    panels, capabilities).
  - Done when: Rust UI tests/e2e cover the same behaviours; keep a mapping table.

### Phase 8 — Shells, packaging, decommission prep

- [ ] **M8-1 · Desktop shell**
  - Decide D1. Recommended: single eframe/egui binary. Port the 57 Tauri commands and native
    integrations (menus, dialogs, recents, watchers, snap layout, Windows accent, portable mode,
    presentation, assistant, latex PDF, update preferences) to Rust crates; keep the Tauri crate
    only if the user prefers a webview.
  - Done when: desktop bundle builds/signs/updates without a JS frontend.

- [ ] **M8-2 · Web shell** (if D1 keeps it)
  - Target: wasm32 build of the egui app (`apps/web` becomes a thin `index.html` + wasm loader);
    implement web platform APIs (File System Access, clipboard, downloads, OPFS/workers, asset
    reads).
  - Done when: Playwright e2e (82 files) is replaced by Rust/wasm e2e or a reduced accepted set.

- [ ] **M8-3 · Landing**
  - Convert `apps/landing` (25 files, 5,365) to plain HTML/CSS (no TS) or a Rust SSG, per D3.
  - Done when: `npm run build:landing` is replaced by a non-Node build in CI.

- [ ] **M8-4 · CI/CD**
  - Replace `lint:prod`, `typecheck`, `test`, `test:e2e`, `test:desktop:e2e` with Cargo
    equivalents; build desktop bundles (Windows/macOS/Linux), wasm, signing, updater.
  - Done when: all workflows run without Node for the app (release notes/static deploy may keep
    Node tooling only if unavoidable).

- [ ] **M8-5 · Delete `lezer-*`, `lang-tikz`, `tex-fuzz`**
  - `lezer-tex` 379, `lezer-tikz` 202, `lang-tikz` 481, `tex-fuzz` 5,439 LOC.
  - Replace grammar-generated parsing with the Rust front-end; port syntax highlighting to
    `egui`/syntect or a hand-written highlighter; port fuzz targets to `cargo-fuzz`/proptest.
  - Done when: packages are gone and CI has Rust fuzz targets.

- [ ] **M8-6 · Remove TypeScript packages/apps**
  - Delete `packages/*`, `apps/*/src`, `test/*.spec.ts`, `tsconfig*`, `eslint.config.mjs`,
    `vitest.config.ts`, `knip.jsonc`, scripts that invoke TS, and npm devDependencies.
  - Update `AGENTS.md`, `README.md`, `DEVELOPMENT.md`, `.gitignore`, `.gitattributes`.
  - Done when `git grep -n "\.tsx\|\.ts\b"` finds no build/test-critical references.

### Phase 9 — Final verification & handover

- [ ] **M9-1 · Full parity matrix** — every corpus, feature matrix green, no diagnostics on
  reference documents.
- [ ] **M9-2 · Performance** — bench stages 2–4, PGO/LTO, SoA/threads measured and compared to
  `legacy_baseline` (624.4 ms / 110 renders, 287 MB peak RSS).
- [ ] **M9-3 · Coverage** — ≥95% branch (or documented `cargo-llvm-cov` exception); record in
  telemetry.
- [ ] **M9-4 · Archive the oracle** — tag the last TypeScript commit (e.g. `legacy-ts-oracle`),
  then delete it from `master` only after M9-1..3 pass.
- [ ] **M9-5 · Migration finale artifacts** — story chapters, spotlight, infographic
  (`telemetry.py render-infographic`), executive summary, per the migration skill.

---

## 4. Parity traps (copy these verbatim; they flip goldens)

These come from `migration/BLUEPRINT.md` §1.3–1.4 and were verified against goldens. Re-check
the source before re-deriving.

| # | Trap | Rule |
| :-: | :--- | :--- |
| T1 | Text-bearing goldens | Until Phase 4, main-corpus element counts cannot match. Use text-free corpora; never accept partial output as parity. |
| T2 | Number formatting | `toFixed(4)` → `Number().toString()`; `<1e-9` and non-finite → `"0"`; ECMAScript ties round to the **larger** integer (Rust `{:.4}` rounds half-even). |
| T3 | viewBox | pad = 12 pt, plus `half stroke width` per side; empty fallback exactly `-12 -12 124 124`. |
| T4 | y-flip | `svgY = 2·vb.y + vb.height − y`; group transform `(a,−b,−c,d,e,f) → (a,−b,−c,d,e′,k)`, `k = 2·vb.y + vb.height`. |
| T5 | Attr order | `stroke, fill, fill-rule (evenodd only), stroke-width, linecap=butt, linejoin=miter, opacities, dasharray, dashoffset (only when `abs(dashoffset) > 1e-6`)`. |
| T6 | Escaping | text: `& < >`; attributes additionally `"`. Do not over-escape. |
| T7 | Defs IDs | FNV-1a over **UTF-16 code units** (`Math.imul(h,0x01000193) >>> 0`), base36 + `-${len.toString(36)}`. |
| T8 | Node text | box line pitch `1.05·fontSize`; multiline tspan step `1.15em`; `DEFAULT_TEXT_FONT_SIZE = 9.96264`; `\tiny…\Huge` = `[0.5,0.7,0.8,0.9,1,1.2,1.44,1.728,2.074,2.488]`. |
| T9 | `to`/bend math | PGF fixed point: `divisor = trunc(round(largest/len·65536)/255)`; `pgfDistance = 16·trunc(16·round(largest·65536)/divisor)/65536`; `baseDistance = pgfDistance·0.3915`; default bend 30°. Do not re-derive with trig. |
| T10 | Option keys | `trim().toLowerCase()`; case-different keys collide by design. Keep it. |
| T11 | Arrow sniffing | Dual-sniff shorthand `-{…}` vs graph-edge syntax with fixed precedence; shortening processes the **end first**, and only the **last subpath** is shortened when tail tips exist. |
| T12 | Comment masking | Length-preserving masking with the 1-char lookbehind quirk at the `\end` boundary; comments in path bodies survive as `PathComment` with exact spans. |
| T13 | Option splitting | Three independent depth counters `( { [`, and `[` counts **only at brace depth 0**. Braced sublists push/pop independently. |
| T14 | Env charset | Figure environments only `tikzpicture`, `tikzpicture*`, `tikzlayer`; env names over `[A-Za-z0-9*@-]`, hand-humped, no regex. |
| T15 | Ellipse nodes | Draw box inflated by √2; anchors `anchorRadius = max(hw,hh)`; outer sep = `lineWidth/2`. |
| T16 | Shading/patterns | `PGF_SHADE_SCALE_FACTOR = 0.01992528`, canonical 100.375 (half 50.1875), ball focus ±10.0375; pattern `mm = 2.84527559055`, phase `vb.y·2 + vb.height`; tile geometry per blueprint row 29. |
| T17 | Offsets | Legacy spans are **UTF-16 code units**; Rust scanners are byte offsets. They coincide only for ASCII. Convert at the AST-parity boundary and for any text-bearing span; never assume equality once M1-5 lands. |
| T18 | Precision | `f64` everywhere; never `f32`. One ulp changes the 4th decimal. |

Reference commands while the legacy code still exists:
`npm run compare:renderers -- --input <file.tex>` and `npm run compare:pgf-docs -- --source-file …`.
Use `pgf-docs/`/`pgf-src/` to settle semantics, not guesses.

---

## 5. Suggested next five tasks

1. **M0-1** Rust CI job — stops regressions immediately.
2. **M0-2** `tikz-cli parse --json` + statement parity — locks the front-end before it changes.
3. **M0-3** Rust capability registry — makes “what is left” machine-checkable.
4. **M1-1/M1-2** parser facade + multi-figure — unblocks every semantic fixture.
5. **M0-5** corpus expansion harness — every later task needs a fixture/golden pair.

After those, pick one vertical feature (recommended: **nodes + plain text**, then **arrows**,
then **`\foreach`**), because they unlock the most main-corpus fixtures and the canvas UI.

---

## 6. Package disposition (for Phase 8)

| Legacy path | Replacement | Status |
| :--- | :--- | :--- |
| `packages/core` | `rust/crates/tikz-core` | in progress (~9.5K of 205K) |
| `packages/app` | `rust/crates/tikz-app` (egui) | shell only (~1.1K of 82K) |
| `packages/lezer-tex`, `lezer-tikz` | Rust parser (`tikz-core::parser`) | delete in M8-5 |
| `packages/lang-tikz` | egui syntax highlighter / syntect | delete in M8-5 |
| `packages/tex-fuzz` | `cargo-fuzz` / proptest targets | delete in M8-5 |
| `apps/desktop/src` (React) | egui UI | delete in M8-6 |
| `apps/desktop/src-tauri` | Rust native shell/integrations or eframe | port in M8-1 |
| `apps/web/src` (React) | wasm/egui shell (D1) | delete/rebuild in M8-2 |
| `apps/landing` | static HTML/CSS or Rust SSG (D3) | M8-3 |
| `test/**/*.spec.ts` | Rust unit/integration/e2e | port per phase |
| `migration/*.mjs`, `migration/*.py` | keep as oracle until M9-4 | support tooling, not product |

---

## 7. When blocked

- Semantic question → check `pgf-docs/`, `pgf-src/`, then the legacy implementation; record the
  decision in the task body and (if architectural) in `migration/BLUEPRINT.md`'s follow-up
  section — do not rewrite the historical audit.
- Parity mismatch → reproduce with one fixture, dump both SVGs, compare element counts first,
  then numbers, then attributes. Do not “fix” the golden.
- Missing environment/tool → record the exact command and error in the task, mark `[!]`, and move
  to an unblocked task. Do not install system packages without asking.
- Ambiguous product scope (web, assistant, landing) → D1–D3; ask the user, do not guess.