#!/usr/bin/env rust-script
//! Check that all dependencies use workspace = true
//!
//! ```cargo
//! [dependencies]
//! walkdir = "2"
//! toml = "0.8"
//! ```

use std::fs;
use std::path::Path;

use walkdir::WalkDir;

fn main() {
	let mut issues = Vec::new();

	// Walk through all Cargo.toml files in crates/
	for entry in WalkDir::new("crates")
		.into_iter()
		.filter_map(|e| e.ok())
		.filter(|e| e.file_name() == "Cargo.toml")
	{
		let path = entry.path();
		let content = match fs::read_to_string(path) {
			Ok(c) => c,
			Err(e) => {
				eprintln!("Warning: Failed to read {}: {}", path.display(), e);
				continue;
			}
		};

		let toml: toml::Value = match toml::from_str(&content) {
			Ok(t) => t,
			Err(e) => {
				eprintln!("Warning: Failed to parse {}: {}", path.display(), e);
				continue;
			}
		};

		// Check [dependencies]
		if let Some(deps) = toml.get("dependencies").and_then(|v| v.as_table()) {
			check_dependencies(path, "dependencies", deps, &mut issues);
		}

		// Check [dev-dependencies]
		if let Some(deps) = toml.get("dev-dependencies").and_then(|v| v.as_table()) {
			check_dependencies(path, "dev-dependencies", deps, &mut issues);
		}
	}

	if !issues.is_empty() {
		eprintln!("❌ Found dependencies not using workspace = true:");
		for issue in issues {
			eprintln!("  {}", issue);
		}
		std::process::exit(1);
	}

	println!("✅ All dependencies use workspace = true");
}

fn check_dependencies(
	path: &Path,
	section: &str,
	deps: &toml::map::Map<String, toml::Value>,
	issues: &mut Vec<String>,
) {
	for (name, value) in deps {
		// Skip if it's a table with 'path' (local workspace member)
		if let Some(table) = value.as_table() {
			if table.contains_key("path") {
				continue;
			}

			// Check if workspace = true is present
			if !table.contains_key("workspace") {
				issues.push(format!(
					"{}:[{}] '{}' should use workspace = true",
					path.display(),
					section,
					name
				));
			}
		} else if value.is_str() {
			// Direct version string (e.g., dep = "1.0")
			issues.push(format!(
				"{}:[{}] '{}' uses hardcoded version, should use workspace = true",
				path.display(),
				section,
				name
			));
		}
	}
}
