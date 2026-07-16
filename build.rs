use std::collections::HashSet;
use std::fs;
use std::path::Path;

fn main() {
    let filter_dirs = [
        Path::new("assets/filters/rtk"),
        Path::new("assets/filters/toche"),
    ];
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR must be set by Cargo");
    let dest = Path::new(&out_dir).join("builtin_filters.toml");

    println!("cargo:rerun-if-changed=assets/filters/rtk");
    println!("cargo:rerun-if-changed=assets/filters/toche");

    let mut files = Vec::new();
    for filters_dir in filter_dirs {
        files.extend(
            fs::read_dir(filters_dir)
                .unwrap_or_else(|e| panic!("{} must exist: {e}", filters_dir.display()))
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "toml")),
        );
    }

    files.sort_by_key(|e| e.file_name());

    let mut combined = String::from("schema_version = 1\n\n");

    for entry in &files {
        let content = fs::read_to_string(entry.path())
            .unwrap_or_else(|e| panic!("Failed to read {:?}: {}", entry.path(), e));
        combined.push_str(&format!(
            "# --- {} ---\n",
            entry.file_name().to_string_lossy()
        ));
        combined.push_str(&content);
        combined.push_str("\n\n");
    }

    // Validate: parse the combined TOML to catch errors at build time
    let parsed: toml::Value = combined.parse().unwrap_or_else(|e| {
        panic!(
            "TOML validation failed for combined filters:\n{}\n\nCheck assets/filters/**/*.toml files",
            e
        )
    });

    // Detect duplicate filter names across files
    if let Some(filters) = parsed.get("filters").and_then(toml::Value::as_table) {
        let mut seen: HashSet<String> = HashSet::new();
        for key in filters.keys() {
            if !seen.insert(key.to_string()) {
                panic!(
                    "Duplicate filter name '{}' found across assets/filters/**/*.toml files",
                    key
                );
            }
        }
    }

    fs::write(&dest, combined).expect("Failed to write combined builtin_filters.toml");
}
