pub const LOGO: &str = r#"
▄▄▄▄  ▄ ▄▄▄▄  ▗▖   ▄  ▄▄▄
█   █ ▄ █ █ █ ▐▌   ▄ ▀▄▄
█   █ █ █   █ ▐▛▀▚▖█ ▄▄▄▀
      █       ▐▙▄▞▘█
"#;

const LABEL_WIDTH: usize = 13;

#[allow(clippy::disallowed_macros)]
pub fn show_logo() {
	let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

	let version = format!(
		"v{} ({}{})",
		env!("CARGO_PKG_VERSION"),
		env!("NIMBIS_GIT_HASH"),
		env!("NIMBIS_GIT_DIRTY"),
	);

	let entries: &[(&str, &str)] = &[
		("Version", &version),
		("Git Branch", env!("NIMBIS_GIT_BRANCH")),
		("Build Date", env!("NIMBIS_BUILD_DATE")),
		("Rust", env!("NIMBIS_RUSTC_VERSION")),
		("Target", env!("NIMBIS_TARGET")),
		("Started", &now),
	];

	let info: String = entries
		.iter()
		.map(|(label, value)| format!("{label:<LABEL_WIDTH$}{value}"))
		.collect::<Vec<_>>()
		.join("\n");

	println!("{}\n{}\n", LOGO.trim_end(), info);
}
