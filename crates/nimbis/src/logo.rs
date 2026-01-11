pub const LOGO: &str = r#"
▄▄▄▄  ▄ ▄▄▄▄  ▗▖   ▄  ▄▄▄
█   █ ▄ █ █ █ ▐▌   ▄ ▀▄▄
█   █ █ █   █ ▐▛▀▚▖█ ▄▄▄▀
      █       ▐▙▄▞▘█
"#;

pub fn show_logo() {
	let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();

	let info = format!(
		r#"Version:     v{}
Started:     {}"#,
		env!("CARGO_PKG_VERSION"),
		now
	);

	println!("{}\n{}\n", LOGO.trim_end(), info);
}
