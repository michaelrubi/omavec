use std::fs;
use std::path::Path;

#[test]
fn test_fixture_trees() {
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut entries: Vec<_> = fs::read_dir(&fixtures_dir)
        .expect("read fixtures dir")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "fig"))
        .collect();
    entries.sort_by_key(|e| e.path());

    assert!(!entries.is_empty(), "expected fixture .fig files");

    for entry in entries {
        let fig_path = entry.path();
        let tree_path = fig_path.with_extension("tree.txt");
        let expected_tree = fs::read_to_string(&tree_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", tree_path.display()));

        let file_bytes = fs::read(&fig_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", fig_path.display()));
        let message = omavec_fig::decode(&file_bytes)
            .unwrap_or_else(|e| panic!("failed to decode {}: {e}", fig_path.display()));
        let actual_tree = omavec_fig::tree(&message)
            .unwrap_or_else(|e| panic!("failed to build tree for {}: {e}", fig_path.display()));

        assert_eq!(
            actual_tree,
            expected_tree,
            "tree mismatch for {}",
            fig_path.display()
        );
    }
}

#[test]
fn test_decode_errors_on_malformed_input() {
    // Empty slice
    assert!(omavec_fig::decode(&[]).is_err());

    // 100 bytes of zeros
    assert!(omavec_fig::decode(&[0u8; 100]).is_err());

    // structure.fig truncated to half its length
    let fixtures_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let structure_path = fixtures_dir.join("structure.fig");
    let bytes = fs::read(&structure_path).expect("read structure.fig");
    let truncated = &bytes[..bytes.len() / 2];
    assert!(omavec_fig::decode(truncated).is_err());
}
