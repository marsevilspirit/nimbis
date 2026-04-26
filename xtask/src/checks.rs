use std::fs;
use std::path::Path;

use regex::Regex;
use toml_edit::DocumentMut;
use toml_edit::Item;
use toml_edit::Table;
use walkdir::DirEntry;
use walkdir::WalkDir;

use crate::write_stderr_line;
use crate::write_stdout_line;

pub fn check_workspace(root: impl AsRef<Path>) -> Result<(), String> {
	let issues = workspace_issues(root.as_ref());
	if !issues.is_empty() {
		return Err(format_issues(
			"❌ Found issues in Cargo.toml files:",
			&issues,
		));
	}

	write_stdout_line("✅ All Cargo.toml files are properly formatted and sorted")?;
	Ok(())
}

pub fn check_code_fmt(root: impl AsRef<Path>) -> Result<(), String> {
	let issues = code_fmt_issues(root.as_ref());
	if !issues.is_empty() {
		return Err(format_issues("❌ Found formatting issues:", &issues));
	}

	write_stdout_line(
		"✅ All code files are properly formatted (impl blocks separated, top-level use)",
	)?;
	Ok(())
}

pub fn check_numbered_comments(root: impl AsRef<Path>) -> Result<(), String> {
	let issues = numbered_comment_issues(root.as_ref());
	if !issues.is_empty() {
		return Err(format_issues(
			"❌ Found numbered step comments (// N.) that should be removed or rewritten:",
			&issues,
		));
	}

	write_stdout_line("✅ No numbered step comments found")?;
	Ok(())
}

pub fn workspace_issues(root: &Path) -> Vec<String> {
	let mut issues = Vec::new();
	let files = cargo_toml_files(root);

	for path in files {
		if !path.exists() {
			continue;
		}

		let content = match fs::read_to_string(&path) {
			Ok(c) => c,
			Err(e) => {
				write_stderr_line(&format!(
					"Warning: Failed to read {}: {}",
					path.display(),
					e
				));
				continue;
			}
		};

		if content.contains('\t') {
			issues.push(format!(
				"{}: contains tabs, use spaces instead",
				path.display()
			));
		}

		let doc = match content.parse::<DocumentMut>() {
			Ok(d) => d,
			Err(e) => {
				write_stderr_line(&format!(
					"Warning: Failed to parse {}: {}",
					path.display(),
					e
				));
				continue;
			}
		};

		let sections = ["dependencies", "dev-dependencies", "build-dependencies"];
		for section in sections {
			if let Some(item) = doc.get(section) {
				if let Some(table) = item.as_table() {
					check_dependencies(&path, section, table, &content, &mut issues);
				} else if let Some(table) = item.as_inline_table() {
					let names: Vec<String> = table.iter().map(|(k, _)| k.to_string()).collect();
					check_order(&path, section, &names, &mut issues);
				}
			}
		}

		if let Some(deps) = doc
			.get("workspace")
			.and_then(|v| v.as_table())
			.and_then(|workspace| workspace.get("dependencies"))
			.and_then(|deps| deps.as_table())
		{
			check_dependencies(&path, "workspace.dependencies", deps, &content, &mut issues);
		}
	}

	issues
}

fn cargo_toml_files(root: &Path) -> Vec<std::path::PathBuf> {
	WalkDir::new(root)
		.into_iter()
		.filter_entry(|e| !is_ignored(e))
		.filter_map(|e| e.ok())
		.filter(|e| e.file_type().is_file())
		.filter(|e| e.file_name() == "Cargo.toml")
		.map(|e| e.path().to_path_buf())
		.collect()
}

pub fn code_fmt_issues(root: &Path) -> Vec<String> {
	let mut issues = Vec::new();
	for entry in WalkDir::new(root)
		.into_iter()
		.filter_entry(|e| !is_ignored(e))
	{
		let entry = match entry {
			Ok(entry) => entry,
			Err(_) => continue,
		};
		if !entry.file_type().is_file() {
			continue;
		}
		if entry.path().extension().is_some_and(|ext| ext == "rs") {
			let content = match fs::read_to_string(entry.path()) {
				Ok(c) => c,
				Err(_) => continue,
			};

			check_impl_spacing(&content, entry.path(), &mut issues);
			check_indented_use(&content, entry.path(), &mut issues);
		}
	}
	issues
}

pub fn numbered_comment_issues(root: &Path) -> Vec<String> {
	let mut issues = Vec::new();
	for entry in WalkDir::new(root)
		.into_iter()
		.filter_entry(|e| !is_ignored(e))
	{
		let entry = match entry {
			Ok(entry) => entry,
			Err(_) => continue,
		};
		if !entry.file_type().is_file() {
			continue;
		}
		if entry.path().extension().is_some_and(|ext| ext == "rs") {
			let content = match fs::read_to_string(entry.path()) {
				Ok(c) => c,
				Err(_) => continue,
			};

			check_numbered_comments_in_content(&content, entry.path(), &mut issues);
		}
	}
	issues
}

fn check_dependencies(
	path: &Path,
	section: &str,
	table: &Table,
	content: &str,
	issues: &mut Vec<String>,
) {
	let is_workspace_root = section == "workspace.dependencies";

	if !is_workspace_root {
		for (name, value) in table.iter() {
			check_workspace_usage(path, section, name, value, issues);
		}
	} else {
		for (name, value) in table.iter() {
			check_specific_version(path, section, name, value, issues);
		}
	}

	let section_header = if is_workspace_root {
		"[workspace.dependencies]".to_string()
	} else {
		format!("[{section}]")
	};

	if let Some(start) = content.find(&section_header) {
		let section_content = &content[start + section_header.len()..];
		let end = section_content.find("\n[").unwrap_or(section_content.len());
		let active_content = &section_content[..end];

		let mut current_block = Vec::new();
		for line in active_content.lines() {
			let trimmed = line.trim();
			if trimmed.is_empty() {
				if !current_block.is_empty() {
					check_order(path, section, &current_block, issues);
					current_block.clear();
				}
				continue;
			}
			if trimmed.starts_with('#') || line.starts_with(|c: char| c.is_whitespace()) {
				continue;
			}
			if let Some(eq_idx) = line.find('=') {
				let name = line[..eq_idx].trim().to_string();
				current_block.push(name);
			}
		}
		if !current_block.is_empty() {
			check_order(path, section, &current_block, issues);
		}
	}
}

fn check_specific_version(
	path: &Path,
	section: &str,
	name: &str,
	value: &Item,
	issues: &mut Vec<String>,
) {
	let version_str = if let Some(s) = value.as_str() {
		Some(s)
	} else if let Some(table) = value.as_table_like() {
		if table.get("path").is_some() || table.get("git").is_some() {
			return;
		}
		table.get("version").and_then(|v| v.as_str())
	} else {
		None
	};

	if let Some(version) = version_str {
		let v = version.trim();
		let starts_with_digit = v.starts_with(|c: char| c.is_ascii_digit());
		let dot_count = v.chars().filter(|&c| c == '.').count();

		if !starts_with_digit || dot_count < 2 {
			issues.push(format!(
				"{}:[{}] '{}' version should be specific (x.y.z), found \"{}\"",
				path.display(),
				section,
				name,
				version
			));
		}
	}
}

fn check_order(path: &Path, section: &str, names: &[String], issues: &mut Vec<String>) {
	let mut sorted_names = names.to_vec();
	sorted_names.sort();

	if names != sorted_names {
		issues.push(format!(
			"{}:[{}] dependencies within a block are not in alphabetical order",
			path.display(),
			section
		));

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

fn check_impl_spacing(content: &str, path: &Path, issues: &mut Vec<String>) {
	let re_impl = Regex::new(r"(?m)^[ \t]*\}\r?\n[ \t]*(?:#\[.*\]\r?\n[ \t]*)*impl").unwrap();

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
	let re_use = Regex::new(r"(?m)^[ \t]+use\s").unwrap();

	let mut in_tests_mod = false;
	let mut tests_mod_brace_depth = 0;
	let mut brace_depth = 0;

	for (i, line) in content.lines().enumerate() {
		let trim_line = line.trim();
		let code_part = trim_line.split("//").next().unwrap_or("");

		if !in_tests_mod && (code_part.contains("mod tests") || code_part.contains("cfg(test)")) {
			in_tests_mod = true;
			tests_mod_brace_depth = brace_depth;
		}

		if !in_tests_mod && re_use.is_match(line) {
			issues.push(format!(
				"{}:{} - 'use' statement should be at the top level (found indented use).",
				path.display(),
				i + 1
			));
		}

		let open_braces = code_part.matches('{').count();
		let close_braces = code_part.matches('}').count();

		brace_depth += open_braces;
		if brace_depth >= close_braces {
			brace_depth -= close_braces;
		} else {
			brace_depth = 0;
		}

		if in_tests_mod && brace_depth <= tests_mod_brace_depth && close_braces > 0 {
			in_tests_mod = false;
		}
	}
}

fn check_numbered_comments_in_content(content: &str, path: &Path, issues: &mut Vec<String>) {
	let re = Regex::new(r"//\s*\d+\.\s").unwrap();

	for (i, line) in content.lines().enumerate() {
		let trimmed = line.trim();

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

fn format_issues(header: &str, issues: &[String]) -> String {
	let mut output = String::from(header);
	for issue in issues {
		output.push_str("\n  ");
		output.push_str(issue);
	}
	output
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn numbered_comment_check_only_flags_pure_comments() {
		let mut issues = Vec::new();
		let content = format!(
			"// {}. remove this\nlet value = \"// 2. keep this\";\n// Regular comment\n",
			1
		);
		check_numbered_comments_in_content(&content, Path::new("sample.rs"), &mut issues);

		assert_eq!(issues, vec!["sample.rs:1 - // 1. remove this"]);
	}

	#[test]
	fn code_fmt_check_flags_adjacent_impl_blocks() {
		let mut issues = Vec::new();
		let content = format!("impl Foo {{\n{}\n{} Bar {{\n}}\n", "}", "impl");
		check_impl_spacing(&content, Path::new("sample.rs"), &mut issues);

		assert_eq!(
			issues,
			vec![
				"sample.rs:2 - Add a blank line between the closing brace '}' and the next 'impl' block."
			]
		);
	}

	#[test]
	fn code_fmt_check_flags_adjacent_impl_blocks_with_crlf() {
		let mut issues = Vec::new();
		let content = format!("impl Foo {{\r\n{}\r\n{} Bar {{\r\n}}\r\n", "}", "impl");
		check_impl_spacing(&content, Path::new("sample.rs"), &mut issues);

		assert_eq!(
			issues,
			vec![
				"sample.rs:2 - Add a blank line between the closing brace '}' and the next 'impl' block."
			]
		);
	}

	#[test]
	fn workspace_check_flags_hardcoded_dependency_versions() {
		let dir =
			std::env::temp_dir().join(format!("nimbis-xtask-workspace-{}", std::process::id()));
		std::fs::create_dir_all(dir.join("crates/demo")).unwrap();
		std::fs::write(
			dir.join("Cargo.toml"),
			r#"[workspace]
members = ["crates/demo"]

[workspace.dependencies]
regex = "1.0.0"
"#,
		)
		.unwrap();
		std::fs::write(
			dir.join("crates/demo/Cargo.toml"),
			r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"

[dependencies]
regex = "1"
"#,
		)
		.unwrap();

		let issues = workspace_issues(&dir);

		assert!(issues.iter().any(|issue| {
			issue.contains("[dependencies] 'regex' uses hardcoded version")
				&& issue.contains("should use workspace = true")
		}));

		std::fs::remove_dir_all(dir).unwrap();
	}

	#[test]
	fn workspace_check_scans_xtask_manifest() {
		let dir = std::env::temp_dir().join(format!(
			"nimbis-xtask-workspace-scan-{}",
			std::process::id()
		));
		std::fs::create_dir_all(dir.join("xtask")).unwrap();
		std::fs::write(
			dir.join("Cargo.toml"),
			r#"[workspace]
members = ["xtask"]

[workspace.dependencies]
regex = "1.0.0"
"#,
		)
		.unwrap();
		std::fs::write(
			dir.join("xtask/Cargo.toml"),
			r#"[package]
name = "xtask"
version = "0.1.0"
edition = "2024"

[dependencies]
regex = "1"
"#,
		)
		.unwrap();

		let issues = workspace_issues(&dir);

		assert!(
			issues
				.iter()
				.any(|issue| issue.contains("[dependencies] 'regex' uses hardcoded version"))
		);

		std::fs::remove_dir_all(dir).unwrap();
	}

	#[test]
	fn workspace_check_ignores_multiline_dependency_inner_keys_for_ordering() {
		let dir = std::env::temp_dir().join(format!(
			"nimbis-xtask-workspace-multiline-{}",
			std::process::id()
		));
		std::fs::create_dir_all(dir.join("crates/demo")).unwrap();
		std::fs::write(
			dir.join("Cargo.toml"),
			r#"[workspace]
members = ["crates/demo"]

[workspace.dependencies]
regex = "1.0.0"
serde = "1.0.0"
"#,
		)
		.unwrap();
		std::fs::write(
			dir.join("crates/demo/Cargo.toml"),
			r#"[package]
name = "demo"
version = "0.1.0"
edition = "2024"

[dependencies]
regex = {
    workspace = true,
    features = ["std"],
}
serde = { workspace = true }
"#,
		)
		.unwrap();

		let issues = workspace_issues(&dir);

		assert!(
			!issues
				.iter()
				.any(|issue| issue
					.contains("dependencies within a block are not in alphabetical order")),
			"{issues:?}"
		);

		std::fs::remove_dir_all(dir).unwrap();
	}
}
