use std::str::FromStr;

pub fn parse_int<T: FromStr>(bytes: &[u8]) -> Result<T, String> {
	let s = std::str::from_utf8(bytes)
		.map_err(|_| "ERR value is not an integer or out of range".to_string())?;
	s.parse::<T>()
		.map_err(|_| "ERR value is not an integer or out of range".to_string())
}
