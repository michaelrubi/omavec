# Omavec roadmap

What's needed, in the order we plan to do it. [DESIGN.md](DESIGN.md) covers
the architecture; [DECISIONS.md](DECISIONS.md) records why. As in Omapix,
finished items are ~~struck through~~ with "(done)", and anything deferred
goes on a "Later:" line under the item.

Status: Phase 0 under way. The workspace, the app shell and the canvas
exist, all six spikes are done with their numbers in DESIGN.md, "Phase 0
findings", and the open decisions are made. What's left of Phase 0 is for
Michael to try the shell from the installed binary. Phase 1 has begun.

## Borrowing from VectorCraft

[VectorCraft](https://github.com/storytold/vectorcraft) is an open-source
Illustrator clone in Rust, on the same egui 0.36 we use and on kurbo 0.13
and linesweeper underneath. It already has much of Omavec's Illustrator
half working and tested: curve booleans, Shape Builder, Offset Path,
Outline Stroke, Simplify, width profiles, warps, scissors and knife, and
SVG in and out. It is MIT OR Apache-2.0, so a GPL-3.0 app can use its code
as long as the copyright notice comes along.

What we take from each of its crates:

| VectorCraft crate | Needs | In Omavec |
| --- | --- | --- |
| `vectorcraft-geom` | kurbo, serde | **Dependency.** Its `PathData` (anchor lists) is the type we hand to pathops; vector networks convert to and from it. |
| `vectorcraft-pathops` | geom, linesweeper | **Dependency.** Booleans that stay curves, Pathfinder, Shape Builder regions (open paths included), offset, outline stroke, simplify. Most of Phase 3's geometry. |
| `vectorcraft-effects` | its document model | **Port** the modules we need into `omavec-geom` (warp styles, width-profile outlines, dashes), each with a header comment naming its source file and commit. Too tied to their document to depend on. |
| `vectorcraft-trace` | geom, pathops, image | Dependency, if Image Trace happens (see Later). |
| `vectorcraft-render`, `ui-egui` | everything | **Reference** for a `vello_cpu` canvas rendered off the UI thread, and for blurs, shadows and glows as `vello_cpu` filter layers. |
| `vectorcraft-tools` | its document model | **Reference** for tools as pointer events in, Begin/Preview/Commit actions out; the pen, direct selection, Shape Builder and cutting tools. |
| `vectorcraft-svg` | its document model | **Reference** for usvg import, a minimal SVG writer (stroke alignment, gradients, clipping) and `css_rules` for code export. |
| `vectorcraft-text` | its document model | **Reference** for the system font catalogue and glyph outlines. |
| `engine`, `mcp`, the rest | | Reference only (command registry, no-panic rules, MCP server), or not needed (CMYK, print, PDF/EPS/CAD import, brushes). |

What doesn't change: Omavec owns its document (frames, auto layout,
components, vector networks), its Figma-style UI, its file format and its
Omarchy integration. VectorCraft's paths have no branches, so vector
networks stay ours and convert at the pathops boundary.

Rules: pin VectorCraft to a commit and bump it on purpose; a file ported
from it says so in a header comment (source path and commit), and
`NOTICE` carries VectorCraft's copyright and MIT licence; take none of its
brand or assets except under the licences in its `ASSETS.md`; keep kurbo
on the version vello uses. Whether we stay on a git dependency or vendor
the two crates is decided in Phase 0 ([DECISIONS.md](DECISIONS.md), "Still
open").

## Releases

The full v1.0 bar (logos, screens, components and Figma import) is large,
so there are two earlier releases you can switch to, one per half of the
app:

| Release | After | You can stop opening… |
| --- | --- | --- |
| **v0.1 "Logo"** | Phases 0–4 | Illustrator, and Figma for icons/logos: draw, combine, offset, outline, vary stroke width, warp, set type, export clean SVG/PNG |
| **v0.2 "Screen"** | Phases 5–6 | Figma for new UI work: frames, auto layout, effects, components, variants, variables |
| **v1.0 "Figma-free"** | Phases 7–8 | Figma entirely: existing files imported, code and token export, the rest of the Illustrator toolkit |

Each phase ends with something usable from the installed binary. The
checklist under each phase is its exit test.

## 0. Foundations and spikes

Set up the workspace the way the sibling apps are set up, and settle the
riskiest technical questions before building on them. Spikes live in
`crates/*/examples/` and are deleted or promoted afterwards; their results
go into DESIGN.md.

- ~~Workspace: `Cargo.toml` (edition 2024, resolver 3, GPL-3.0-or-later),
  crates from DESIGN.md (empty but compiling), `Makefile`
  (`build`/`test`/`install`/`uninstall`), `assets/omavec.desktop`,
  `assets/omavec.svg`, `packaging/arch/PKGBUILD`, `CONTRIBUTING.md`, CI
  for `cargo test` and `clippy`.~~ (done) The five crates compile;
  `omavec-geom` already depends on `vectorcraft-geom` and
  `vectorcraft-pathops`, pinned to one commit, with one kurbo (0.13.1) in
  the tree. `NOTICE` carries VectorCraft's copyright and licence. CI is a
  GitHub Actions workflow.
  Later: a MIME type for `.omavecz` in the `.desktop` file, when Phase 1
  can open one.
- ~~App shell: an eframe window with the Omarchy theme (ported `theme.rs`),
  a menu bar, empty left (layers) and right (properties) panels, and the
  canvas in the middle.~~ (done) The theme follows Omarchy live. The first
  two `Command`s are Quit (Ctrl+Q) and Show/Hide UI (Ctrl+\, as in Figma),
  tested headless.
- ~~**Spike: canvas renderer.** vello 0.11 renders into a texture on
  egui-wgpu's device (both use wgpu 30) and shows in the canvas panel.
  Against it, VectorCraft's approach: `vello_cpu` on a worker thread,
  uploaded as an egui texture, with the last frame reprojected while the
  next renders (it reports 20,000 shapes in 27 ms per retina frame). Pan
  and zoom with 10,000 random cubic paths in both; record frame times at
  1×, 64× and 0.05× zoom, and pick one. If `vello_cpu` holds up, the
  canvas and headless export share one renderer.~~ (done) **`vello_cpu`
  on a worker thread.** At 2560 × 1440 it draws the 10,000 paths in 1.3 to
  2.7 ms at the three zooms, plus 2.6 ms to upload the frame; vello on the
  integrated GPU takes 3 to 5 ms and leaves a wrong frame on screen at 64×
  unless off-screen paths are skipped first. The numbers and reasons are
  in DESIGN.md, "Phase 0 findings". The winner is already the app's
  canvas (`crates/omavec/src/canvas.rs`): `OMAVEC_BLOBS=10000 omavec`
  shows the test scene to pan and zoom by hand, with the zoom and the
  last frame's time in the corner.
  Later: delete `examples/canvas_bench.rs`, the `vello` dev-dependency and
  `omavec_render::spike` once documents can be drawn.
- ~~**Spike: blurs and shadows.** Prototype a drop shadow and a layer blur
  on an arbitrary path with the renderer picked above. VectorCraft draws
  both as `vello_cpu` filter layers (`crates/render/src/fx.rs`): start
  there; with vello on the GPU, find what it can blur today or add a
  render-to-texture pass. Record the cost.~~ (done) Both work as
  `vello_cpu` filter layers drawn on a small single-threaded context and
  composited as an image (`examples/effects.rs`). About 2 ms for a 300 px
  shape and 15 to 30 ms for a 1,200 px one, so Phase 5 caches them per
  node and zoom; rounded-rectangle shadows take a shortcut that costs
  nothing.
- ~~**Spike: vector network.** `VectorNetwork` with vertices, segments and
  regions; find regions (smallest faces) from the planar graph; convert to
  `BezPath` and to and from `vectorcraft_geom::PathData`; property tests
  on random graphs.~~ (done) `omavec_geom::network`. Faces are checked
  against a flood fill on random grids and against Euler's formula on
  random curved ones; a network survives the trip through `PathData`
  unchanged. Faces of a 4,900-segment mesh take 1.3 ms. What it leaves
  for Phase 2 is listed in DESIGN.md.
- ~~**Spike: vectorcraft-pathops.** Add `vectorcraft-geom` and
  `vectorcraft-pathops` as git dependencies pinned to a commit.
  Union/subtract/intersect/exclude two and twenty overlapping curved
  shapes, and get Shape Builder regions; record time, anchor count and
  behaviour on coincident edges and tangencies. Fall back to raw
  `linesweeper` or `i_overlay` only for what it gets wrong. Check its
  kurbo matches vello's.~~ (done) It gets nothing wrong that the spike
  could find: 63 µs for two circles, 4 ms for twenty shapes, 18 ms for
  Shape Builder's arrangement of those twenty, results within 0.04% of
  the inputs on a point grid, no panics on shared edges, identical shapes
  or tangencies, and one kurbo in the tree. No fallback is needed
  (`examples/pathops.rs`; table in DESIGN.md).
- ~~**Spike: text.** Lay out a line with parley using a system font found by
  fontique, draw it with the canvas renderer, and turn it into outlines
  with skrifa. Read `vectorcraft-text`'s font catalogue and outline code
  first.~~ (done) `examples/text.rs` does all four with the system
  sans-serif and JetBrains Mono. fontique agrees with `fc-match`, layout
  takes microseconds once a font is loaded, and the skrifa outlines match
  `vello_cpu`'s own glyph rendering to within 0.002% of pixels. The three
  crates share one skrifa. Notes for the text tool are in DESIGN.md.
- ~~**Spike: .fig.** Decode a real `.fig` ("Save local copy") with
  `kiwi-schema` and dump its node tree as JSON. Start the fixture
  folder.~~ (done) `omavec_fig::decode` reads four real files from 2022 to
  2026 (deflate and zstd, zipped and bare) and a test checks their node
  trees against fig2sketch's decoder. `kiwi-schema` 0.2.1 needed no
  patching. `cargo run -p omavec-fig --example fig_dump -- file.fig`
  prints a tree, or the whole message with `--json`.
  Later: add two or three of Michael's own files to
  `crates/omavec-fig/tests/fixtures/` before Phase 7.
- ~~Decide the first two "Still open" items in DECISIONS.md, and item 5
  (VectorCraft as a git dependency or vendored).~~ (done) Figma's letters
  win, with a command palette, a `:` command line and hjkl nudging as an
  off-by-default setting; the `.omavec` folder is canonical, with a zipped
  `.omavecz` for sending and for file managers; VectorCraft is a pinned
  git dependency.

Exit: the shell runs from `make install` in the Omarchy theme, and every
spike has numbers and a decision written down.

## 1. Document core and canvas

The editor skeleton: a document you can draw simple shapes in, save,
reopen and export.

- Engine: node tree with stable ids, `Arc` copy-on-write snapshots, undo
  and redo, the `Command` enum, dirty tracking. Shipped code returns
  errors instead of panicking, as VectorCraft enforces with clippy lints
  (`unwrap_used`, `expect_used`, `panic` denied outside tests).
- `.omavec` folder format: deterministic JSON, format version, assets by
  hash. Save, open, recent files, autosave and crash recovery. Then
  `.omavecz`, the same folder zipped, with its MIME type.
- Canvas: pan (Space/H/middle drag), zoom (Ctrl+wheel, Shift+0/1/2, pinch),
  pixel grid at high zoom, rulers. Phase 0 already built wheel, middle
  drag and Space+drag panning, Ctrl+wheel and pinch zoom about the
  pointer, and Ctrl+= / Ctrl+- / Shift+0; the pixel grid and rulers are
  built and only the Hand tool and zoom to fit and to selection are left.
- Selection: click, Shift+click, marquee, deep select (Ctrl+click), select
  in group (double-click / Enter), Esc to parent.
- Tools as in `vectorcraft-tools`: pointer events in, Begin/Preview/Commit
  actions out, so every drag is one undo step and every tool is testable
  without a window.
- Transform: move, resize and rotate handles; Shift/Alt modifiers; nudge
  with arrows (Shift: 10); numeric X/Y/W/H/rotation in the properties panel.
- Tools: Frame (artboards = top-level frames, with Figma's device presets),
  Rectangle (per-corner radii), Ellipse (arc/ratio), Polygon, Star, Line,
  Arrow.
- Paint: solid fills and strokes, multiple fills, opacity, blend modes;
  linear/radial/angular/diamond gradients with on-canvas handles; the
  colour picker with eyedropper.
- Panels: layers (tree, rename, reorder by drag, hide, lock, multi-select),
  properties (Figma's right panel layout).
- Smart guides and snapping: edges, centres, equal spacing, pixel grid.
- Group (Ctrl+G), frame selection (Ctrl+Alt+G), duplicate (Ctrl+D,
  Alt+drag), copy/paste within Omavec and as SVG to the Wayland clipboard.
- Export: per-node export settings (SVG, PNG @1x/@2x/@3x); `omavec export`
  CLI; `vello_cpu` for headless PNG. `vectorcraft-svg`'s writer is the
  reference for the SVG side.
- `OMAVEC_SCRIPT` replay and the egui `Harness` for UI tests.
- Command palette (Ctrl+K, Ctrl+/) listing every `Command`, and `:` to
  open it as a command line.

Exit: draw a few shapes in two frames, style them, save, reopen, undo
through the session, and export the frames as SVG and PNG from the app and
from the CLI.

## 2. Vector editing

The Figma half of paths.

- Vector networks in the engine, built on Phase 0's
  `omavec_geom::network`: crossings without a vertex, corner radius and
  handle mirroring per vertex.
- Pen tool with Figma's behaviour: click for corners, drag for curves,
  branch from any vertex, close onto any vertex, Shift for 45°, Alt to
  break handles, Ctrl for the bend tool.
- Vector edit mode (Enter or double-click): select and drag vertices,
  segments and handles; handle mirroring (none, angle, angle and length);
  per-vertex corner radius; delete a vertex and heal (Ctrl+Delete)
  or delete and split.
- Paint bucket in edit mode: fill or clear individual regions.
- Pencil (Shift+P) with curve fitting (`vectorcraft-pathops`'
  `simplify_with`: least squares with corner detection); pressure saved
  for Phase 8.
- Convert shapes to vectors, and Flatten (Ctrl+E).
- SVG import (paste and open) into networks; SVG export from networks with
  minimal path data. VectorCraft's `crates/svg` covers the usvg mapping
  and the awkward cases (stroke alignment, clip paths, gradients).
- Hit testing and snapping on segments and vertices (`rstar`).

Exit: redraw three icons from a real icon set by hand in Omavec, paste
another SVG icon in, edit it, and export all four as clean SVG that
round-trips through import unchanged.

## 3. Logo toolkit

The Illustrator half: what people leave Figma for. The geometry comes
from `vectorcraft-pathops` and ported VectorCraft modules, so this phase
is mostly wiring it to vector networks, live modifiers and Figma-style
UI.

- Live boolean groups (union, subtract, intersect, exclude), curve-native,
  nestable, with Flatten to bake. Toolbar buttons and shortcuts as in
  Figma. Built on `boolean` and `boolean_n`.
- **Shape Builder (Shift+M):** hover highlights faces, drag across faces
  to merge them, Alt+drag to delete faces or edges, with the result's fill
  taken from the face first clicked. Built on `shape_builder`,
  `region_at` and `merge_regions`, which already handle open paths
  cutting regions.
- **Outline Stroke (Ctrl+Shift+O):** exact caps, joins and dashes; inside
  and outside alignment. `outline_stroke` plus the dash code ported from
  `vectorcraft-effects`.
- **Offset Path:** as a live modifier and as a one-off action; miter,
  round and bevel joins; positive and negative distances; clean result
  (few anchors). `offset_path`; VectorCraft lists a large-offset bug as
  open, so test big distances.
- Modifiers panel (the Appearance stack) for live operations on a node.
- **Scissors (C)** to cut at a point, **Knife (Shift+C)** to slice across
  shapes. `vectorcraft-tools`' cutting tools are the reference.
- **Width tool (Shift+W)** and width profiles, moved up from Phase 8:
  port `vectorcraft-effects`' width outline. Pressure stays in Phase 8.
- **Warp presets** (arc, arch, bulge, flag, wave, fish, rise, squeeze,
  twist) as live modifiers, moved up from Phase 8: port
  `vectorcraft-effects`' warp styles.
- Precision: snap to anchors, segment midpoints, intersections and
  tangents; typed angles and lengths while drawing; align to pixel grid.
- Simplify path (`simplify_with`); curvature comb display when editing,
  to see G2 continuity.
- Tests: VectorCraft's property and regression tests cover pathops
  upstream. Ours cover the vector network ↔ `PathData` conversion, live
  modifier caching, and fuzzing booleans and offsets on vector networks.

Exit: build a real logo (overlapping circles cut with Shape Builder, an
offset outline, an outlined stroke, a variable-width stroke) without
Illustrator, and export SVG with no stray anchors.

## 4. Text (→ v0.1 "Logo")

- Text tool (T): auto width, auto height and fixed boxes; editing on the
  canvas with cursor, selection, IME and clipboard, built on parley.
- Font family/style/size, line height, letter spacing, alignment,
  decoration and case; system fonts through fontique with a searchable
  picker and previews.
- OpenType basics: ligatures, tabular figures, stylistic sets.
- Text styles (named, reusable).
- **Text → Outlines** (Ctrl+Shift+O on text) into editable vector networks.
- Hand-kerning for logotypes is done after outlining, in Phase 2's edit
  mode. Later: per-glyph kerning on live text.
- Release v0.1: version bump, README screenshots, AUR package.

Exit: set a wordmark, outline it, merge it with a symbol using Shape
Builder, and ship SVG/PNG/PDF exports. **Logos no longer need Figma or
Illustrator.**

## 5. Layout and effects

The Figma half: screens.

- **Auto layout (Shift+A)** via taffy: direction, gap (and auto gap),
  padding, alignment, wrap, hug/fill/fixed per child, min/max sizes,
  absolute position inside auto layout, reversed z-order, nested auto
  layout. Canvas handles for padding and gap like Figma.
- Constraints for children of frames without auto layout; resizing frames
  re-lays out their children live.
- Layout grids (columns, rows, grid) on frames.
- Images: drag-in and paste, fill/fit/crop/tile, crop on canvas, image as
  a fill on any shape.
- Effects: drop shadow, inner shadow, layer blur, background blur, as
  `vello_cpu` filter layers drawn off to the side and cached per node and
  zoom (Phase 0's `examples/effects.rs`; VectorCraft's
  `crates/render/src/fx.rs` for inner shadows and the cache).
- Corner smoothing (Figma's squircle corners) on frames and rectangles.
- Clip content, masks (use a shape as a mask, Ctrl+Alt+M).
- Performance pass: a 2,000-frame document stays at display rate.

Exit: lay out a real app screen (nav bar, list of cards, responsive to
frame width) and export it.

## 6. Design system (→ v0.2 "Screen")

- Components (Ctrl+Alt+K) and instances: overrides by node id path, reset
  overrides, detach, swap instance, go to main component.
- Variants: component sets with variant properties, the variant picker on
  instances, and instances that keep overrides across variant swaps.
- Component properties: boolean, text, instance swap.
- Named styles: colour, text, effect, grid.
- Variables: colour, number, string and boolean, in collections with modes
  (light/dark and so on); bind fills, strokes, gaps, padding, radii, text
  and visibility; switch a frame's mode.
- Assets panel listing the file's components, styles and variables.
- Release v0.2.

Exit: build a small design system (buttons with size and state variants,
an input, a card, light and dark colour modes) and assemble two screens
from it. **New UI work no longer needs Figma.**

## 7. Interop

- **.fig import** (`omavec import`, and File › Import): pages, frames,
  groups, shapes, vector networks, booleans, text, images, fills, strokes,
  effects, auto layout, constraints, components, instances with overrides,
  variants and variables. An import report lists every dropped feature
  with the node it was on. Fixture tests from real files.
- **Code export** panel for the selection (Figma Dev Mode style): CSS,
  Tailwind classes, SVG, SVG-in-JSX/TSX, with variables as CSS custom
  properties. `vectorcraft-svg`'s `css_rules` is the reference for paints,
  borders, fonts and shadows as CSS.
- **Token export** (`omavec tokens`): variables as CSS, a Tailwind theme,
  or an Omarchy theme.
- PDF export through svg2pdf (or krilla, which VectorCraft uses); export
  presets for whole pages.

Exit: import your three most-used Figma files, fix up what the report
lists in under an hour each, and hand a screen off as Tailwind.

## 8. Illustrator extras and polish (→ v1.0 "Figma-free")

- Pressure-sensitive pencil via the tablet, feeding width profiles (the
  Width tool itself moved to Phase 3).
- **Envelope distort:** mesh envelopes as live modifiers (warp presets
  moved to Phase 3). VectorCraft's mesh envelopes are the reference.
- Rich text: mixed styles in one box. Type on a path.
- Torn-off panels as separate windows for Hyprland tiling.
- Performance: 100,000-node documents; idle uses no CPU.
- Release v1.0.

## Later (not committed)

- Prototyping: links between frames and a present mode (the document model
  leaves room for it).
- Shared libraries: publish components from one file, use them in others.
- Plugin API (Lua, Rhai or WASM) for custom tools and exports. VectorCraft
  runs WebAssembly plug-ins with `wasmi` (`crates/plugins`).
- Image Trace for turning scanned sketches into logo outlines, through
  `vectorcraft-trace`.
- An MCP server so agents can drive Omavec, as VectorCraft's does. Every
  action is already a `Command`, so it is a thin layer.
- Edit placed images in Omapix and update them on save.
- Display P3 documents.
- PDF/AI/EPS import.
