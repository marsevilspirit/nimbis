#!/usr/bin/env rust-script
//! ```cargo
//! [dependencies]
//! walkdir = "2"
//! regex = "1"
//! ```

use std::fs;

use regex::Regex;
use walkdir::DirEntry;
use walkdir::WalkDir;

fn main() {
	let root = ".";
	let walker = WalkDir::new(root).into_iter();
	let mut found_issues = false;

	// Pattern:
	// ^\s*\}       -> Line starting with '}' (possibly indented)
	// \n           -> Immediately followed by a newline
	// \s*          -> Optional whitespace on the next line
	// (?:#\[.*\]\n\s*)* -> Optional attributes (lines starting with #[...])
	// impl         -> The word 'impl'
	//
	// This detects cases where an 'impl' block (possibly with attributes) follows a
	// closing brace '}' without a blank line in between.
	let re = Regex::new(r"(?m)^[ \t]*\}\n[ \t]*(?:#\[.*\]\n[ \t]*)*impl").unwrap();

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

			for mat in re.find_iter(&content) {
				found_issues = true;
				let start = mat.start();
				// Calculate line number (1-based)
				// lines().count() gives the number of lines. Adding 1 because the match is on
				// the line of '}'
				let line_num = content[..start].lines().count() + 1;

				issues.push(format!(
                    "{}:{}:{} - Add a blank line between the closing brace '}}' and the next 'impl' block.",
                    entry.path().display(),
                    line_num,
                    // We can't really get column easily and regex match spans multiple lines
                    // so just line num is fine
                    "" 
                ).replace(": -", " -")); // Fix formatted string if empty string
				                         // arg looks weird
			}
		}
	}

	if found_issues {
		eprintln!("❌ Found formatting issues:");
		for issue in issues {
			eprintln!("  {}", issue);
		}
		std::process::exit(1);
	} else {
		println!("✅ All code files are properly formatted (impl blocks separated)");
	}
}

fn is_ignored(entry: &DirEntry) -> bool {
	entry
		.file_name()
		.to_str()
		.map(|s| s.starts_with(".") && s != "." && s != "./" || s == "target")
		.unwrap_or(false)
}
