//! Phase 0 spike: text stack end-to-end.
//!
//! Measures fontique resolution against fontconfig, parley layout at 48 px,
//! vello_cpu glyph_run rendering, and skrifa unhinted outline extraction.
//! Verifies pixel equivalence between glyph rasterization and outline fills.
//!
//!     cargo run --release -p omavec-render --example text
//!
//! With `DUMP=dir` writes `text.png` and `text.svg`.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use fontique::{GenericFamily, QueryFamily, QueryStatus, SourceKind};
use kurbo::{Affine, BezPath, Rect, Shape};
use parley::{
    Alignment, AlignmentOptions, FontContext, FontFamily, Layout, LayoutContext,
    PositionedLayoutItem, StyleProperty,
};
use peniko::Color;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlineGlyphCollection, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider};
use vello_cpu::{Pixmap, RenderContext, Resources};

const FONT_SIZE: f32 = 48.0;
const TEXT: &str = "Omavec fi fl AV Ty 0123 — ñ é";
const CANVAS_WIDTH: u16 = 900;
const CANVAS_HEIGHT: u16 = 360;

struct Target<'a> {
    label: &'a str,
    query: QueryFamily<'a>,
    family: FontFamily<'a>,
    fc_pattern: &'a str,
    y_glyph: f64,
    y_outline: f64,
}

// Flips Y because font design coordinates are Y-up (ascenders positive) while
// the canvas is Y-down.
struct OutlineBuilder {
    path: BezPath,
    offset: (f64, f64),
}

impl OutlinePen for OutlineBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to((self.offset.0 + f64::from(x), self.offset.1 - f64::from(y)));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to((self.offset.0 + f64::from(x), self.offset.1 - f64::from(y)));
    }
    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.path.quad_to(
            (self.offset.0 + f64::from(cx0), self.offset.1 - f64::from(cy0)),
            (self.offset.0 + f64::from(x), self.offset.1 - f64::from(y)),
        );
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.path.curve_to(
            (self.offset.0 + f64::from(cx0), self.offset.1 - f64::from(cy0)),
            (self.offset.0 + f64::from(cx1), self.offset.1 - f64::from(cy1)),
            (self.offset.0 + f64::from(x), self.offset.1 - f64::from(y)),
        );
    }
    fn close(&mut self) {
        self.path.close_path();
    }
}

fn build_outline(glyphs: &[vello_cpu::Glyph], outlines: &OutlineGlyphCollection<'_>, size: f32) -> BezPath {
    let mut pen = OutlineBuilder { path: BezPath::new(), offset: (0.0, 0.0) };
    for g in glyphs {
        pen.offset = (f64::from(g.x), f64::from(g.y));
        if let Some(glyph) = outlines.get(GlyphId::new(g.id)) {
            let _ = glyph.draw(DrawSettings::unhinted(Size::new(size), LocationRef::default()), &mut pen);
        }
    }
    pen.path
}

fn render_to_pixmap(width: u16, height: u16, draw: impl FnOnce(&mut RenderContext, &mut Resources)) -> Pixmap {
    let mut ctx = RenderContext::new(width, height);
    let mut res = Resources::new();
    ctx.set_paint(Color::WHITE);
    ctx.fill_rect(&Rect::new(0.0, 0.0, f64::from(width), f64::from(height)));
    ctx.set_paint(Color::BLACK);
    draw(&mut ctx, &mut res);
    ctx.flush();
    let mut pixmap = Pixmap::new(width, height);
    ctx.render(&mut pixmap, &mut res);
    pixmap
}

fn pixel_difference_pct(p1: &Pixmap, p2: &Pixmap) -> f64 {
    assert_eq!((p1.width(), p1.height()), (p2.width(), p2.height()));
    let (d1, d2) = (p1.data(), p2.data());
    let diff = d1.iter().zip(d2.iter()).filter(|(a, b)| {
        (i32::from(a.r) - i32::from(b.r)).abs() > 32
            || (i32::from(a.g) - i32::from(b.g)).abs() > 32
            || (i32::from(a.b) - i32::from(b.b)).abs() > 32
            || (i32::from(a.a) - i32::from(b.a)).abs() > 32
    }).count();
    (diff as f64 / d1.len() as f64) * 100.0
}

fn median_duration(mut runs: Vec<Duration>) -> Duration {
    runs.sort();
    runs[runs.len() / 2]
}

fn resolve_font(
    font_cx: &mut FontContext,
    query_family: QueryFamily<'_>,
    fc_pattern: &str,
) -> (String, String, String) {
    let mut matched = None;
    {
        let mut query = font_cx.collection.query(&mut font_cx.source_cache);
        query.set_families([query_family]);
        query.matches_with(|font| {
            matched = Some(font.family);
            QueryStatus::Stop
        });
    }

    let (mut resolved_family, mut resolved_path) = (String::new(), String::new());
    if let Some((family_id, font_index)) = matched
        && let Some(family) = font_cx.collection.family(family_id)
    {
        resolved_family = family.name().to_string();
        if let Some(font_info) = family.fonts().get(font_index)
            && let SourceKind::Path(ref p) = font_info.source().kind
        {
            resolved_path = p.to_string_lossy().to_string();
        }
    }

    let output = Command::new("fc-match").args(["-f", "%{family}\n", fc_pattern]).output().expect("run fc-match");
    let fc_families = String::from_utf8_lossy(&output.stdout);
    let matches = fc_families.split(',').map(str::trim).any(|s| s == resolved_family);
    let match_msg = if matches { "matches fc-match" } else { "DIFFERS from fc-match" };

    (resolved_family, resolved_path, match_msg.to_string())
}

fn layout_line(
    font_cx: &mut FontContext,
    layout_cx: &mut LayoutContext<[u8; 4]>,
    family: &FontFamily<'_>,
) -> Layout<[u8; 4]> {
    let mut builder = layout_cx.ranged_builder(font_cx, TEXT, 1.0, true);
    builder.push_default(StyleProperty::FontSize(FONT_SIZE));
    builder.push_default(StyleProperty::FontFamily(family.clone()));
    let mut layout = builder.build(TEXT);
    layout.break_all_lines(None);
    layout.align(Alignment::Start, AlignmentOptions::default());
    layout
}

fn test_font(
    target: Target<'_>,
    font_cx: &mut FontContext,
    layout_cx: &mut LayoutContext<[u8; 4]>,
    combined_canvas: &mut RenderContext,
    res: &mut Resources,
) -> BezPath {
    println!("{}:", target.label);
    let (_, path, match_msg) = resolve_font(font_cx, target.query, target.fc_pattern);
    println!("  font file: {path} ({match_msg})");

    // The first layout with a font loads and indexes it; the rest reuse that.
    let start = Instant::now();
    let layout = layout_line(font_cx, layout_cx, &target.family);
    let first_layout = start.elapsed();
    let layout_times: Vec<_> = (0..50).map(|_| {
        let start = Instant::now();
        let _ = layout_line(font_cx, layout_cx, &target.family);
        start.elapsed()
    }).collect();
    let median_layout = median_duration(layout_times);

    let line = layout.lines().next().expect("single line");
    let mut runs = line.items().filter_map(|item| match item {
        PositionedLayoutItem::GlyphRun(gr) => Some(gr),
        _ => None,
    });
    let glyph_run = runs.next().expect("glyph run");
    // A second run would mean a character fell back to another font, which
    // the rest of this spike would silently leave out.
    assert_eq!(runs.count(), 0, "the line needed a fallback font");

    let (num_chars, num_glyphs) = (TEXT.chars().count(), glyph_run.glyphs().count());
    let width = layout.width();
    let ascent = glyph_run.run().font_metrics().ascent;
    let descent = glyph_run.run().font_metrics().descent;
    println!("  layout: {num_chars} chars, {num_glyphs} glyphs, width {width:.2} px, ascent {ascent:.2} px, descent {descent:.2} px, first {first_layout:?}, then {median_layout:?} (median of 50)");

    let font_data = glyph_run.run().font();
    let font_ref = FontRef::from_index(font_data.data.data(), font_data.index).expect("valid font ref");
    let outlines = font_ref.outline_glyphs();
    let positioned: Vec<_> = glyph_run.positioned_glyphs().map(|g| vello_cpu::Glyph { id: g.id, x: g.x, y: g.y }).collect();

    combined_canvas.set_transform(Affine::translate((20.0, target.y_glyph)));
    combined_canvas.glyph_run(res, font_data).font_size(FONT_SIZE).hint(false).fill_glyphs(positioned.iter().copied()).expect("render glyphs");
    combined_canvas.set_transform(Affine::IDENTITY);

    let outline_path = build_outline(&positioned, &outlines, FONT_SIZE);
    let outline_times: Vec<_> = (0..50).map(|_| {
        let start = Instant::now();
        let _ = build_outline(&positioned, &outlines, FONT_SIZE);
        start.elapsed()
    }).collect();
    let median_outline = median_duration(outline_times);
    println!("  outline: {} elements, {median_outline:?} build time (median of 50)", outline_path.elements().len());

    combined_canvas.set_transform(Affine::translate((20.0, target.y_outline)));
    combined_canvas.fill_path(&outline_path);
    combined_canvas.set_transform(Affine::IDENTITY);

    let pw = (width.ceil() as u16) + 40;
    let ph = ((ascent + descent).ceil() as u16) + 40;
    let pix_glyphs = render_to_pixmap(pw, ph, |ctx, r| {
        ctx.glyph_run(r, font_data).font_size(FONT_SIZE).hint(false).fill_glyphs(positioned.iter().copied()).expect("fill glyphs");
    });
    let pix_outline = render_to_pixmap(pw, ph, |ctx, _| {
        ctx.fill_path(&outline_path);
    });

    let diff_pct = pixel_difference_pct(&pix_glyphs, &pix_outline);
    println!("  pixel difference: {diff_pct:.4}%");
    assert!(diff_pct < 1.0, "pixel difference {diff_pct:.4}% exceeds 1% limit");

    let bbox = outline_path.bounding_box();
    println!("  outline bbox: [x: {:.2}..{:.2}, y: {:.2}..{:.2}, height: {:.2}]", bbox.x0, bbox.x1, bbox.y0, bbox.y1, bbox.height());
    assert!(
        bbox.height() >= 0.5 * f64::from(FONT_SIZE) && bbox.height() <= 1.5 * f64::from(FONT_SIZE),
        "outline height {:.2} out of bounds",
        bbox.height()
    );
    assert!(bbox.y0 >= 0.0, "outline y0 lies above line top 0.0");

    outline_path
}

fn main() {
    // Finding the system's fonts is paid once, at startup or on first use.
    let start = Instant::now();
    let mut font_cx = FontContext::new();
    println!("system font collection: {:?}", start.elapsed());
    let mut layout_cx: LayoutContext<[u8; 4]> = LayoutContext::new();
    let mut resources = Resources::new();

    let mut combined_canvas = RenderContext::new(CANVAS_WIDTH, CANVAS_HEIGHT);
    combined_canvas.set_paint(Color::WHITE);
    combined_canvas.fill_rect(&Rect::new(0.0, 0.0, f64::from(CANVAS_WIDTH), f64::from(CANVAS_HEIGHT)));
    combined_canvas.set_paint(Color::BLACK);

    let outline1 = test_font(
        Target {
            label: "sans-serif (Liberation Sans)",
            query: QueryFamily::Generic(GenericFamily::SansSerif),
            family: FontFamily::from(GenericFamily::SansSerif),
            fc_pattern: "sans-serif",
            y_glyph: 10.0,
            y_outline: 80.0,
        },
        &mut font_cx,
        &mut layout_cx,
        &mut combined_canvas,
        &mut resources,
    );

    let outline2 = test_font(
        Target {
            label: "JetBrainsMono Nerd Font",
            query: QueryFamily::Named("JetBrainsMono Nerd Font"),
            family: FontFamily::named("JetBrainsMono Nerd Font"),
            fc_pattern: "JetBrainsMono Nerd Font",
            y_glyph: 170.0,
            y_outline: 250.0,
        },
        &mut font_cx,
        &mut layout_cx,
        &mut combined_canvas,
        &mut resources,
    );

    combined_canvas.flush();
    let mut frame = Pixmap::new(CANVAS_WIDTH, CANVAS_HEIGHT);
    combined_canvas.render(&mut frame, &mut resources);

    if let Some(dir) = std::env::var_os("DUMP") {
        let dir = Path::new(&dir);
        let _ = std::fs::write(dir.join("text.png"), frame.into_png().expect("encode png"));
        let p1 = Affine::translate((20.0, 60.0)) * outline1;
        let p2 = Affine::translate((20.0, 160.0)) * outline2;
        let b = p1.bounding_box().union(p2.bounding_box()).inflate(20.0, 20.0);
        let svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{:.2} {:.2} {:.2} {:.2}\">\n  <path d=\"{}\" fill=\"black\" />\n  <path d=\"{}\" fill=\"black\" />\n</svg>\n",
            b.x0, b.y0, b.width(), b.height(), p1.to_svg(), p2.to_svg());
        let _ = std::fs::write(dir.join("text.svg"), svg);
    }
}
