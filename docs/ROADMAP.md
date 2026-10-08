# Omavec roadmap

What's needed, in the order we plan to do it. [DESIGN.md](DESIGN.md) covers
the architecture; [DECISIONS.md](DECISIONS.md) records why. As in Omapix,
finished items are ~~struck through~~ with "(done)", and anything deferred
goes on a "Later:" line under the item.

Status: scoping done, nothing built. Next up is Phase 0.

## Releases

The full v1.0 bar (logos, screens, components and Figma import) is large,
so there are two earlier releases you can switch to, one per half of the
app:

| Release | After | You can stop opening… |
| --- | --- | --- |
| **v0.1 "Logo"** | Phases 0–4 | Illustrator, and Figma for icons/logos: draw, combine, offset, outline, set type, export clean SVG/PNG |
| **v0.2 "Screen"** | Phases 5–6 | Figma for new UI work: frames, auto layout, effects, components, variants, variables |
| **v1.0 "Figma-free"** | Phases 7–8 | Figma entirely: existing files imported, code and token export, the rest of the Illustrator toolkit |

Each phase ends with something usable from the installed binary. The
checklist under each phase is its exit test.

## 0. Foundations and spikes

Set up the workspace the way the sibling apps are set up, and settle the
riskiest technical questions before building on them. Spikes live in
`crates/*/examples/` and are deleted or promoted afterwards; their results
go into DESIGN.md.

- Workspace: `Cargo.toml` (edition 2024, resolver 3, GPL-3.0-or-later),
  crates from DESIGN.md (empty but compiling), `Makefile`
  (`build`/`test`/`install`/`uninstall`), `assets/omavec.desktop`,
  `assets/omavec.svg`, `packaging/arch/PKGBUILD`, `CONTRIBUTING.md`, CI
  for `cargo test` and `clippy`.
- App shell: an eframe window with the Omarchy theme (ported `theme.rs`),
  a menu bar, empty left (layers) and right (properties) panels, and the
  canvas in the middle.
- **Spike: vello in egui.** vello 0.11 renders into a texture on egui-wgpu's
  device (both use wgpu 30) and shows in the canvas panel. Pan and zoom
  with 10,000 random cubic paths; record frame times at 1×, 64× and
  0.05× zoom.
- **Spike: blurs and shadows.** Find out what vello can blur today, and
  prototype a drop shadow and a layer blur on an arbitrary path (vello's
  own support, or a render-to-texture pass). Record the cost.
- **Spike: vector network.** `VectorNetwork` with vertices, segments and
  regions; find regions (smallest faces) from the planar graph; convert to
  `BezPath`; property tests on random graphs.
- **Spike: curve booleans.** Union/subtract/intersect/exclude two and
  twenty overlapping curved shapes with `linesweeper`, compare with
  `i_overlay` (time, anchor count, robustness on coincident edges), and
  extract faces for Shape Builder.
- **Spike: text.** Lay out a line with parley using a system font found by
  fontique, draw it with vello, and turn it into outlines with skrifa.
- **Spike: .fig.** Decode a real `.fig` ("Save local copy") with
  `kiwi-schema` and dump its node tree as JSON. Start the fixture folder.
- Decide the first two "Still open" items in DECISIONS.md.

Exit: the shell runs from `make install` in the Omarchy theme, and every
spike has numbers and a decision written down.

## 1. Document core and canvas

The editor skeleton: a document you can draw simple shapes in, save,
reopen and export.

- Engine: node tree with stable ids, `Arc` copy-on-write snapshots, undo
  and redo, the `Command` enum, dirty tracking.
- `.omavec` folder format: deterministic JSON, format version, assets by
  hash. Save, open, recent files, autosave and crash recovery.
- Canvas: pan (Space/H/middle drag), zoom (Ctrl+wheel, Shift+0/1/2, pinch),
  pixel grid at high zoom, rulers.
- Selection: click, Shift+click, marquee, deep select (Ctrl+click), select
  in group (double-click / Enter), Esc to parent.
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
  CLI; `vello_cpu` for headless PNG.
- `OMAVEC_SCRIPT` replay and the egui `Harness` for UI tests.

Exit: draw a few shapes in two frames, style them, save, reopen, undo
through the session, and export the frames as SVG and PNG from the app and
from the CLI.

## 2. Vector editing

The Figma half of paths.

- Vector networks in the engine, replacing the spike.
- Pen tool with Figma's behaviour: click for corners, drag for curves,
  branch from any vertex, close onto any vertex, Shift for 45°, Alt to
  break handles, Ctrl for the bend tool.
- Vector edit mode (Enter or double-click): select and drag vertices,
  segments and handles; handle mirroring (none, angle, angle and length);
  per-vertex corner radius; delete a vertex and heal (Ctrl+Delete)
  or delete and split.
- Paint bucket in edit mode: fill or clear individual regions.
- Pencil (Shift+P) with curve fitting; pressure saved for Phase 8.
- Convert shapes to vectors, and Flatten (Ctrl+E).
- SVG import (paste and open) into networks; SVG export from networks with
  minimal path data.
- Hit testing and snapping on segments and vertices (`rstar`).

Exit: redraw three icons from a real icon set by hand in Omavec, paste
another SVG icon in, edit it, and export all four as clean SVG that
round-trips through import unchanged.

## 3. Logo toolkit

The Illustrator half: what people leave Figma for.

- Live boolean groups (union, subtract, intersect, exclude), curve-native,
  nestable, with Flatten to bake. Toolbar buttons and shortcuts as in
  Figma.
- **Shape Builder (Shift+M):** hover highlights faces, drag across faces
  to merge them, Alt+drag to delete faces or edges, with the result's fill
  taken from the face first clicked.
- **Outline Stroke (Ctrl+Shift+O):** exact caps, joins and dashes; inside
  and outside alignment.
- **Offset Path:** as a live modifier and as a one-off action; miter,
  round and bevel joins; positive and negative distances; clean result
  (few anchors).
- Modifiers panel (the Appearance stack) for live operations on a node.
- **Scissors (C)** to cut at a point, **Knife (Shift+C)** to slice across
  shapes.
- Precision: snap to anchors, segment midpoints, intersections and
  tangents; typed angles and lengths while drawing; align to pixel grid.
- Simplify path (fewer anchors within a tolerance); curvature comb display
  when editing, to see G2 continuity.
- Fuzz tests for booleans and offsets against `i_overlay`.

Exit: build a real logo (overlapping circles cut with Shape Builder, an
offset outline, an outlined stroke) without Illustrator, and export SVG
with no stray anchors.

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
- Effects: drop shadow, inner shadow, layer blur, background blur
  (approach from Phase 0's spike).
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
  properties.
- **Token export** (`omavec tokens`): variables as CSS, a Tailwind theme,
  or an Omarchy theme.
- PDF export through svg2pdf; export presets for whole pages.

Exit: import your three most-used Figma files, fix up what the report
lists in under an hour each, and hand a screen off as Tailwind.

## 8. Illustrator extras and polish (→ v1.0 "Figma-free")

- **Width tool (Shift+W):** drag on a stroke to change its width at a
  point; saved width profiles; pressure-sensitive pencil via the tablet.
- **Envelope distort and warp:** warp presets (arc, arch, bulge, flag,
  wave, fish, rise, squeeze, twist) and mesh envelopes, as live modifiers.
- Rich text: mixed styles in one box. Type on a path.
- Torn-off panels as separate windows for Hyprland tiling.
- Performance: 100,000-node documents; idle uses no CPU.
- Release v1.0.

## Later (not committed)

- Prototyping: links between frames and a present mode (the document model
  leaves room for it).
- Shared libraries: publish components from one file, use them in others.
- Plugin API (Lua, Rhai or WASM) for custom tools and exports.
- Edit placed images in Omapix and update them on save.
- Display P3 documents.
- PDF/AI/EPS import.
