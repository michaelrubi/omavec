//! `omavec export`: frames to SVG and PNG files, with no window and no GPU.
//!
//! ```text
//! omavec export logo.omavec --frame Logo --format svg,png@2x --out dist/
//! ```
//!
//! With no `--frame` every top-level frame is exported; with no `--format`,
//! each frame as its own export settings say, or as PNG at 1× if it has
//! none; with no `--out`, into the current folder.

use std::path::{Path, PathBuf};

use omavec_engine::{Export, Node, NodeKind, file};

/// Runs `omavec export` with `arguments` (what follows the word `export`)
/// and returns the files it wrote.
pub fn export(arguments: &[String]) -> Result<Vec<PathBuf>, String> {
    let (mut folder, mut names, mut formats, mut out) = (None, Vec::new(), Vec::new(), PathBuf::from("."));
    let mut arguments = arguments.iter();
    while let Some(argument) = arguments.next() {
        // The next option is not this one's value.
        let mut value = || arguments.next().filter(|value| !value.starts_with("--")).ok_or(format!("{argument} needs a value"));
        match argument.as_str() {
            "--frame" => names.push(value()?.clone()),
            "--format" => {
                for format in value()?.split(',') {
                    formats.push(Export::parse(format)?);
                }
            }
            "--out" => out = PathBuf::from(value()?),
            option if option.starts_with("--") => return Err(format!("{option} is not an option of export")),
            path if folder.is_none() => folder = Some(Path::new(path)),
            extra => return Err(format!("one document at a time: what is {extra}?")),
        }
    }
    let folder = folder.ok_or("usage: omavec export file.omavec [--frame NAME]… [--format svg,png@2x] [--out DIR]")?;
    let document = file::open(folder).map_err(|error| error.to_string())?;
    let all: Vec<&Node> = document.pages.iter().flat_map(|page| &page.children).map(|node| &**node).filter(|node| matches!(node.kind, NodeKind::Frame { .. })).collect();
    let frames = if names.is_empty() {
        all.clone()
    } else {
        let find = |name: &String| all.iter().copied().find(|frame| frame.name == *name).ok_or_else(|| {
            let known: Vec<&str> = all.iter().map(|frame| frame.name.as_str()).collect();
            format!("no frame called \"{name}\"; there is: {}", known.join(", "))
        });
        names.iter().map(find).collect::<Result<_, _>>()?
    };
    if frames.is_empty() {
        return Err("there are no frames to export".into());
    }

    let ids: Vec<_> = frames.iter().map(|frame| frame.id).collect();
    crate::export::write(&document, &ids, &formats, &out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use omavec_engine::Document;
    use omavec_geom::kurbo::{Affine, Size};

    /// A folder for the test called `name`, holding `doc.omavec` with two
    /// frames, "Logo" (120 × 80, at (500, 300), holding an ellipse) and
    /// "Icon / Small" (16 × 16), and a loose rectangle beside them.
    fn saved(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("omavec-cli-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let mut document = Document::default();
        let page = document.pages[0].id;
        let mut logo = document.create(NodeKind::Frame { clip: true }, Size::new(120.0, 80.0));
        (logo.name, logo.transform) = ("Logo".into(), Affine::translate((500.0, 300.0)));
        let logo_id = logo.id;
        document.insert(page, 0, logo).unwrap();
        let ellipse = document.create(NodeKind::Ellipse, Size::new(40.0, 40.0));
        document.insert(logo_id, 0, ellipse).unwrap();
        let mut icon = document.create(NodeKind::Frame { clip: true }, Size::new(16.0, 16.0));
        icon.name = "Icon / Small".into();
        document.insert(page, 1, icon).unwrap();
        let loose = document.create(NodeKind::Rectangle, Size::new(10.0, 10.0));
        document.insert(page, 2, loose).unwrap();
        file::save(&document, &folder.join("doc.omavec")).unwrap();
        folder
    }

    fn run(folder: &Path, arguments: &[&str]) -> Result<Vec<String>, String> {
        let document = folder.join("doc.omavec").to_string_lossy().into_owned();
        let out = folder.join("out").to_string_lossy().into_owned();
        let arguments: Vec<String> = [document.as_str()].iter().chain(arguments).chain(&["--out", out.as_str()]).map(|argument| argument.to_string()).collect();
        let written = export(&arguments)?;
        Ok(written.iter().map(|path| path.strip_prefix(folder.join("out")).unwrap().to_string_lossy().into_owned()).collect())
    }

    /// A PNG's width and height, from its header.
    fn png_size(path: &Path) -> (u32, u32) {
        let bytes = std::fs::read(path).unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        let number = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap());
        (number(16), number(20))
    }

    #[test]
    fn one_frame_in_two_formats() {
        let folder = saved("one");
        assert_eq!(run(&folder, &["--frame", "Logo", "--format", "svg,png@2x"]).unwrap(), ["Logo.svg", "Logo@2x.png"]);
        let svg = std::fs::read_to_string(folder.join("out/Logo.svg")).unwrap();
        // The frame at its own origin, whatever its place on the page.
        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"120\" height=\"80\""), "{svg}");
        assert!(svg.contains("<ellipse cx=\"20\" cy=\"20\" rx=\"20\" ry=\"20\" fill=\"#d9d9d9\"/>"), "{svg}");
        assert_eq!(png_size(&folder.join("out/Logo@2x.png")), (240, 160));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn every_frame_as_png_when_nothing_is_asked_for() {
        let folder = saved("all");
        // The loose rectangle isn't a frame; a slash in a name isn't a folder.
        assert_eq!(run(&folder, &[]).unwrap(), ["Logo.png", "Icon - Small.png"]);
        assert_eq!(png_size(&folder.join("out/Logo.png")), (120, 80));
        assert_eq!(png_size(&folder.join("out/Icon - Small.png")), (16, 16));
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_frame_is_exported_as_its_own_settings_say_unless_told_otherwise() {
        let folder = saved("own");
        let path = folder.join("doc.omavec");
        let mut document = file::open(&path).unwrap();
        let logo = document.pages[0].children[0].id;
        document.node_mut(logo).unwrap().exports = vec![Export::Svg, Export::Png { scale: 3.0 }];
        file::save(&document, &path).unwrap();
        // The settings survive the file, and say what comes out.
        assert_eq!(file::open(&path).unwrap(), document);
        assert_eq!(run(&folder, &[]).unwrap(), ["Logo.svg", "Logo@3x.png", "Icon - Small.png"]);
        assert_eq!(run(&folder, &["--format", "png"]).unwrap(), ["Logo.png", "Icon - Small.png"]);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn what_cannot_be_exported_says_why_and_writes_nothing() {
        let folder = saved("errors");
        assert_eq!(run(&folder, &["--frame", "Nope"]).unwrap_err(), "no frame called \"Nope\"; there is: Logo, Icon / Small");
        assert_eq!(run(&folder, &["--format", "jpeg"]).unwrap_err(), "\"jpeg\" is not a format: use svg, png or png@2x");
        assert_eq!(run(&folder, &["--format", "png@0x"]).unwrap_err(), "\"png@0x\" is not a size like png@2x");
        assert_eq!(run(&folder, &["--format", "png@9999x"]).unwrap_err(), "Logo at 9999x is 1199880 pixels: too big, or nothing");
        assert_eq!(run(&folder, &["--colour"]).unwrap_err(), "--colour is not an option of export");
        assert_eq!(run(&folder, &["--frame"]).unwrap_err(), "--frame needs a value");
        assert!(export(&[]).unwrap_err().starts_with("usage: omavec export"));
        assert!(export(&["/nowhere/x.omavec".into()]).unwrap_err().contains("No such file"));
        assert!(!folder.join("out").exists() || std::fs::read_dir(folder.join("out")).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(folder);
    }
}
