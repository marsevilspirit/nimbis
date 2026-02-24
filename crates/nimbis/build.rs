#![allow(clippy::disallowed_macros)]

use std::process::Command;

fn main() {
	// Get git commit short hash
	let git_hash = run_command("git", &["rev-parse", "--short", "HEAD"]);
	println!("cargo:rustc-env=NIMBIS_GIT_HASH={}", git_hash);

	// Get git branch name
	let git_branch = run_command("git", &["rev-parse", "--abbrev-ref", "HEAD"]);
	println!("cargo:rustc-env=NIMBIS_GIT_BRANCH={}", git_branch);

	// Get build date
	let build_date = run_command("date", &["+%Y-%m-%d %H:%M:%S"]);
	println!("cargo:rustc-env=NIMBIS_BUILD_DATE={}", build_date);

	// Get rustc version
	let rustc_version = run_command("rustc", &["--version"]);
	println!("cargo:rustc-env=NIMBIS_RUSTC_VERSION={}", rustc_version);

	// Get target platform from Cargo environment variables
	let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_else(|_| "unknown".into());
	let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_else(|_| "unknown".into());
	println!(
		"cargo:rustc-env=NIMBIS_TARGET={}-{}",
		target_arch, target_os
	);

	// Check if repo is dirty
	let dirty = run_command("git", &["status", "--porcelain"]);
	let dirty_flag = if dirty.is_empty() { "" } else { "-dirty" };
	println!("cargo:rustc-env=NIMBIS_GIT_DIRTY={}", dirty_flag);

	// Rebuild when git HEAD changes (new commits, branch switch, etc.)
	println!("cargo:rerun-if-changed=../../.git/HEAD");
	println!("cargo:rerun-if-changed=../../.git/refs");
}

fn run_command(cmd: &str, args: &[&str]) -> String {
	Command::new(cmd)
		.args(args)
		.output()
		.ok()
		.and_then(|output| {
			if output.status.success() {
				String::from_utf8(output.stdout).ok()
			} else {
				None
			}
		})
		.unwrap_or_else(|| "unknown".into())
		.trim()
		.to_string()
}
