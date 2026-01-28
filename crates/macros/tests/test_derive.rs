use std::str::FromStr;

use config::OnlineConfig;
use rstest::rstest;

#[derive(OnlineConfig, Default)]
struct TestConfig {
	pub addr: String,
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

#[test]
fn test_list_fields() {
	let fields = TestConfig::list_fields();
	assert_eq!(fields.len(), 3);
	assert!(fields.contains(&"addr"));
	assert!(fields.contains(&"port"));
	assert!(fields.contains(&"id"));
}

#[test]
fn test_get_all_fields() {
	let conf = TestConfig::default();
	let all_fields = conf.get_all_fields();
	assert_eq!(all_fields.len(), 3);

	// Check that all expected fields are present
	let field_map: std::collections::HashMap<_, _> = all_fields.into_iter().collect();
	assert_eq!(field_map.get("addr"), Some(&"".to_string()));
	assert_eq!(field_map.get("port"), Some(&"0".to_string()));
	assert_eq!(field_map.get("id"), Some(&"0".to_string()));
}

#[test]
fn test_match_fields_all() {
	let fields = TestConfig::match_fields("*");
	assert_eq!(fields.len(), 3);
	assert!(fields.contains(&"addr"));
	assert!(fields.contains(&"port"));
	assert!(fields.contains(&"id"));
}

#[test]
fn test_match_fields_prefix() {
	let fields = TestConfig::match_fields("addr*");
	assert_eq!(fields, vec!["addr"]);

	// Test prefix that doesn't match
	let fields = TestConfig::match_fields("xyz*");
	assert!(fields.is_empty());
}

#[test]
fn test_match_fields_suffix() {
	let fields = TestConfig::match_fields("*port");
	assert_eq!(fields, vec!["port"]);

	let fields = TestConfig::match_fields("*t");
	assert_eq!(fields, vec!["port"]);
}

#[test]
fn test_match_fields_contains() {
	let fields = TestConfig::match_fields("*dd*");
	assert_eq!(fields, vec!["addr"]);

	let fields = TestConfig::match_fields("*or*");
	assert_eq!(fields, vec!["port"]);
}

#[test]
fn test_match_fields_exact() {
	let fields = TestConfig::match_fields("addr");
	assert_eq!(fields, vec!["addr"]);

	let fields = TestConfig::match_fields("nonexistent");
	assert!(fields.is_empty());
}

#[derive(Clone, Default)]
struct CallbackLog(std::rc::Rc<std::cell::RefCell<Vec<String>>>);

impl std::fmt::Display for CallbackLog {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "{:?}", self.0)
	}
}

impl std::str::FromStr for CallbackLog {
	type Err = String;
	fn from_str(_: &str) -> Result<Self, Self::Err> {
		Ok(CallbackLog::default())
	}
}

#[derive(OnlineConfig, Default)]
struct TestCallbackConfig {
	#[online_config(callback = "on_addr_change")]
	pub addr: String,
	#[online_config(callback = "on_port_change")]
	pub port: u16,
	#[online_config(immutable)]
	pub id: i32,
	// Use cells to track callback invocations because set_field takes &mut self
	// and we want to verify side effects
	#[online_config(immutable)]
	pub callbacks: CallbackLog,
}

impl TestCallbackConfig {
	fn on_addr_change(&mut self) -> Result<(), String> {
		self.callbacks
			.0
			.borrow_mut()
			.push(format!("addr changed to {}", self.addr));
		Ok(())
	}

	fn on_port_change(&mut self) -> Result<(), String> {
		self.callbacks
			.0
			.borrow_mut()
			.push(format!("port changed to {}", self.port));
		Ok(())
	}
}

#[test]
fn test_callback() {
	let callbacks = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
	let mut conf = TestCallbackConfig {
		callbacks: CallbackLog(callbacks.clone()),
		..Default::default()
	};

	// Test address change callback
	conf.set_field("addr", "192.168.1.1").unwrap();
	assert_eq!(callbacks.borrow().len(), 1);
	assert_eq!(callbacks.borrow()[0], "addr changed to 192.168.1.1");

	// Test port change callback
	conf.set_field("port", "9090").unwrap();
	assert_eq!(callbacks.borrow().len(), 2);
	assert_eq!(callbacks.borrow()[1], "port changed to 9090");

	// Test immutable field (no callback should be triggered even if we could set
	// it)
	assert!(conf.set_field("id", "999").is_err());
	assert_eq!(callbacks.borrow().len(), 2);
	assert_eq!(conf.id, 0);
}
