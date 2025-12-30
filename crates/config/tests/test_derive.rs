use std::str::FromStr;

use config::OnlineConfig;
use rstest::rstest;

#[derive(OnlineConfig, Default)]
struct TestConfig {
	pub addr: String,
	pub port: u16,
}

#[rstest]
#[case("addr", "127.0.0.1")]
#[case("port", "8080")]
fn test_set_field_success(#[case] key: &str, #[case] value: &str) {
	let mut conf = TestConfig::default();
	assert!(conf.set_field(key, value).is_ok());

	match key {
		"addr" => assert_eq!(conf.addr, value),
		"port" => assert_eq!(conf.port.to_string(), value),
		_ => unreachable!(),
	}
}

#[rstest]
#[case("unknown", "value")]
#[case("port", "invalid")]
fn test_set_field_failure(#[case] key: &str, #[case] value: &str) {
	let mut conf = TestConfig::default();
	assert!(conf.set_field(key, value).is_err());
}
