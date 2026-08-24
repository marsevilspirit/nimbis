use std::str::FromStr;

use nimbis_storage::data_type::DataType;

const INVALID_DATA_TYPE_ERROR: &str =
	"ERR invalid key type; expected STRING, HASH, LIST, SET, or ZSET";

pub(super) fn parse_data_type(bytes: &[u8]) -> Result<DataType, &'static str> {
	DataType::from_name(bytes).ok_or(INVALID_DATA_TYPE_ERROR)
}

pub fn parse_int<T: FromStr>(bytes: &[u8]) -> Result<T, String> {
	let s = std::str::from_utf8(bytes)
		.map_err(|_| "ERR value is not an integer or out of range".to_string())?;
	s.parse::<T>()
		.map_err(|_| "ERR value is not an integer or out of range".to_string())
}

#[cfg(test)]
mod tests {
	use nimbis_storage::data_type::DataType;

	use super::INVALID_DATA_TYPE_ERROR;
	use super::parse_data_type;

	#[test]
	fn parses_supported_data_types_case_insensitively() {
		let cases: &[(&[u8], DataType)] = &[
			(b"STRING", DataType::String),
			(b"string", DataType::String),
			(b"HaSh", DataType::Hash),
			(b"LIST", DataType::List),
			(b"set", DataType::Set),
			(b"zSeT", DataType::ZSet),
		];

		for (input, expected) in cases {
			assert_eq!(parse_data_type(input), Ok(*expected));
		}
	}

	#[test]
	fn rejects_unknown_alias_and_binary_data_types() {
		for input in [
			b"".as_slice(),
			b"STR".as_slice(),
			b"ALL".as_slice(),
			b"s".as_slice(),
			b"SORTEDSET".as_slice(),
			b"\xff".as_slice(),
		] {
			assert_eq!(parse_data_type(input), Err(INVALID_DATA_TYPE_ERROR));
		}
	}
}
