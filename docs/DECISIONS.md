# Omavec scoping decisions

The answers from the scoping interview (8 October 2026), and what each one
means for the build. [DESIGN.md](DESIGN.md) turns these into an
architecture; [ROADMAP.md](ROADMAP.md) turns them into an order of work.
When a later decision overrides one of these, strike it through and add the
new one below it, so the history stays readable.

## Identity

| Question | Answer | Consequence |
| --- | --- | --- |
| Name | **Omavec** | Crates `omavec-*`, binary `omavec`, config in `~/.config/omavec/`, `omavec.desktop`. Pairs with Omapix (pixels ↔ vectors). |
| Primary use | **Logos/icons and UI screens, equally** | Neither half gets dropped. The roadmap alternates between them and has two "switch points" (logos first, then screens) so it's usable before it's finished. |
| Smallest version you'd switch to | **Draw and export a logo, lay out a screen, reusable components, open my Figma files** | This is the v1.0 bar. The full bar is large, so v0.1 (logos) and v0.2 (screens) are intermediate releases you can actually use. |
| Interop | **SVG round-trip, .fig import, code export** | No PDF/EPS/AI import. SVG export must be clean enough to ship as-is. |

## Stack

| Question | Answer | Consequence |
| --- | --- | --- |
| Relationship to Graphite | **Borrow crates and ideas, own the app** | Fresh codebase. Graphite, Inkscape (lib2geom) and Penpot are references; their code is only taken where licences allow and it fits. |
| Relationship to VectorCraft (added 8 October 2026) | **Depend on its geometry, port or read the rest** | VectorCraft (MIT OR Apache-2.0, egui 0.36, kurbo 0.13) is an open-source Illustrator clone in Rust. `vectorcraft-geom` and `vectorcraft-pathops` become dependencies for booleans, Shape Builder, offset, outline stroke and simplify; width profiles and warps are ported; its renderer, tools, SVG and text code are references. Omavec still owns its document, UI and format. Details in ROADMAP.md, "Borrowing from VectorCraft". |
| UI toolkit | **Match the other Oma apps** | egui 0.36 / eframe on wgpu, as in Omapix and Omacull. ~~The canvas is drawn by vello into a wgpu texture shown inside egui (both use wgpu 30).~~ The canvas is drawn by `vello_cpu` on a worker thread and shown as an egui texture (decided 9 October 2026 from Phase 0's benchmark: on the integrated GPU vello was no faster, it draws wrong frames without saying so when its fixed buffers overflow, and it can't blur arbitrary shapes; DESIGN.md, "Phase 0 findings"). |
| Native file format | **Git-friendly text** | A `.omavec` folder of pretty-printed JSON (one file per page) plus content-addressed assets. Stable node IDs so diffs are small. |
| Omarchy integration | **Live theme, Vim-style modal keys, Hyprland-aware, Omarchy install** | Port Omapix's `theme.rs`; command palette and keyboard-first editing; native Wayland, multi-window panels, tablet pressure; Makefile + PKGBUILD like the siblings. |

## Behaviour and scope

| Question | Answer | Consequence |
| --- | --- | --- |
| Shortcut muscle memory | **Figma** | Figma's tool letters and shortcuts by default. Illustrator-only tools take Illustrator's letters where they don't clash (Shift+M Shape Builder, C Scissors, Shift+W Width). Vim-style keys are layered on, not a replacement. |
| Typography in v1 | **UI-grade text** | Single-style text boxes, auto width/height/fixed, system fonts, basic OpenType features, Text → Outlines. Rich text and type on a path come later. |
| .fig import | **Best-effort converter** | One-way, offline importer for `.fig` files ("Save local copy" in Figma). Frames, vectors, text, auto layout, components. Unsupported features are listed in an import report, not silently dropped. |
| Non-goals | **Real-time collaboration, raster painting, print/CMYK prepress** | Single user, local files, git for history. Images are placed, never painted. sRGB (Display P3 maybe later), no separations. |
| Prototyping | **Someday / maybe** | Not on the roadmap. The document model doesn't preclude it (stable IDs, frames as screens), but no work is planned. |
| Design-system features | **Variants, variables / tokens** | Component sets with properties, and variables with modes. Shared libraries across files are not in v1. Named styles are included as the simple case. |
| Ecosystem hooks | **Headless CLI, scripted test harness** | `omavec export …` for asset pipelines, and an `OMAVEC_SCRIPT` command replay like Omapix's. No plugin API or Omapix round-trip in v1. |
| Working style | **Same as Omapix** | Claude builds roadmap items; Michael tests by hand from the `make install`ed binary; every non-trivial engine or UI change gets a headless test; `docs/ROADMAP.md` tracks status with ~~struck-through~~ items. |

## Still open

These came up while scoping and don't block Phase 0. Decide them when the
phase that needs them starts.

1. **Vim-style keys versus Figma letters.** Figma already uses H (hand),
   K (scale) and L (line), so hjkl can't move things by default. Proposal:
   a command palette (Ctrl+K, and Ctrl+/ as in Figma) plus a `:` command
   line, with hjkl nudging only in vector edit mode or behind a setting.
   Decide in Phase 1.
2. **Folder or single file.** A folder bundle diffs best in git but is
   awkward in file pickers and for `xdg-open`. Proposal: the folder is the
   canonical format; a zipped `.omavecz` is offered for sending to people.
   Decide in Phase 1.
3. **A shared `oma-ui` crate.** Theme loading, tablet input and the
   Wayland clipboard are now written (or about to be) in three apps.
   Extracting them into a shared crate is worth it once Omavec needs them,
   but it touches the other repos. Decide in Phase 1.
4. **Display P3.** Figma supports a P3 document profile. Not needed for
   v1; revisit when colour management comes up.
5. ~~**VectorCraft: git dependency or vendored copy.** A git dependency
   pinned to a commit gets upstream fixes with one line; a vendored copy
   of the two crates in `crates/` can't break under us and builds offline
   from the AUR without a git fetch. Proposal: git dependency, vendored if
   its API churns. Decide in Phase 0.~~
   **Decided (9 October 2026): git dependency.** `omavec-geom` depends on
   `vectorcraft-geom` and `vectorcraft-pathops` at commit `4cf912fa`. They
   pull in only kurbo, linesweeper, serde and thiserror, and share the
   tree's one kurbo. `cargo fetch --locked` in the PKGBUILD fetches the
   pinned commit, so the AUR build needs nothing extra. Vendor the two
   crates if upgrades get painful.
