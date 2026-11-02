# tikz-editor — Rust workspace

Safe-Rust port of the `tikz-editor` pipeline (`#![forbid(unsafe_code)]` in every
crate root). The TypeScript implementation stays in the repository as the A/B
parity oracle; see `migration/BLUEPRINT.md` for the audit, the emission
contract and the deferred subsystems.

## Crates

| Crate | Role |
| :--- | :--- |
| `tikz-core` | Front-end scanning, semantic evaluation (`parse → style → geometry → Scene`), SVG emitter, `render_tex` facade, scene + geometric hit testing. |
| `tikz-cli` | Headless `render <in.tex> <out.svg>` used for golden parity checks (exit `3` = unsupported TikZ, diagnostics on stderr; warnings still render). |
| `tikz-app` | egui/eframe editor shell: source editor, canvas with pan/zoom and picking, inspector, diagnostics. |

## Running the editor

```bash
cd rust
cargo run -p tikz-app -- ../migration/corpus-slice3/geometry.tex   # optional file argument
```

Keys and gestures: drag to pan, scroll (or pinch) to zoom, click an element to
select it, `Fit view` frames the drawing, `Ctrl+S` saves, `Ctrl+N` starts a new
document, `Esc` clears the selection. Files can also be dropped onto the window.

`Copy SVG` in the inspector writes the SVG for the current source to the
clipboard.

## Renderer surface

Supported today:

* **Statements** — `\draw`, `\path`, `\fill`, `\filldraw`, `\shade`, `\shadedraw`
  (everything else is reported, never silently dropped).
* **Coordinates** — Cartesian `(x,y)`, polar `(30:2)`, arithmetic
  (`+ - * /`, parentheses), units `cm`/`pt`/`mm`/`in`/`bp`, relative `+`/`++`.
* **Path operations** — `--`, `|-`, `-|`, `cycle`, `rectangle`, `circle`
  (`(r)` and `[radius=…]`), `ellipse`, `arc`, `grid` (`[step=…]`).
* **Options** — colour names, `name!pct` mixes and hex; `color=`/`draw=`/`fill=`;
  `line width=` and the width keywords; `dashed`/`dotted`/`densely …`/`loosely …`
  and `dash pattern=`; `opacity`, `draw opacity`, `fill opacity`; `even odd rule`;
  `scale`/`xscale`/`yscale`/`shift`/`xshift`/`yshift`/`rotate`/`rotate around`;
  `radius`/`x radius`/`y radius`; `start angle`/`end angle`.
* **Multi-figure documents** — not yet (reported as an error, so no figure is
  silently dropped).
* **Still pending** — nodes and the TeX text engine, arrows, `to`/`bends`,
  `\tikzset`/`.style` definitions, `\foreach`, `\scope`, patterns and shadings.

Unsupported statements, options and path items become typed issues. Errors stop
the document; **warnings render the rest of the figure** and are listed in the
app's diagnostics panel and on the CLI's stderr.

## Verification

```bash
cd rust
cargo test --workspace --all-targets && cargo test --workspace --doc
cargo clippy --workspace --all-targets

# A/B parity against the legacy TypeScript renderer
cargo build -p tikz-cli
python3 ../migration/parity.py --corpus slice3   # draw-only fixtures
python3 ../migration/parity.py --corpus main     # the 11-document corpus
```

Current state: 149 unit/integration tests plus 44 doc-tests, 0 clippy warnings.
`--corpus slice3` is **3/3 fixtures byte-identical** to the legacy emitter
(max absolute error `0`, epsilon `1e-3`); `--corpus main` is **1/11** — the
remaining documents need nodes/text (element counts differ), multi-figure
support, `.style` definitions and arrow specs.

Pending slices are tracked in `migration/BLUEPRINT.md` §6 and logged in
`migration/migration_telemetry.json`: nodes and plain-text layout, arrows,
patterns/shading, edit actions with undo, and the final 4-stage benchmark + PGO
staircase.
