# Omavec design

Omavec is a local-first vector design app for Omarchy: Figma's frames,
auto layout and components, plus the handful of Illustrator tools people
leave Figma for (Shape Builder, Offset Path, Outline Stroke, variable-width
strokes, envelope distort, scissors and knife). It should feel like Figma to
someone with Figma muscle memory, and start and run like a native Omarchy
app.

It is an independent project, not part of Omarchy.

Status: Phase 1 (document core and canvas). The decisions behind this document are in
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
  omavec-render   display list → pixels with vello_cpu: on a worker thread
                  for the canvas, and directly for headless PNG export and
                  golden-image tests. (No GPU.)
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
  or number can be bound to a variable. A gradient's two points are places
  in the node's box, a unit square, so it follows the node when it is
  resized or turned, and a radial one is as wide and as high as the box
  makes it; the display list carries the transform from that square to
  the page with each fill.
- **Strokes**: a stack of paints plus one stroke style: weight, align
  (inside/centre/outside), cap, join, miter limit, dashes, and an optional
  **width profile** (widths at positions along each segment). A stroke is
  drawn as the area it covers: a centred stroke's outline, and for inside
  or outside a stroke twice as wide cut to the half inside or outside the
  shape. SVG has only centred strokes, so those export as `stroke`
  attributes and the other two as the area, a filled path. A path that
  isn't closed has no inside, so its stroke is centred, and each end has
  a cap: none, round, square, or an arrowhead (open or filled) that is
  joined to the stroke's area.
- **Effects**: drop shadow, inner shadow, layer blur, background blur.
- **Modifiers**: an ordered, live list of geometry operations: Offset Path,
  Outline Stroke, Warp/Envelope, Round Corners, Simplify. This is
  Illustrator's Appearance panel in Figma's node tree. Flatten bakes the
  whole stack into a plain `Vector`.

What is built of that tree so far: `Page`, `Frame`, `Group`, and the
shapes as kinds of their own: `Rectangle`, `Ellipse`, `Arc` (an ellipse
with part of it gone or a hole in it; the panel turns one into the other),
`Polygon`, `Star` and `Line`. A node has a radius for each corner, used by
frames and rectangles. A line lies along the top of its box, which has no
height; it is drawn by its stroke, and its two ends are its handles.
`Node::shape` gives any of them as a path, from `omavec_geom::shapes`.
kurbo's own ellipse is not a closed path, so ours is the arc function
going all the way round: a stroke needs to know a shape is closed to have
an inside.

A group has no size of its own: its box is whatever holds its children
(`Node::bounds`), so moving a child changes the group's box without a
second edit to keep in step. Resizing a group, or several nodes at once,
resizes each node in it (`Node::stretch`): a node keeps its angle, its
middle moves, and its sides grow by as much as the stretch grows along
each, so nothing is ever skewed and strokes keep their weight. What is in
a frame stays put until Phase 5's constraints.

Grouping, ungrouping, duplicating, restacking, copying and pasting
(`omavec-engine/src/arrange.rs`) all leave every node where it was on the
page; a property test does them in random order and checks.

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
  (smallest cycles, as in Figma's paint bucket; "Phase 0 findings" below
  has the algorithm) and can carry their own fills, so one network can
  hold a multi-colour icon. A region is a list of loops, each a list of
  segments with a direction, so it can have holes.
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
engine document ──► display list ──► omavec-render ──► frame (RGBA) ──► egui texture
   (dirty nodes)     (per node,        (vello_cpu, on the       (uploaded when      (canvas panel)
                     cached)            canvas's worker thread)   it changes)
```

- `vello_cpu` draws the canvas on a worker thread, straight from curves,
  so zooming never shows stale tiles. The UI thread only uploads the
  finished frame as a texture, so a heavy document never stalls a panel or
  a drag.
- While a frame is on its way, the last one is shown moved and scaled to
  where its pixels now belong, so panning and zooming answer at once and
  sharpen a moment later. The worker draws only the newest view asked for.
- `vello_cpu` keeps no scene between frames, so the renderer skips what is
  off screen before drawing; that is most of the cost of a zoomed-in view.
- Nothing is drawn while nothing changes: an idle canvas uses no CPU.
- The display list is fills between two kinds of bracket. `Clip` … `Unclip`
  goes round the children of a frame that clips (not its own fill or
  stroke, which may sit outside its edge) and becomes `vello_cpu`'s
  `push_clip_path`. `Fade` … `Unfade` goes round a node whose opacity is
  below 1 and which draws more than one thing (two paints, or children),
  and becomes an opacity layer, so where its parts overlap neither shows
  through the other. The same layer carries the node's blend mode, and a
  node with one is always drawn as a layer. A node with one paint and nothing else is just that
  much fainter, with no layer. SVG says the same things with `clip-path`
  and group `opacity`, and a test holds the two renderings together.
- Headless export (CLI, tests) uses the same renderer on the calling
  thread, so an export is what the canvas showed. Golden images need no
  GPU.
- Blurs and shadows are `vello_cpu` filter layers. Those only work on a
  single-threaded context, so each effect is drawn on its own small
  context, cropped to what it can reach, and drawn into the frame as an
  image, as VectorCraft does (`crates/render/src/fx.rs`). The result is
  cached per node and zoom. A shadow on a rounded rectangle, the common
  case in UI work, takes `vello_cpu`'s analytic shortcut instead.
- Canvas overlays (selection handles, smart guides, Shape Builder
  highlights, vector edit handles) are drawn by egui's painter on top of
  the frame, so they follow the pointer without waiting for a render.

Vello on the GPU was the first plan and was measured against this in
Phase 0; see "Phase 0 findings" below for the numbers and why it lost.
`vello_hybrid` (the same CPU front end with GPU compositing) is the
upgrade path if 4K canvases get slow: its API is close to `vello_cpu`'s.

### Undo

Documents are trees of `Arc` nodes with copy-on-write, so an undo snapshot
shares every unchanged node (the same idea as Omapix's tiles). Each command
is one undo step; dragging coalesces into one step on release.

`History` holds the document. `edit` makes one change as one step, and
leaves no trace if the change fails. A tool calls `begin` when a drag
starts, edits as often as the pointer moves, and `commit`s on release or
`cancel`s on Esc. Every change gets a new revision number and an undo
brings the old number back, so the canvas redraws when the number changes
and the document is "dirty" when it isn't the number that was saved.

### Files

A document is a folder:

```
logo.omavec/
  document.json        format version, the pages in order, the next node id
  pages/
    01-cover.json      node tree for one page, pretty-printed
  assets/
    3f9a…c2.png        images by content hash (shared across pages)
  thumbnail.png        for file pickers
```

A node in a page file, with whatever has its usual value left out (shown,
unlocked, opaque, not transformed, no children), so a diff shows only what
someone changed:

```json
{
  "id": 3,
  "type": "rectangle",
  "name": "Rectangle",
  "transform": [1.0, 0.0, 0.0, 1.0, 20.0, 40.0],
  "size": { "width": 100.0, "height": 50.0 },
  "fills": [{ "type": "solid", "color": "#d9d9d9" }]
}
```

(The real files put each number on its own line.) Saving rewrites only
the files whose contents changed, removes the files of pages that were
deleted or renamed, and touches nothing else in the folder. Opening
refuses a format newer than it knows, and says which file and what is
wrong when a folder is damaged or a merge has left two nodes with one id.
Variables, styles, assets and the thumbnail aren't written yet.

- Text formatting is deterministic (sorted keys, fixed float formatting),
  so saving an unchanged document changes nothing in git.
- Fonts are referenced by family/style, not embedded.
- A `.omavecz` is the same folder zipped, for sending files to people and
  for opening from a file manager. Omavec opens and saves both; the folder
  is the one to keep in git.
- Crash recovery never writes into the document: an unsaved one may have
  no folder yet, and a folder in git shouldn't change behind its owner's
  back. Each session keeps one `.omavecz` copy named after its process id
  in `~/.local/state/omavec/recovery/`, with a note beside it of where the
  document belongs. A copy whose process is gone is what a crash leaves;
  the next session offers it back, as a document with unsaved changes.

### Import and export

- **SVG import** through `usvg`, which normalises the SVG, into vector
  networks, frames and groups, keeping ids and names.
- **SVG export** written by us (not `usvg`) for minimal output: merged
  transforms, shortest path data, optional `currentColor`, per-frame or
  per-selection.
- **PNG/JPEG/WebP export** at @1x/@2x/@3x presets, per node, like Figma's
  export settings. A node's settings are a list in the file (`exports`:
  SVG, or PNG at a scale; JPEG and WebP to come), and the app's Export
  command and `omavec export` are one function
  (`crates/omavec/src/export.rs`), so they can't write different files.
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
omavec export file.omavec --out dist/                every frame, as its export settings say
omavec run "Frame 0 0 400 300,Export dist" [file]    a script's steps, with no window
omavec import design.fig --out design.omavec         .fig conversion + report
omavec tokens file.omavec --format css|tailwind|omarchy
```

The CLI uses the same engine, renderer and commands as the app, with no
window and no GPU. `omavec file.omavec` and `omavec export` exist; the
other three are planned.

## Omarchy integration

- **Theme**: colours come from the active Omarchy theme and update live,
  ported from Omapix's `theme.rs`. Theme colours are for the UI only; the
  canvas shows document colours unchanged, on a neutral backdrop.
- **Wayland and Hyprland**: native Wayland through winit. Panels can be
  torn off into their own windows (egui viewports), so Hyprland can tile
  them. Tablet pressure through the Wayland tablet protocol, ported from
  Omapix's `tablet.rs`, for the pencil and width tools.
- **Keyboard first**: Figma shortcuts; a command palette that lists every
  `Command` (Ctrl+K, Ctrl+/); `:` to open it as a command line that takes
  arguments; and hjkl nudging as a setting, off by default, because H, K
  and L are Figma's Hand, Scale and Line.
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
| I | Eyedropper | Enter / Shift+Enter | Select children (later: edit vector) / select parent |
| Ctrl+G / Ctrl+Shift+G | Group / ungroup | Ctrl+Alt+G | Frame selection |
| Ctrl+D | Duplicate | Ctrl+C / Ctrl+X / Ctrl+V | Copy / cut / paste |
| ] / [ | Bring to front / send to back | Ctrl+] / Ctrl+[ | Bring forward / send backward |
| Ctrl+A | Select all |  |  |
| Shift+A | Add auto layout | Ctrl+Alt+K | Create component |
| Ctrl+E | Flatten | Ctrl+Shift+O | Outline stroke |
| Ctrl+Shift+H | Show/hide | Ctrl+Shift+L | Lock/unlock |
| Shift+1 / Shift+2 | Zoom to fit / selection | Ctrl+K, Ctrl+/ | Command palette |
| Ctrl+= / Ctrl+- | Zoom in / out | Shift+0 | Zoom to 100% |
| Ctrl+\ | Show/hide UI | Ctrl+Q | Quit |
| Ctrl+Z / Ctrl+Shift+Z | Undo / redo | Delete, Backspace | Delete |
| Ctrl+N / Ctrl+O | New / open | Ctrl+S / Ctrl+Shift+S | Save / save as |
| Ctrl+Shift+E | Export |  |  |
| Esc | Give up the drag, then the tool, then the selection | Arrows / Shift+arrows | Nudge by 1 / 10 |
| Shift+R | Rulers | Shift+' | Pixel grid |
| Ctrl+Shift+' | Snapping |  |  |

On the canvas: the wheel pans, Ctrl+wheel or a pinch zooms about the
pointer, and middle drag or Space+drag pans.

Selecting, with the Move tool: a click selects the deepest node under the
pointer that is a child of the page, of a top-level frame, or of whatever
holds something already selected; Ctrl+click selects the deepest there
is, and a double click goes one deeper. Shift adds, or takes away on
release. A drag from the bare page, or from the background of a top-level
frame that has things in it, is a marquee: it selects what it touches of
the page's children, and of a top-level frame's unless the frame is wholly
inside it. The selection's box resizes from its corners and edges (Shift
keeps proportions, Alt about the middle) and turns from just outside a
corner (Shift by 15°). Dragging the selection moves it; Shift keeps the
move on one axis and Alt moves a copy.

The command palette (Ctrl+K, Ctrl+/ or a colon) lists every command and
filters as you type; what is typed that is no command's name is taken as a
line of script (`crates/omavec/src/script.rs`), which is the `:` command
line.

Copy puts the nodes on Omavec's own clipboard and offers them to other
apps as `image/svg+xml` through `wl-copy`. Paste takes only Omavec's own:
into the selected frame or group, or beside the selection, or onto the
page, where the nodes were on the page when copied.

## Open-source references

What to take from each, and how. Licences are checked before any code is
copied; the crates below are all MIT/Apache, which work with Omavec's
GPL-3.0.

| Project | Take | How |
| --- | --- | --- |
| Linebender (`kurbo`, `vello_cpu`, `parley`, `fontique`, `peniko`, `linesweeper`) | Curve maths, rendering, text, booleans | Dependencies |
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

## Phase 0 findings

What the spikes measured, on the machine Omavec is built on (i9-13900H,
Intel Iris Xe and an RTX 4070 Laptop GPU, 2560 × 1440). The code is in
each crate's `examples/`.

### Canvas renderer: `vello_cpu`

`examples/canvas_bench.rs` (deleted with its `vello` dependency once the
canvas could draw documents; it is in the history at `ba407d5`) drew 10,000
random cubic blobs (translucent, overlapping, scattered over 8,000 units)
into a 2560 × 1440 frame with both renderers and takes the median of 30
frames. Times are milliseconds per frame.

| 10,000 paths | 100% (648 on screen) | 6400% (6) | 5% (10,000) |
| --- | --- | --- | --- |
| `vello_cpu`, 8 threads, off-screen paths skipped | 2.0 | 1.3 | 2.7 |
| `vello_cpu`, 1 thread, off-screen paths skipped | 5.9 | 3.1 | 11.3 |
| `vello_cpu`, 8 threads, every path | 2.5 | 4.9 | 2.7 |
| vello on Iris Xe, whole scene appended | 4.2 | **wrong frame** | 4.2 |
| vello on Iris Xe, off-screen paths skipped | 3.1 | 3.1 | 4.9 |
| vello on RTX 4070, whole scene appended | 1.0 | **wrong frame** | 1.4 |
| vello on RTX 4070, off-screen paths skipped | 0.9 | 1.0 | 1.9 |

| 100,000 paths | 100% (6,522) | 6400% (40) | 5% (100,000) |
| --- | --- | --- | --- |
| `vello_cpu`, 8 threads, off-screen paths skipped | 11.1 | 3.0 | 21.5 |
| vello on Iris Xe, off-screen paths skipped | 16.1 | 7.5 | 35.9 |
| vello on RTX 4070, off-screen paths skipped | 3.2 | 2.0 | 15.5 |
| vello, whole scene appended (either GPU) | **wrong frame** | **wrong frame** | 30.4 / 9.0 |

Uploading a `vello_cpu` frame to the GPU adds 2.6 ms (Iris Xe) to 3.6 ms
(RTX) at this size. The two renderers' frames match to within 0.7% RMSE.

Decision: **`vello_cpu` on a worker thread.**

- Omavec opens on the integrated GPU, as Omapix does, to keep a laptop's
  discrete GPU asleep. There, vello is no faster than `vello_cpu` plus its
  upload: 3–5 ms against 4–5 ms at 10,000 paths, and slower at 100,000.
  Only the discrete GPU is clearly ahead, and every case is already inside
  a 60 Hz frame at 10,000 paths.
- vello 0.11's GPU buffers have fixed sizes
  (`vello_encoding::BufferSizes`: 2²¹ lines, segments and tiles, 2¹⁸ bin
  entries). A scene that needs more draws a wrong frame or leaves the last
  one on screen, and `render_to_texture` returns `Ok`. Zooming to 6400%
  with 10,000 paths in the scene does it, because off-screen paths are
  flattened at full size. Skipping them first avoids that case, but a busy
  visible scene can still overflow, and the only way to find out is a
  debug feature that reads buffers back every frame.
- vello on the GPU can blur only rounded rectangles. `vello_cpu` has blur
  and drop-shadow filter layers for any shape (below).
- One renderer draws the canvas, CLI exports and golden-image tests, so
  they cannot disagree, and none of them needs a GPU. vello and its
  shaders leave the build, and so does the need to keep vello and egui on
  the same wgpu.

What it costs: panning a heavy document uses several cores instead of the
GPU; a 4K canvas has 2.25 times the pixels to draw and upload; and
`vello_cpu` is young (0.3). egui itself links `vello_cpu` 0.1 for its own
drawing, so two versions are in the tree.

### Blurs and shadows: `vello_cpu` filter layers, drawn off to the side

`cargo run --release -p omavec-render --example effects` puts a drop
shadow and a layer blur on a star with curved sides. Each effect is drawn
on its own single-threaded context, cropped to its reach, then drawn into
the frame as an image.

| Effect on one shape | σ 4 | σ 16 | σ 64 |
| --- | --- | --- | --- |
| Drop shadow, 300 px star | 2.4 ms | 2.1 ms | 6.4 ms |
| Layer blur, 300 px star | 2.1 ms | 1.6 ms | 4.6 ms |
| Drop shadow, 1,200 px star | 23 ms | 21 ms | 29 ms |
| Layer blur, 1,200 px star | 27 ms | 15 ms | 18 ms |

Compositing six of them into a frame costs about 1 ms, and a blurred
rounded rectangle (`fill_blurred_rounded_rect`) costs nothing measurable.
So: icon-sized effects are cheap, screen-sized ones are not, and the image
must be cached per node and zoom and reused while panning. Effects on
different nodes can be drawn in parallel, since each has its own context.
Inner shadow and background blur were not prototyped; VectorCraft draws
inner glows as a blurred inverse silhouette clipped to the shape, which is
the plan for inner shadows.

### `vectorcraft-pathops`: use it as it is

`cargo run --release -p omavec-geom --example pathops` runs the booleans
and Shape Builder regions and checks every result against the inputs on a
300 × 300 grid of points, without using the library to do it.

| Case | Time | Anchors in → out | Wrong grid points |
| --- | --- | --- | --- |
| Two circles: union, intersect, subtract, exclude | 63 µs each | 14 → 7–18 | 0.00% |
| Twenty overlapping curved shapes: union | 4.2 ms | 174 → 21 | 0.00% |
| Twenty shapes: exclude (odd count) | 3.9 ms | 174 → 1,046 in 207 pieces | 0.03% |
| Twenty shapes: first minus the rest | 3.8 ms | 174 → 17 | 0.00% |
| Three-circle Venn: regions | 0.15 ms | 7 regions, 53 anchors | 0.00% |
| Twenty shapes: `regions` / `shape_builder` with edges | 7.3 ms / 18 ms | 425 regions, 2,066 anchors | not checked |
| Rectangles sharing a whole or part edge | 5 µs | 8 → 4–8, or nothing | 0.00% |
| Identical circles | 66 µs | 14 → 7 or nothing | 0.00% |
| Circles touching outside, and inside | 45 µs | 13 → 6–13, or nothing | ≤ 0.03% |
| Circle on a rounded rectangle's corner arc | 240 µs | 18 → 10–15 | ≤ 0.04% |

No panics and no errors, areas agree with the closed forms to five digits,
and `A − B` is not confused with `B − A`. Results stay curves and come
back with about as many anchors as went in. Live booleans can re-evaluate
on every drag frame; Shape Builder's arrangement of a busy selection
(18 ms) is built once when the tool starts, not per pointer move. Neither
`i_overlay` nor raw `linesweeper` is needed. Its kurbo (0.13.1) is the one
`vello_cpu` and peniko use.

### Vector networks: faces from a walk round each vertex

`omavec_geom::network` holds the spike: `VectorNetwork` (vertices,
segments with tangents relative to their vertices, regions as loops of
half-edges), `faces()`, and conversion to and from `BezPath` and
VectorCraft's `PathData`.

- **Finding faces.** Sort the half-edges leaving each vertex by angle.
  Walking a half-edge and then always taking the next one clockwise from
  the way back traces one face; every half-edge is on exactly one walk.
  Walks with positive area are faces, and the one negative walk per
  connected piece is its outside. Dead ends drop out of a walk (a segment
  walked there and back bounds nothing), and a piece that sits inside
  another piece's face becomes a hole in the smallest face that holds it.
- **Checked against a flood fill.** On random grids of straight edges, a
  flood fill over the cells says which cells are enclosed and which belong
  together, with no geometry involved. `faces()` must give exactly those
  areas, holes and islands included. On random bent grids with diagonals
  it must satisfy Euler's formula (faces = segments − vertices + pieces)
  with no two faces overlapping. Both hold over 20,000 random graphs, and
  networks full of nonsense (missing vertices, NaN, loops on one vertex)
  never panic.
- **Paths.** `to_bezpath` writes each region's loops as closed subpaths
  and the segments no region uses as open runs; `from_bezpath` merges
  points that coincide into one vertex and an edge two subpaths share into
  one segment, so two squares side by side come in as six vertices and
  seven segments. A network goes out through `PathData` and comes back
  with the same vertices, segments and faces. `stroke_path` gives every
  segment once, joined into the longest runs, which is what a stroke
  follows.
- **Speed** (`cargo run --release -p omavec-geom --example network`):
  faces of a 50 × 50 mesh (4,900 segments, 2,401 faces) in 1.3 ms, of a
  200 × 200 mesh (79,600 segments) in 23 ms, and of 2,500 islands inside
  one face in 7.5 ms.
- **Left for Phase 2.** Segments that cross without a vertex (run them
  through `vectorcraft-pathops`' planar map first), curves that leave a
  vertex in exactly the same direction and curvature, per-vertex corner
  radius and handle mirroring, and an R-tree instead of the hash grid that
  merges points.

### Text: parley, fontique and skrifa fit together

`cargo run --release -p omavec-render --example text` lays out one line at
48 px in the system's sans-serif and in JetBrains Mono, draws it with
`vello_cpu`'s glyph runs, turns the same glyphs into one `BezPath` with
skrifa, and compares the two renderings pixel by pixel.

- fontique resolves the same files `fc-match` does, for a generic family
  and for a family by name. Opening the system collection (798 fonts
  here) takes 15 ms, once.
- parley lays the line out in 0.1 to 0.5 ms the first time a font is used
  and 7 to 9 µs after that. Kerning applies. Neither font has `fi` or
  `fl` ligatures, so those weren't exercised.
- Outlines for the 29 glyphs take 5 µs and match the glyph-run rendering:
  0.000% and 0.002% of pixels differ. Font units are y-up, so the pen
  flips y as it goes.
- `vello_cpu` 0.3, parley 0.12 and skrifa 0.44 share one skrifa,
  read-fonts and peniko, so a font from parley's run goes straight into
  `glyph_run` with no conversion.
- For the text tool: `glyph_run` hints by default, which moves outlines
  by up to a pixel; Omavec draws text with `.hint(false)` so the canvas
  matches the exported outlines. A line can come back as several glyph
  runs when a character falls back to another font; draw and outline
  every run. fontique's `Query` borrows the collection, so read family
  names after the query is dropped.

### `.fig`: `kiwi-schema` reads current files

`omavec_fig::decode` reads four real files saved between 2022 and 2026
(file versions 20, 48 and 106; 222 to 558 schema definitions) and
reproduces the node trees that fig2sketch's own decoder gives, exactly.
`cargo run -p omavec-fig --example fig_dump -- file.fig` prints one.

- `kiwi-schema` 0.2.1 already handles the `int64` and `uint64` fields
  newer schemas use. Nothing had to be patched.
- A `.fig` is a zip holding `canvas.fig`, `thumbnail.png`, `meta.json` and
  `images/`; older ones can be the bare `canvas.fig`. That file is
  `fig-kiwi`, a version, then two length-prefixed chunks: the schema and
  the message. Version 106 compresses the schema with raw deflate and the
  message with zstd; older files use deflate for both.
- Children are ordered by `parentIndex.position`, a fractional-index
  string compared as text, not a number.
- Decoding is fast: 0.3 ms for a 100 KB file.

## Risks

1. ~~**Blurs and shadows in vello.**~~ Settled in Phase 0: `vello_cpu`
   filter layers draw them on any shape. What remains is their cost on
   large shapes (15–30 ms each at 1,200 px), which caching per node and
   zoom has to hide.
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
7. ~~**GPU compatibility.**~~ Gone with the move to `vello_cpu`: the GPU
   only shows a texture, which egui already does for the rest of the UI.
8. **CPU rendering at 4K.** `vello_cpu` holds 60 Hz at 2560 × 1440 with
   10,000 paths and falls to about 40 Hz with 100,000 all on screen. A 4K
   display has 2.25 times the pixels. Mitigation: the last frame is
   reprojected while the next one draws, so interaction never waits;
   `vello_hybrid` if that isn't enough.

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
