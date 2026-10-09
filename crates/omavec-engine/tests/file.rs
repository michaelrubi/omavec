//! The `.omavec` folder format.

use std::path::{Path, PathBuf};

use omavec_engine::file::{self, FileError};
use omavec_engine::{Document, NodeId, NodeKind};
use omavec_geom::kurbo::{Affine, Size};

/// An empty folder under `target/` for the test called `name`.
fn folder(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}.omavec"));
    let _ = std::fs::remove_dir_all(&path);
    path
}

/// Two pages: a frame holding a moved, half-transparent, locked rectangle
/// and a hidden ellipse with a hidden fill, and an empty page with an
/// awkward name.
fn sample() -> Document {
    let mut document = Document::default();
    let page = document.pages[0].id;
    let frame = document.create(NodeKind::Frame { clip: true }, Size::new(390.0, 844.0));
    let frame_id = frame.id;
    document.insert(page, 0, frame).unwrap();
    let mut rectangle = document.create(NodeKind::Rectangle, Size::new(100.0, 50.5));
    rectangle.transform = Affine::translate((20.0, 0.1 + 0.2));
    rectangle.opacity = 0.5;
    rectangle.locked = true;
    document.insert(frame_id, 0, rectangle).unwrap();
    let mut ellipse = document.create(NodeKind::Ellipse, Size::new(10.0, 10.0));
    ellipse.visible = false;
    ellipse.name = "Dot \"one\"".into();
    (ellipse.fills[0].opacity, ellipse.fills[0].visible) = (0.25, false);
    document.insert(frame_id, 1, ellipse).unwrap();
    document.add_page("Écrans / Ébauches!");
    document
}

fn files(folder: &Path) -> Vec<String> {
    fn walk(at: &Path, root: &Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(at).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                out.push(path.strip_prefix(root).unwrap().to_string_lossy().into_owned());
            }
        }
    }
    let mut out = Vec::new();
    walk(folder, folder, &mut out);
    out.sort();
    out
}

#[test]
fn a_document_comes_back_as_it_was_saved() {
    let folder = folder("round-trip");
    let document = sample();
    file::save(&document, &folder).unwrap();
    assert_eq!(files(&folder), ["document.json", "pages/01-page-1.json", "pages/02-écrans-ébauches.json"]);
    let mut opened = file::open(&folder).unwrap();
    assert_eq!(opened, document);
    // New nodes carry on from where the ids stopped.
    let next = opened.create(NodeKind::Group, Size::ZERO).id;
    assert_eq!(next, sample().create(NodeKind::Group, Size::ZERO).id);
}

#[test]
fn the_files_are_readable_and_leave_out_what_is_usual() {
    let folder = folder("text");
    file::save(&sample(), &folder).unwrap();
    assert_eq!(
        std::fs::read_to_string(folder.join("document.json")).unwrap(),
        r#"{
  "format": 1,
  "next_id": 6,
  "pages": [
    "01-page-1.json",
    "02-écrans-ébauches.json"
  ]
}
"#
    );
    assert_eq!(
        std::fs::read_to_string(folder.join("pages/01-page-1.json")).unwrap(),
        r##"{
  "id": 1,
  "type": "page",
  "name": "Page 1",
  "size": {
    "width": 0.0,
    "height": 0.0
  },
  "children": [
    {
      "id": 2,
      "type": "frame",
      "clip": true,
      "name": "Frame",
      "size": {
        "width": 390.0,
        "height": 844.0
      },
      "fills": [
        {
          "type": "solid",
          "color": "#ffffff"
        }
      ],
      "children": [
        {
          "id": 3,
          "type": "rectangle",
          "name": "Rectangle",
          "locked": true,
          "opacity": 0.5,
          "transform": [
            1.0,
            0.0,
            0.0,
            1.0,
            20.0,
            0.30000000000000004
          ],
          "size": {
            "width": 100.0,
            "height": 50.5
          },
          "fills": [
            {
              "type": "solid",
              "color": "#d9d9d9"
            }
          ]
        },
        {
          "id": 4,
          "type": "ellipse",
          "name": "Dot \"one\"",
          "visible": false,
          "size": {
            "width": 10.0,
            "height": 10.0
          },
          "fills": [
            {
              "type": "solid",
              "color": "#d9d9d9",
              "opacity": 0.25,
              "visible": false
            }
          ]
        }
      ]
    }
  ]
}
"##
    );
}

#[test]
fn saving_again_touches_nothing_that_did_not_change() {
    let folder = folder("unchanged");
    let mut document = sample();
    file::save(&document, &folder).unwrap();
    let stamps = |folder: &Path| -> Vec<_> { files(folder).iter().map(|name| std::fs::metadata(folder.join(name)).unwrap().modified().unwrap()).collect() };
    let before = stamps(&folder);
    std::thread::sleep(std::time::Duration::from_millis(20));

    // The same document, and the document read back from disk: nothing is rewritten.
    file::save(&document, &folder).unwrap();
    file::save(&file::open(&folder).unwrap(), &folder).unwrap();
    assert_eq!(stamps(&folder), before);

    // A change on the first page rewrites that page and nothing else.
    let frame = document.pages[0].children[0].id;
    document.node_mut(frame).unwrap().name = "Phone".into();
    file::save(&document, &folder).unwrap();
    let after = stamps(&folder);
    assert_eq!(files(&folder), ["document.json", "pages/01-page-1.json", "pages/02-écrans-ébauches.json"]);
    assert_eq!(after[0], before[0]);
    assert_ne!(after[1], before[1]);
    assert_eq!(after[2], before[2]);
}

#[test]
fn old_page_files_go_and_everything_else_in_the_folder_stays() {
    let folder = folder("stale");
    let mut document = sample();
    file::save(&document, &folder).unwrap();
    // What else lives in a document's folder, or gets left there.
    std::fs::create_dir_all(folder.join("assets")).unwrap();
    std::fs::write(folder.join("assets/3f9a.png"), b"png").unwrap();
    std::fs::write(folder.join("thumbnail.png"), b"png").unwrap();
    std::fs::write(folder.join("pages/notes.txt"), b"mine").unwrap();

    let second = document.pages[1].id;
    document.node_mut(second).unwrap().name = "Screens".into();
    file::save(&document, &folder).unwrap();
    assert_eq!(files(&folder), ["assets/3f9a.png", "document.json", "pages/01-page-1.json", "pages/02-screens.json", "pages/notes.txt", "thumbnail.png"]);

    document.remove(second).unwrap();
    file::save(&document, &folder).unwrap();
    assert_eq!(files(&folder), ["assets/3f9a.png", "document.json", "pages/01-page-1.json", "pages/notes.txt", "thumbnail.png"]);
    assert_eq!(file::open(&folder).unwrap(), document);
}

/// Saves the sample, lets `damage` at the folder, and returns what opening
/// it says.
fn open_after(name: &str, damage: impl FnOnce(&Path)) -> String {
    let folder = folder(name);
    file::save(&sample(), &folder).unwrap();
    damage(&folder);
    let error = file::open(&folder).unwrap_err();
    error.to_string().replace(&folder.to_string_lossy().into_owned(), "…")
}

fn rewrite(path: &Path, from: &str, to: &str) {
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains(from), "{from} not in {}", path.display());
    std::fs::write(path, text.replacen(from, to, 1)).unwrap();
}

#[test]
fn a_folder_that_is_not_a_document_says_why() {
    let nowhere = folder("nowhere");
    assert!(matches!(file::open(&nowhere), Err(FileError::Io { .. })));

    assert_eq!(
        open_after("newer", |folder| rewrite(&folder.join("document.json"), "\"format\": 1", "\"format\": 2")),
        "…/document.json was saved by a newer Omavec (format 2; this one reads up to 1)"
    );
    // A newer format is refused even if its manifest has changed shape.
    assert_eq!(
        open_after("newer-shape", |folder| std::fs::write(folder.join("document.json"), r#"{"format": 9, "pages": {"a": 1}}"#).unwrap()),
        "…/document.json was saved by a newer Omavec (format 9; this one reads up to 1)"
    );
    assert!(open_after("truncated", |folder| {
        let path = folder.join("pages/01-page-1.json");
        let text = std::fs::read(&path).unwrap();
        std::fs::write(&path, &text[..text.len() / 2]).unwrap();
    })
    .starts_with("…/pages/01-page-1.json: EOF while parsing"));
    assert!(open_after("missing-page", |folder| std::fs::remove_file(folder.join("pages/01-page-1.json")).unwrap()).starts_with("…/pages/01-page-1.json: "));
    assert_eq!(
        open_after("outside", |folder| rewrite(&folder.join("document.json"), "01-page-1.json", "../../secrets.json")),
        "…/document.json: \"../../secrets.json\" is not a page file name"
    );
    assert_eq!(
        open_after("not-a-page", |folder| rewrite(&folder.join("pages/01-page-1.json"), "\"type\": \"page\"", "\"type\": \"group\"")),
        "…/pages/01-page-1.json: the top node is not a page"
    );
    assert_eq!(
        open_after("no-pages", |folder| std::fs::write(folder.join("document.json"), r#"{"format": 1, "next_id": 1, "pages": []}"#).unwrap()),
        "…/document.json: the document has no pages"
    );
    // What a careless merge in git leaves: one id on two nodes.
    assert_eq!(
        open_after("merged", |folder| rewrite(&folder.join("pages/01-page-1.json"), "\"id\": 4", "\"id\": 3")),
        "…/pages/01-page-1.json: id 3 is used twice in the document"
    );
    assert_eq!(
        open_after("page-in-page", |folder| rewrite(&folder.join("pages/01-page-1.json"), "\"type\": \"ellipse\"", "\"type\": \"page\"")),
        "…/pages/01-page-1.json: node 4 is a page inside node 2"
    );
}

#[test]
fn ids_in_the_pages_win_over_a_stale_next_id() {
    // Two branches each added a node; the merge kept one branch's counter.
    let folder = folder("next-id");
    file::save(&sample(), &folder).unwrap();
    rewrite(&folder.join("document.json"), "\"next_id\": 6", "\"next_id\": 2");
    let mut opened = file::open(&folder).unwrap();
    assert_eq!(opened.create(NodeKind::Group, Size::ZERO).id, NodeId(6));
}

#[test]
fn a_zipped_document_is_the_same_files_and_opens_the_same() {
    let folder = folder("zipped-source");
    let zipped = folder.with_file_name("zipped.omavecz");
    let _ = std::fs::remove_file(&zipped);
    let document = sample();
    file::save(&document, &folder).unwrap();
    file::save(&document, &zipped).unwrap();
    assert!(zipped.is_file());
    assert_eq!(file::open(&zipped).unwrap(), document);

    // Every file of the folder, byte for byte, and nothing else.
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&zipped).unwrap()).unwrap();
    let mut names: Vec<String> = archive.file_names().map(str::to_owned).collect();
    names.sort();
    assert_eq!(names, files(&folder));
    for name in &names {
        let mut inside = Vec::new();
        std::io::Read::read_to_end(&mut archive.by_name(name).unwrap(), &mut inside).unwrap();
        assert_eq!(inside, std::fs::read(folder.join(name)).unwrap(), "{name}");
    }

    // The same document zips to the same bytes, whenever it is saved.
    let first = std::fs::read(&zipped).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let again = folder.with_file_name("zipped-again.omavecz");
    file::save(&file::open(&zipped).unwrap(), &again).unwrap();
    assert_eq!(std::fs::read(&again).unwrap(), first);
}

#[test]
fn a_file_that_is_not_a_zipped_document_says_so() {
    let path = folder("not-a-zip").with_file_name("not-a-zip.omavecz");
    std::fs::write(&path, b"this is not a zip").unwrap();
    assert!(matches!(file::open(&path), Err(FileError::Zip { .. })));
    // A zip with something else in it.
    let other = path.with_file_name("other.omavecz");
    let mut archive = zip::ZipWriter::new(std::fs::File::create(&other).unwrap());
    archive.start_file("hello.txt", zip::write::SimpleFileOptions::default()).unwrap();
    std::io::Write::write_all(&mut archive, b"hello").unwrap();
    archive.finish().unwrap();
    let error = file::open(&other).unwrap_err().to_string();
    assert!(error.contains("other.omavecz/document.json"), "{error}");
}
