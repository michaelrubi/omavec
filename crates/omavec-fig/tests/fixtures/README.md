# .fig fixtures

Real Figma files ("Save local copy") for the importer's tests.

| File | Saved | Covers |
| --- | --- | --- |
| `vector.fig` | 2022 | one vector network |
| `structure.fig` | 2024 | groups, a component and instances, images, vectors |
| `corners.fig` | 2026 | corner radii and smoothing |
| `stacks_wrap.fig` | 2026 | auto layout with wrapping |

They come from [fig2sketch](https://github.com/sketch-hq/fig2sketch)'s
`tests/data` (commit `4eddfd4`), which is MIT-licensed, copyright (c) 2022
Sketch B.V. and contributors.

Each `.tree.txt` is the file's node tree as fig2sketch's own decoder reads
it: one node per line as `TYPE session:local name`, children indented and in
order. They are the expected output for our decoder, made independently of
it. To make one for a new fixture, from a checkout of fig2sketch:

```bash
uv run --with zstd python path/to/tree.py path/to/file.fig
```
