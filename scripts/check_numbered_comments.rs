#!/usr/bin/env rust-script
//! Check that code does not contain numbered step comments like `// 1.`, `//
//! 2.` etc. These are typically leftover development notes that should be
//! converted to proper comments.
//!
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

			check_numbered_comments(&content, entry.path(), &mut issues);
		}
	}

	if !issues.is_empty() {
		eprintln!("❌ Found numbered step comments (// N.) that should be removed or rewritten:");
		for issue in issues {
			eprintln!("  {}", issue);
		}
		std::process::exit(1);
	} else {
		println!("✅ No numbered step comments found");
	}
}

fn check_numbered_comments(content: &str, path: &Path, issues: &mut Vec<String>) {
	// Match comments like `// 1.`, `// 2.`, `// 10.` etc.
	// The pattern requires: optional whitespace, //, optional whitespace, digit(s),
	// dot, space or end
	let re = Regex::new(r"//\s*\d+\.\s").unwrap();

	for (i, line) in content.lines().enumerate() {
		let trimmed = line.trim();

		// Only check lines that are pure comments (start with //)
		if !trimmed.starts_with("//") {
			continue;
		}

		if re.is_match(trimmed) {
			issues.push(format!("{}:{} - {}", path.display(), i + 1, trimmed));
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
