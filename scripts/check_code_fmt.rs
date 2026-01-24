#!/usr/bin/env rust-script
//! ```cargo
//! [dependencies]
//! walkdir = "2"
//! regex = "1"
//! ```

use std::fs;
use std::path::Path;

use regex::Regex;
use walkdir::DirEntry;
use walkdir::WalkDir;

fn main() {
	let root = ".";
	let walker = WalkDir::new(root).into_iter();
	let mut issues = Vec::new();

	for entry in walker.filter_entry(|e| !is_ignored(e)) {
		let entry = entry.unwrap();
		if !entry.file_type().is_file() {
			continue;
		}
		if entry.path().extension().map_or(false, |ext| ext == "rs") {
			let content = match fs::read_to_string(entry.path()) {
				Ok(c) => c,
				Err(_) => continue,
			};

			check_impl_spacing(&content, entry.path(), &mut issues);
			check_indented_use(&content, entry.path(), &mut issues);
		}
	}

	if !issues.is_empty() {
		eprintln!("❌ Found formatting issues:");
		for issue in issues {
			eprintln!("  {}", issue);
		}
		std::process::exit(1);
	} else {
		println!("✅ All code files are properly formatted (impl blocks separated, top-level use)");
	}
}

fn check_impl_spacing(content: &str, path: &Path, issues: &mut Vec<String>) {
	// Pattern 1: Impl spacing
	let re_impl = Regex::new(r"(?m)^[ \t]*\}\n[ \t]*(?:#\[.*\]\n[ \t]*)*impl").unwrap();

	for mat in re_impl.find_iter(content) {
		let start = mat.start();
		let line_num = content[..start].lines().count() + 1;
		issues.push(format!(
			"{}:{} - Add a blank line between the closing brace '}}' and the next 'impl' block.",
			path.display(),
			line_num
		));
	}
}

fn check_indented_use(content: &str, path: &Path, issues: &mut Vec<String>) {
	// Pattern 2: Indented use statement (use at start of line is fine)
	let re_use = Regex::new(r"(?m)^[ \t]+use\s").unwrap();

	let mut in_tests_mod = false;
	let mut tests_mod_brace_depth = 0;
	let mut brace_depth = 0;

	for (i, line) in content.lines().enumerate() {
		let trim_line = line.trim();
		// Basic comment stripping (simplified)
		let code_part = trim_line.split("//").next().unwrap_or("");

		// Check for mod tests
		// Common patterns: "mod tests", "#[cfg(test)] mod tests", "mod tests {"
		if !in_tests_mod && (code_part.contains("mod tests") || code_part.contains("cfg(test)")) {
			// Heuristic: If it looks like a test module, start ignoring
			// This simplistic check assumes consistent formatting
			in_tests_mod = true;
			tests_mod_brace_depth = brace_depth;
		}

		// Check for indented use
		if !in_tests_mod && re_use.is_match(line) {
			issues.push(format!(
				"{}:{} - 'use' statement should be at the top level (found indented use).",
				path.display(),
				i + 1
			));
		}

		// Update brace depth
		// Note: this is very basic and could be fooled by strings/chars '{}'
		// But for formatted code it's usually fine
		let open_braces = code_part.matches('{').count();
		let close_braces = code_part.matches('}').count();

		brace_depth += open_braces;
		if brace_depth >= close_braces {
			brace_depth -= close_braces;
		} else {
			brace_depth = 0; // Should not happen in valid code
		}

		if in_tests_mod && brace_depth <= tests_mod_brace_depth && close_braces > 0 {
			// We might have closed the tests module
			in_tests_mod = false;
		}
	}
}

fn is_ignored(entry: &DirEntry) -> bool {
	entry
		.file_name()
		.to_str()
		.map(|s| s.starts_with(".") && s != "." && s != "./" || s == "target")
		.unwrap_or(false)
}
