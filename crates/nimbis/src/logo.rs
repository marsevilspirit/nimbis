pub const LOGO: &str = r#"
    _   __    _                  __       _
   / | / /   (_)   ____ ___     / /_     (_)   _____
  /  |/ /   / /   / __ `__ \   / __ \   / /   / ___/
 / /|  /   / /   / / / / / /  / /_/ /  / /   (__  )
/_/ |_/   /_/   /_/ /_/ /_/  /_.___/  /_/   /____/
"#;

pub fn show_logo() {
	println!("{}\tv{}", LOGO.trim_end(), env!("CARGO_PKG_VERSION"));
}
