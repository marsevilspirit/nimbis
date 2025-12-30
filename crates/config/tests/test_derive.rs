use std::str::FromStr;

use config::OnlineConfig;
use rstest::rstest;

#[derive(OnlineConfig, Default)]
struct TestConfig {
	#[online_config(mutable)]
	pub addr: String,
	#[online_config(mutable)]
	pub port: u16,
	#[online_config(immutable)]
	pub id: i32,
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

#[test]
fn test_immutable_field() {
	let mut conf = TestConfig::default();
	let res = conf.set_field("id", "123");
	assert!(res.is_err());
	assert_eq!(res.unwrap_err(), "Field 'id' is immutable");
	assert_eq!(conf.id, 0);
}

#[rstest]
#[case("addr", "")]
#[case("port", "0")]
#[case("id", "0")]
fn test_get_field_success(#[case] key: &str, #[case] expected: &str) {
	let conf = TestConfig::default();
	let result = conf.get_field(key);
	assert!(result.is_ok());
	assert_eq!(result.unwrap(), expected);
}

#[test]
fn test_get_field_after_set() {
	let mut conf = TestConfig::default();
	conf.set_field("addr", "127.0.0.1").unwrap();
	conf.set_field("port", "8080").unwrap();

	assert_eq!(conf.get_field("addr").unwrap(), "127.0.0.1");
	assert_eq!(conf.get_field("port").unwrap(), "8080");
	assert_eq!(conf.get_field("id").unwrap(), "0");
}

#[test]
fn test_get_field_unknown() {
	let conf = TestConfig::default();
	let result = conf.get_field("unknown");
	assert!(result.is_err());
	assert_eq!(result.unwrap_err(), "Field 'unknown' not found");
}
