# Omavec design

Omavec is a local-first vector design app for Omarchy: Figma's frames,
auto layout and components, plus the handful of Illustrator tools people
leave Figma for (Shape Builder, Offset Path, Outline Stroke, variable-width
strokes, envelope distort, scissors and knife). It should feel like Figma to
someone with Figma muscle memory, and start and run like a native Omarchy
app.

It is an independent project, not part of Omarchy.

Status: scoping. No code yet. The decisions behind this document are in
[DECISIONS.md](DECISIONS.md); the order of work is in
[ROADMAP.md](ROADMAP.md).

## Principles

1. **One app for logos and screens.** The reason Omavec exists is to stop
   switching between Figma and Illustrator. A feature that only makes sense
   if you also own the other app is a bug.
2. **Figma muscle memory.** Default shortcuts, tool letters, panels and
   canvas behaviour match Figma. Illustrator-only tools use Illustrator's
   letters where they don't clash.
3. **Non-destructive by default, bakeable on demand.** Booleans, offsets,
   strokes and warps are live until you choose Flatten (Ctrl+E) or Outline
   Stroke (Ctrl+Shift+O). The source geometry is never lost silently.
4. **Clean output.** Exported SVG should be shippable as-is: few anchor
   points, curves kept as curves, no needless groups or transforms. Curve
   booleans and offsets stay on Béziers instead of flattening to polygons.
5. **Omarchy-native.** Wayland-native, colours follow the active Omarchy
   theme live, keyboard-first, installs as one package.
6. **Local and plain.** Files are folders of readable JSON that diff well in
   git. No account, no network, no telemetry.
7. **Lightweight.** Starts instantly and stays at display rate on documents
   with tens of thousands of nodes. Every feature has to justify its cost
   in startup time and memory.

Not in scope: real-time collaboration, raster painting (that's Omapix),
print/CMYK prepress, and (for now) prototyping.

## Architecture

```
crates/
  omavec-geom     curve maths: vector networks, booleans, offsets, stroke
                  expansion, width profiles, warps, snapping and hit tests.
                  Knows nothing about documents. (No UI or GPU.)
  omavec-engine   the document: node tree, paints, effects, layout (taffy),
                  text layout (parley), components, variables, undo, the
                  .omavec format, SVG import/export, and the display list.
                  (No UI or GPU; headlessly testable.)
  omavec-render   display list → vello Scene. GPU path (vello on wgpu) for
                  the canvas; CPU path (vello_cpu) for headless PNG export
                  and golden-image tests.
  omavec-fig      best-effort .fig importer (kiwi decoding → engine nodes).
                  Kept separate because the format changes under us.
  omavec          the app: egui UI on wgpu, canvas, tools, commands,
                  panels, Omarchy theme, and the `omavec` CLI.
```

As in Omapix, the engine never depends on the UI, and every user action is
a `Command` (`crates/omavec/src/commands.rs`), so menus, shortcuts, the
command palette, `OMAVEC_SCRIPT` and the CLI never diverge.

`omavec-geom` is the "Omapath" idea from the naming notes: a reusable
geometry core that could serve other apps. It gets its own crate so it
stays free of document concepts.

### The document model

The model unites Figma's "everything is a box" with Illustrator's
"everything is a path".

```
Document
└─ Page*
   └─ Node*            stable 64-bit id, name, visible, locked, opacity,
                       blend mode, transform (affine), constraints
      ├─ Frame         container: clip flag, corner radii, fills/strokes,
      │                optional AutoLayout, layout grids. Artboards are
      │                just top-level frames.
      ├─ Group         transparent container, no own paint
      ├─ Boolean       live CSG: union | subtract | intersect | exclude
      │                over its children, evaluated by omavec-geom
      ├─ Shape         parametric rect / ellipse (arcs) / polygon / star /
      │                line; edits as parameters until converted
      ├─ Vector        a VectorNetwork
      ├─ Text          parley layout of one text run (v1)
      ├─ Image         asset reference + fill mode (fill/fit/crop/tile)
      ├─ Component     definition; ComponentSet groups variants
      └─ Instance      component reference + override map
```

Every visual node carries:

- **Fills**: a stack of paints (solid, linear/radial/angular/diamond
  gradient, image), each with its own opacity and blend mode. Any colour
  or number can be bound to a variable.
- **Strokes**: a stack of paints plus one stroke style: weight, align
  (inside/centre/outside), cap, join, miter limit, dashes, and an optional
  **width profile** (widths at positions along each segment).
- **Effects**: drop shadow, inner shadow, layer blur, background blur.
- **Modifiers**: an ordered, live list of geometry operations: Offset Path,
  Outline Stroke, Warp/Envelope, Round Corners, Simplify. This is
  Illustrator's Appearance panel in Figma's node tree. Flatten bakes the
  whole stack into a plain `Vector`.

Coordinates are `f64` canvas units (kurbo's type), y down, as in Figma.
Node ids stay stable across saves, which keeps git diffs small and lets
instance overrides address nodes inside components by id path.

### Vector networks

Following Figma's vector networks, a `Vector` is a planar graph rather
than a list of subpaths:

```rust
struct VectorNetwork {
    vertices: Vec<Vertex>,   // position, corner radius, handle mirroring
    segments: Vec<Segment>,  // start, end, tangent_start, tangent_end
    regions:  Vec<Region>,   // loops of segment refs + fill rule + fills
}
```

- A vertex can have any number of segments (branches, webs, T-joins).
- Regions are the fillable faces. They are found from the planar graph
  (smallest cycles, as in Figma's paint bucket) and can carry their own
  fills, so one network can hold a multi-colour icon.
- Rendering, booleans and export convert regions and open chains to
  `kurbo::BezPath`. SVG import goes the other way: subpaths become chains,
  and shared endpoints are merged.

### Geometry operations

| Operation | Approach | Crates |
| --- | --- | --- |
| Booleans (live and baked) | Curve-native sweep line, so results stay Béziers; pieces refitted to few anchors | `vectorcraft-pathops` (on `linesweeper`); `i_overlay` only if it fails on degenerate input |
| Shape Builder | Arrange every selected outline into one planar graph; find the faces with their winding numbers; hit-test faces and edges under the drag; union the picked faces (Alt: delete them, or delete edges) | `vectorcraft-pathops` (`shape_builder`, `region_at`, `merge_regions`) |
| Offset Path | Fill ∪ stroke of twice the distance (outset) or fill − stroke (inset), normalised and refitted | `vectorcraft-pathops` (`offset_path`); `cavalier_contours` if large offsets need it |
| Outline Stroke | Expand the stroke exactly (caps, joins, dashes, inside/outside align by boolean against the fill) | `vectorcraft-pathops` (`outline_stroke`), dashes ported from `vectorcraft-effects` |
| Variable width | Sample the width profile, offset both sides by half the local width, join at corners, cap | Ported from `vectorcraft-effects` (`stroke/width.rs`) into `omavec-geom` |
| Envelope / warp | Map points through a warp (arc, bulge, flag…) or a mesh (Coons patch), subdividing segments until the mapped curve is within tolerance, then fit | Warp styles ported from `vectorcraft-effects` (`warp.rs`); meshes in `omavec-geom` |
| Scissors / knife | Split segments at a point or along a drawn path; knife splits regions | `kurbo` intersections |
| Snapping and hit tests | R-tree of node and segment bounds; snap to points, edges, midpoints, centres, angles, pixel grid | `rstar`, `kurbo::ParamCurveNearest` |

`vectorcraft-pathops` works on `vectorcraft_geom::PathData` (subpaths of
anchors). Vector networks convert to it at the boundary and results
convert back; see [ROADMAP.md](ROADMAP.md), "Borrowing from VectorCraft".

Every operation is a pure function from geometry to geometry, so live
modifiers can be cached by input hash and evaluated in parallel with
`rayon`.

### Layout

- **Auto layout** maps onto CSS flexbox and grid through `taffy`: direction,
  gap, padding, alignment, wrap, hug/fill/fixed sizing, min/max sizes, and
  absolutely positioned children. Layout runs when a child or frame size
  changes, only on the dirty subtree.
- **Constraints** (left/right/centre/scale…) apply to children of frames
  without auto layout, as in Figma.
- **Layout grids** (columns, rows, grid) are guides only.

### Text

`parley` lays text out, `fontique` finds system fonts (through fontconfig,
so fonts installed on Omarchy just appear), and `skrifa` provides glyph
outlines for Text → Outlines. Text editing on the canvas is our own, built
on parley's editor, not egui's text widgets: they can't edit text on a
transformed canvas.

### Rendering

```
engine document ──► display list ──► omavec-render ──► vello::Scene ──► wgpu texture ──► egui
   (dirty nodes)     (per node,          (per-node scene          (vello 0.11,      (canvas panel,
                     cached)              fragments, appended      wgpu 30)          same wgpu 30
                                          with the view transform)                   device)
```

- vello renders the whole canvas on the GPU every frame it changes,
  straight from curves, so zooming never shows stale tiles.
- Each node's encoded scene fragment is cached and appended with the view
  transform, so panning and zooming re-encode nothing.
- Canvas overlays (selection handles, smart guides, Shape Builder
  highlights, vector edit handles) are a separate vello layer drawn on top.
  egui draws only the UI around the canvas.
- Headless export (CLI, tests) uses `vello_cpu`, with no GPU needed.
- **Known gap:** vello has limited support for blur filters. Shadows and
  blurs on arbitrary shapes may need our own wgpu pass (render the node to
  a texture, blur it, composite it). Phase 0 checks this.
- **The alternative:** VectorCraft renders its canvas with `vello_cpu` on a
  worker thread and only composites on the GPU, with blurs, shadows and
  glows as `vello_cpu` filter layers. Phase 0 measures both and picks one;
  the diagram above assumes vello on the GPU.

### Undo

Documents are trees of `Arc` nodes with copy-on-write, so an undo snapshot
shares every unchanged node (the same idea as Omapix's tiles). Each command
is one undo step; dragging coalesces into one step on release.

### Files

A document is a folder:

```
logo.omavec/
  document.json        format version, pages list, variables, styles
  pages/
    01-cover.json      node tree for one page, pretty-printed, keys sorted
  assets/
    3f9a…c2.png        images by content hash (shared across pages)
  thumbnail.png        for file pickers
```

- Text formatting is deterministic (sorted keys, fixed float formatting),
  so saving an unchanged document changes nothing in git.
- Fonts are referenced by family/style, not embedded.
- A zipped single-file form (`.omavecz`) is planned for sending files to
  people (see [DECISIONS.md](DECISIONS.md), "Still open").

### Import and export

- **SVG import** through `usvg`, which normalises the SVG, into vector
  networks, frames and groups, keeping ids and names.
- **SVG export** written by us (not `usvg`) for minimal output: merged
  transforms, shortest path data, optional `currentColor`, per-frame or
  per-selection.
- **PNG/JPEG/WebP export** at @1x/@2x/@3x presets, per node, like Figma's
  export settings.
- **PDF export** through `svg2pdf`, mainly for logo handoff.
- **Code export** from a selection: CSS, Tailwind classes, SVG, and
  SVG-in-JSX/TSX; variables as CSS custom properties, a Tailwind theme, or
  an Omarchy theme.
- **.fig import** (`omavec-fig`): a `.fig` file is a zip around a
  kiwi-encoded binary (a schema chunk plus a data chunk, deflate- or
  zstd-compressed). `kiwi-schema` decodes it with the file's own schema,
  and the converter maps Figma's node changes to Omavec nodes. Every import
  produces a report listing what couldn't be converted. Sketch's
  open-source `fig2sketch` converter is the main reference.

### The CLI

```
omavec file.omavec                                   open in the app
omavec export file.omavec --frame Logo --format svg,png@2x --out dist/
omavec export file.omavec --all-export-settings      every node's export presets
omavec import design.fig --out design.omavec         .fig conversion + report
omavec tokens file.omavec --format css|tailwind|omarchy
```

The CLI uses the same engine, renderer and commands as the app, with no
window and no GPU.

## Omarchy integration

- **Theme**: colours come from the active Omarchy theme and update live,
  ported from Omapix's `theme.rs`. Theme colours are for the UI only; the
  canvas shows document colours unchanged, on a neutral backdrop.
- **Wayland and Hyprland**: native Wayland through winit. Panels can be
  torn off into their own windows (egui viewports), so Hyprland can tile
  them. Tablet pressure through the Wayland tablet protocol, ported from
  Omapix's `tablet.rs`, for the pencil and width tools.
- **Keyboard first**: Figma shortcuts, a command palette that lists every
  `Command`, and Vim-style keys where they don't fight Figma (see
  [DECISIONS.md](DECISIONS.md), "Still open").
- **Install**: `make install` to `~/.local`, plus an AUR `PKGBUILD`, a
  `.desktop` file with a MIME type for `.omavec`, and an icon.

## Shortcuts (initial)

Figma's defaults, plus Illustrator's letters for the tools Figma lacks.

| Key | Action | Key | Action |
| --- | --- | --- | --- |
| V | Move | K | Scale |
| F / A | Frame | R | Rectangle |
| O | Ellipse | L / Shift+L | Line / Arrow |
| P | Pen | Shift+P | Pencil |
| T | Text | H | Hand |
| Shift+M | Shape Builder | C | Scissors |
| Shift+C | Knife | Shift+W | Width tool |
| I | Eyedropper | Enter | Edit vector / enter group |
| Ctrl+G | Group | Ctrl+Alt+G | Frame selection |
| Shift+A | Add auto layout | Ctrl+Alt+K | Create component |
| Ctrl+E | Flatten | Ctrl+Shift+O | Outline stroke |
| Ctrl+Shift+H | Show/hide | Ctrl+Shift+L | Lock/unlock |
| Shift+1 / Shift+2 | Zoom to fit / selection | Ctrl+K, Ctrl+/ | Command palette |

## Open-source references

What to take from each, and how. Licences are checked before any code is
copied; the crates below are all MIT/Apache, which work with Omavec's
GPL-3.0.

| Project | Take | How |
| --- | --- | --- |
| Linebender (`kurbo`, `vello`, `parley`, `fontique`, `peniko`, `linesweeper`, `vello_cpu`) | Curve maths, GPU rendering, text, booleans | Dependencies |
| `taffy` | Flexbox/grid layout | Dependency |
| `cavalier_contours`, `i_overlay` | Polyline offsetting; robust polygon booleans as a fallback | Dependencies |
| `usvg`, `svg2pdf` | SVG import, PDF export | Dependencies |
| `kiwi-schema` | Decoding `.fig` files | Dependency |
| VectorCraft (MIT OR Apache-2.0) | Booleans, Shape Builder, offset, outline stroke, simplify; width profiles, warps, dashes; the `vello_cpu` canvas and filter-layer effects; tool, SVG and command patterns | `vectorcraft-geom` and `vectorcraft-pathops` as dependencies; effect modules ported with a source note; the rest read. See ROADMAP.md, "Borrowing from VectorCraft" |
| Graphite (Apache-2.0) | Its Bézier-path boolean library and `bezier-rs`; how it caches node evaluation and bridges documents to vello | Read; take `bezier-rs` if it beats kurbo for something |
| Inkscape / lib2geom (LGPL-2.1 or MPL-1.1) | Edge cases: self-intersections, curve extrema, envelope warps, live path effects | Read as a reference; port algorithms, not code |
| Penpot (MPL-2.0) | How auto layout maps to flex/grid, and the component/variant/override data model | Read as a reference |
| Figma (blog posts and docs) | Vector network semantics, paint bucket behaviour, auto layout and constraint rules, shortcut list | Behaviour reference |
| `fig2sketch` (Sketch, MIT) | How `.fig` node changes map to a document | Reference for `omavec-fig` |

## Risks

1. **Blurs and shadows in vello.** If vello can't blur arbitrary shapes
   yet, effects need our own render-to-texture passes, which costs
   complexity and per-frame time. Mitigation: VectorCraft's `vello_cpu`
   canvas already draws them; Phase 0 decides between the two.
2. **Robustness of curve booleans.** `linesweeper` is young. Shape Builder
   and live booleans have to survive coincident edges, tangencies and tiny
   slivers. Mitigation: `vectorcraft-pathops` wraps it with refitting,
   property tests and regression tests from a shipping app; our own fuzz
   tests on vector networks; `i_overlay` as a last resort.
3. **Depending on VectorCraft.** It is young and moves fast, and its API
   can change under us. Mitigation: pin to a commit, depend on only its two
   leaf crates, and vendor them if upgrades get painful.
4. **The .fig format changes.** Figma changes the schema, but every file
   carries its own schema, which helps. Mitigation: fixture files from real
   documents, a version gate, and a report of what was dropped.
5. **Scope.** Figma and Illustrator are decades of work. Mitigation: two
   intermediate releases (logos, then screens) you actually switch to, and
   a roadmap that keeps every phase usable.
6. **Canvas text editing.** IME, selection and cursor movement on a
   transformed canvas are fiddly and egui can't help. Mitigation: build on
   parley's editor; keep v1 text single-style.
7. **GPU compatibility.** vello relies on compute shaders. Omapix already
   runs wgpu on NVIDIA under Hyprland, so the risk is low; `vello_hybrid`
   or `vello_cpu` are the fallbacks.

## Testing

- `omavec-geom` and `omavec-engine`: unit tests, plus property and fuzz
  tests for booleans, offsets and the vector network (they must never
  panic and must keep areas consistent).
- Rendering: golden PNGs through `vello_cpu`, compared with a tolerance.
- SVG: round-trip tests (import → export → import is stable) against a
  corpus of real icons and logos.
- UI: the egui `Harness` pattern from Omapix, and `OMAVEC_SCRIPT` replays
  for whole workflows.
- `.fig`: fixture files with expected node trees.

## Licensing

GPL-3.0-or-later, like Omapix and Omacull. Code taken from VectorCraft
(MIT OR Apache-2.0) names its source in a header comment, and `NOTICE`
carries VectorCraft's copyright and licence.
