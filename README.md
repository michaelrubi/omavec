# Omavec

A local-first vector design app for [Omarchy](https://omarchy.org). It
combines Figma's frames, auto layout and components with the Illustrator
tools people leave Figma for: Shape Builder, Offset Path, Outline Stroke,
variable-width strokes and envelope distort. Use it for logos and screens
without switching apps.

> Omavec is an independent project. It is not made by or affiliated with
> Omarchy.

**Status:** early. You can draw frames, rectangles and ellipses, move,
resize and recolour them, undo, save to a `.omavec` folder, and export
frames as SVG and PNG with `omavec export`. That is all so far. The plan:

- [docs/DECISIONS.md](docs/DECISIONS.md): what Omavec is and isn't, and why
- [docs/DESIGN.md](docs/DESIGN.md): the architecture
- [docs/ROADMAP.md](docs/ROADMAP.md): the order of work, phase by phase

## The idea in one table

| Concept | Illustrator | Figma | Omavec |
| --- | --- | --- | --- |
| Artboards | Fixed canvas regions | Nested frames with auto layout | Frames are container nodes, optionally with auto layout |
| Paths | Two-ended Bézier paths | Vector networks | Vector networks, rendered as Bézier paths |
| Strokes | Variable-width profiles | Uniform, inside/centre/outside | Inside/centre/outside **and** width profiles |
| Booleans | Destructive Pathfinder, Shape Builder | Live boolean groups | Live boolean groups **and** Shape Builder |
| Effects on geometry | Appearance panel (offset, warp) | — | Live modifier stack, baked with Flatten |

## Built on

Rust, [egui](https://github.com/emilk/egui) for the UI (like Omapix and
Omacull), and Linebender's [vello_cpu](https://github.com/linebender/vello)
(rendering), [kurbo](https://github.com/linebender/kurbo) (curves),
[parley](https://github.com/linebender/parley) (text) and
[linesweeper](https://github.com/jneem/linesweeper) (booleans), with
[taffy](https://github.com/DioxusLabs/taffy) for auto layout. Booleans,
Shape Builder, offsets and stroke outlines come from
[VectorCraft](https://github.com/storytold/vectorcraft)'s geometry crates.

## Licence

GPL-3.0-or-later. See [LICENSE](LICENSE).
