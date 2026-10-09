use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let json_mode = args.iter().any(|arg| arg == "--json");
    let file_path = args.iter().find(|arg| *arg != "--json");

    let Some(path) = file_path else {
        eprintln!("Usage: fig_dump [--json] <file.fig>");
        process::exit(1);
    };

    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Failed to read {path}: {e}");
            process::exit(1);
        }
    };

    let message = match omavec_fig::decode(&bytes) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Failed to decode {path}: {e}");
            process::exit(1);
        }
    };

    if json_mode {
        match serde_json::to_string_pretty(&message) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("Failed to serialize JSON: {e}");
                process::exit(1);
            }
        }
        return;
    }

    let tree = match omavec_fig::tree(&message) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to build node tree: {e}");
            process::exit(1);
        }
    };

    print!("{tree}");

    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    if let Some(nodes) = message.get("nodeChanges").and_then(|v| v.as_array()) {
        for node in nodes {
            if let Some(node_type) = node.get("type").and_then(|v| v.as_str()) {
                *counts.entry(node_type).or_default() += 1;
            }
        }
    }

    println!("\nNode counts per type:");
    for (node_type, count) in &counts {
        println!("  {node_type}: {count}");
    }

    let schema_defs = message
        .get("schema_definitions")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let version = message
        .get("version")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    println!("\nSchema definitions: {schema_defs}");
    println!("File version: {version}");
}
