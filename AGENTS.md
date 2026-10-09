# AGENTS.md

Omavec is a local-first vector design app for Omarchy: Figma's frames, auto layout and components plus Illustrator's logo tools (Shape Builder, Offset Path, Outline Stroke, width profiles, envelope distort). It is designed to match Figma muscle memory and feel native to Omarchy. Sibling apps: Omapix (raster, `michaelrubi/omapix`) and Omacull (culling, `michaelrubi/omacull`). Follow their conventions unless a doc here says otherwise. VectorCraft (`storytold/vectorcraft`, an MIT/Apache Illustrator clone in Rust) supplies our geometry crates and is the first reference for Illustrator-side features; see "Borrowing from VectorCraft" in `docs/ROADMAP.md` before writing geometry from scratch.

**Status: Phase 0 (foundations and spikes).** Read `docs/DECISIONS.md`, `docs/DESIGN.md` and `docs/ROADMAP.md` before starting work. Work through `docs/ROADMAP.md` in order, and strike items through with "(done)" when finished.

## Principles

- **One app for logos and screens**: a feature that only works if you also own Figma or Illustrator is a bug.
- **Figma muscle memory**: shortcuts, tool letters, panels and canvas behaviour match Figma; Illustrator-only tools use Illustrator's letters where they don't clash.
- **Non-destructive, bakeable**: booleans, offsets, strokes and warps stay live until Flatten / Outline Stroke.
- **Clean output**: curves stay curves; exported SVG is shippable as-is.
- **Engine/UI separation**: `omavec-geom` and `omavec-engine` have no UI or GPU dependencies and are 100% headlessly testable.

## Architecture

```
crates/
  omavec-geom/     vector networks, booleans, offsets, stroke expansion, warps (kurbo, linesweeper)
  omavec-engine/   document tree, layout (taffy), text (parley), components, variables, undo, .omavec IO, SVG
  omavec-render/   display list → vello Scene (GPU), vello_cpu for headless export and golden tests
  omavec-fig/      best-effort .fig importer (kiwi-schema)
  omavec/          egui app on wgpu, canvas, tools, commands, panels, Omarchy theme, CLI
```

- **Commands**: every action goes through `Command` in `crates/omavec/src/commands.rs`, so menus, shortcuts, the command palette, `OMAVEC_SCRIPT` and the CLI never diverge.
- **Versions**: egui/eframe 0.36 and vello 0.11 share wgpu 30. Keep them in step when upgrading.

## Build and Test Commands

```bash
cargo test                 # engine, geometry and headless UI tests
cargo build --release
make install               # installs to ~/.local/bin (the copy Michael actually runs)
cargo run --release -- file.omavec
OMAVEC_SCRIPT="Rectangle 0 0 100 100,Ellipse 50 50 100 100,BooleanUnion" cargo run --release
omavec export file.omavec --frame Logo --format svg,png@2x --out dist/
```

## Conventions

- **Code edits**: keep diffs minimal and surgical. Do not introduce speculative abstractions or unnecessary dependencies.
- **Hand testing**: Michael tests from the installed binary, not `cargo run`. After a change he'll try by hand, run `make install` and ask him to restart Omavec.
- **Testing**: whenever non-trivial UI or engine logic is added, write a headless test. Geometry gets property/fuzz tests; rendering gets `vello_cpu` golden images; UI uses the egui `Harness` pattern from Omapix's `layers_panel.rs`.
- **Docs**: plain, concrete prose in the style of Omapix's docs. Update `docs/ROADMAP.md` and `docs/DESIGN.md` in the same change as the code.
- **Formatting**: output code blocks flush-left (zero indentation).
