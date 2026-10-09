# Contributing to Omavec

Omavec is a small, opinionated project, built around one designer's logo
and UI work and tested on one machine. It is at an early stage: read
[docs/ROADMAP.md](docs/ROADMAP.md) to see what exists.

Issues and pull requests go to
[github.com/michaelrubi/omavec](https://github.com/michaelrubi/omavec).

## Ways to help

- **Try it on your machine.** It has only run on Omarchy (Arch, Hyprland)
  with an NVIDIA GPU. A report from an AMD or Intel GPU, another Wayland
  compositor, X11, a tablet, or another distribution is worth having, even
  if all it says is "it works".
- **Report bugs.** See below for what to put in one.
- **Say where it differs from Figma.** A shortcut, a modifier key or a tool
  that doesn't behave as Figma's does is a bug here, not a matter of taste.
  The same goes for Illustrator's behaviour in the tools Figma lacks (Shape
  Builder, Offset Path, the Width tool).
- **Write code.** The roadmap lists what's next, and the "Later:" lines
  under finished items are mostly small and self-contained.

## Reporting a bug

Open an issue with:

- what you did, what you expected, and what happened
- the commit you built (`git rev-parse --short HEAD`)
- your distribution, compositor and GPU
- for a problem with one file: the `.omavec` folder, SVG or `.fig` itself
  if you can share it
- for a crash or anything odd: the output of running Omavec from a
  terminal with `RUST_LOG=info omavec`

## Before you write code

Open an issue first for anything bigger than a fix. Omavec says no to a
lot, and it's better to find that out before the work than after.
[docs/DESIGN.md](docs/DESIGN.md) has the principles in full; in short:

- **One app for logos and screens.** A feature that only makes sense if you
  also own Figma or Illustrator is a bug.
- **Figma muscle memory.** If Figma has the feature, Omavec copies its
  name, shortcut and behaviour.
- **Non-destructive, bakeable.** Booleans, offsets, strokes and warps stay
  live until Flatten or Outline Stroke.
- **Clean output.** Curves stay curves, and exported SVG ships as it is.
- **Lightweight and local.** A feature has to justify what it costs in
  startup time, memory and dependencies, and Omavec never opens a network
  connection.

Not in scope: real-time collaboration, raster painting (that's
[Omapix](https://github.com/michaelrubi/omapix)), print and CMYK, and
prototyping.

## Building and testing

You need a recent stable Rust, fontconfig and Vulkan.

```bash
cargo test
```

```bash
cargo clippy --workspace --all-targets
```

Both should pass with no warnings before you open a pull request; CI runs
them too. The tests need no display or GPU.

To try a change by hand, `cargo run --release`, or `make install` to
replace the copy in `~/.local/bin`.

## How the code is laid out

```
crates/
  omavec-geom     curve maths: vector networks, booleans, offsets, stroke
                  expansion, warps. No documents, no UI, no GPU.
  omavec-engine   the document: node tree, layout, text, components, undo,
                  the .omavec format, SVG. No UI and no GPU, so all of it is
                  tested headless.
  omavec-render   display list to vello: GPU for the canvas, CPU for
                  headless export and golden-image tests.
  omavec-fig      best-effort .fig importer.
  omavec          the app: egui on wgpu, canvas, tools, commands, panels,
                  theme, CLI.
```

- **Keep the engine and geometry free of UI and GPU code.**
- **Everything the user can do is a `Command`**
  (`crates/omavec/src/commands.rs`), so menus, shortcuts, the command
  palette, `OMAVEC_SCRIPT` and the CLI can't drift apart.
- **Test what you add.** Geometry gets property tests; rendering gets
  golden images through `vello_cpu`; UI behaviour is tested by driving egui
  with made-up events, as the tests in `crates/omavec/src/app.rs` do.
- **Look in VectorCraft first.** Before writing geometry from scratch, read
  "Borrowing from VectorCraft" in the roadmap. Code ported from it names
  its source file and commit in a header comment.
- **Keep changes small.** No new abstraction until something needs it, and
  no new dependency without a reason that's worth its build time.
- **Don't run `cargo fmt`.** Match the code around yours, and turn off
  format-on-save.

## Pull requests

- One feature or fix in each, on a branch from `main`.
- Commit messages say what changed for the user: "Outline Stroke",
  "Shape Builder: delete edges with Alt".
- If the change finishes something on the roadmap, strike it through there
  and write what was built in its place, with anything left over as a
  "Later:" line. If it adds a shortcut, add it to the table in DESIGN.md.
- Say how you tested it, and on what hardware.

[AGENTS.md](AGENTS.md) has the same rules in short, for coding agents.

## Licences

Omavec is GPL-3.0-or-later, and contributions are taken under the same
licence. Code from elsewhere is fine from GPL-compatible projects; say
where it came from in the file it lands in, and add it to
[NOTICE](NOTICE). New dependencies need GPL-compatible licences.
