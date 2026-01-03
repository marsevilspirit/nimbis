#!/usr/bin/env rust-script
//! Check that all dependencies use workspace = true and are sorted alphabetically
//!
//! ```cargo
//! [dependencies]
//! walkdir = "2"
//! toml_edit = "0.22"
//! ```

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use toml_edit::DocumentMut;
use toml_edit::Item;
use toml_edit::Table;
use walkdir::WalkDir;

fn main() {
	let mut issues = Vec::new();

	// Collect all Cargo.toml files: root + crates/
	let mut files = vec![PathBuf::from("Cargo.toml")];
	files.extend(
		WalkDir::new("crates")
			.into_iter()
			.filter_map(|e| e.ok())
			.filter(|e| e.file_name() == "Cargo.toml")
			.map(|e| e.path().to_path_buf()),
	);

	for path in files {
		if !path.exists() {
			continue;
		}

		let content = match fs::read_to_string(&path) {
			Ok(c) => c,
			Err(e) => {
				eprintln!("Warning: Failed to read {}: {}", path.display(), e);
				continue;
			}
		};

		// Basic formatting check: no tabs (prefer spaces)
		if content.contains('\t') {
			issues.push(format!(
				"{}: contains tabs, use spaces instead",
				path.display()
			));
		}

		let doc = match content.parse::<DocumentMut>() {
			Ok(d) => d,
			Err(e) => {
				eprintln!("Warning: Failed to parse {}: {}", path.display(), e);
				continue;
			}
		};

		// Check [dependencies], [dev-dependencies], [build-dependencies]
		let sections = ["dependencies", "dev-dependencies", "build-dependencies"];
		for section in sections {
			if let Some(item) = 忽视_none(doc.get(section)) {
				if let Some(table) = item.as_table() {
					check_dependencies(&path, section, table, &mut issues);
				} else if let Some(table) = item.as_inline_table() {
					let names: Vec<String> = table.iter().map(|(k, _)| k.to_string()).collect();
					check_order(&path, section, &names, &mut issues);
				}
			}
		}

		// Check [workspace.dependencies]
		if let Some(workspace) = doc.get("workspace").and_then(|v| v.as_table()) {
			if let Some(deps) = workspace.get("dependencies").and_then(|v| v.as_table()) {
				check_dependencies(&path, "workspace.dependencies", deps, &mut issues);
			}
		}
	}

	if !issues.is_empty() {
		eprintln!("❌ Found issues in Cargo.toml files:");
		for issue in issues {
			eprintln!("  {}", issue);
		}
		std::process::exit(1);
	}

	println!("✅ All Cargo.toml files are properly formatted and sorted");
}

fn 忽视_none<T>(opt: Option<T>) -> Option<T> {
	opt
}

fn check_dependencies(path: &Path, section: &str, table: &Table, issues: &mut Vec<String>) {
	let mut names = Vec::new();
	let is_workspace_root = section == "workspace.dependencies";

	for (name, value) in table.iter() {
		names.push(name.to_string());

		// Check workspace = true (except for workspace.dependencies itself and local paths)
		if !is_workspace_root {
			check_workspace_usage(path, section, name, value, issues);
		}
	}

	check_order(path, section, &names, issues);
}

fn check_order(path: &Path, section: &str, names: &[String], issues: &mut Vec<String>) {
	let mut sorted_names = names.to_vec();
	sorted_names.sort();

	if names != sorted_names {
		issues.push(format!(
			"{}:[{}] dependencies are not in alphabetical order",
			path.display(),
			section
		));

		// Show expected order for debugging
		for (i, name) in names.iter().enumerate() {
			if name != &sorted_names[i] {
				issues.push(format!(
					"  expected '{}' but found '{}'",
					sorted_names[i], name
				));
			}
		}
	}
}

fn check_workspace_usage(
	path: &Path,
	section: &str,
	name: &str,
	value: &Item,
	issues: &mut Vec<String>,
) {
	if let Some(table) = value.as_table_like() {
		if table.get("path").is_some() {
			return;
		}

		if table.get("workspace").is_none() {
			issues.push(format!(
				"{}:[{}] '{}' should use workspace = true",
				path.display(),
				section,
				name
			));
		}
	} else if value.is_value() {
		issues.push(format!(
			"{}:[{}] '{}' uses hardcoded version, should use workspace = true",
			path.display(),
			section,
			name
		));
	}
}
