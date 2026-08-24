use bytes::Bytes;
use nimbis_storage::DataType;

const INVALID_DATA_TYPE_ERROR: &str =
	"ERR invalid key type; expected STRING, HASH, LIST, SET, or ZSET";

pub(super) struct TypedKeyArgs {
	data_type: DataType,
	key: Bytes,
}

impl TypedKeyArgs {
	pub(super) fn parse(args: &[Bytes]) -> Result<Self, &'static str> {
		let (data_type, args) = parse_typed_args(args)?;
		let key = args
			.first()
			.expect("typed key command arity is validated before parsing")
			.clone();
		Ok(Self { data_type, key })
	}

	pub(super) fn into_parts(self) -> (DataType, Bytes) {
		(self.data_type, self.key)
	}
}

pub(super) struct TypedKeysArgs<'a> {
	data_type: DataType,
	keys: &'a [Bytes],
}

impl<'a> TypedKeysArgs<'a> {
	pub(super) fn parse(args: &'a [Bytes]) -> Result<Self, &'static str> {
		let (data_type, keys) = parse_typed_args(args)?;
		debug_assert!(
			!keys.is_empty(),
			"typed key command requires at least one key"
		);
		Ok(Self { data_type, keys })
	}

	pub(super) fn data_type(&self) -> DataType {
		self.data_type
	}

	pub(super) fn keys(&self) -> impl Iterator<Item = Bytes> + '_ {
		self.keys.iter().cloned()
	}
}

fn parse_typed_args(args: &[Bytes]) -> Result<(DataType, &[Bytes]), &'static str> {
	let (data_type, args) = args
		.split_first()
		.expect("typed key command arity is validated before parsing");
	Ok((parse_data_type(data_type)?, args))
}

fn parse_data_type(bytes: &[u8]) -> Result<DataType, &'static str> {
	if bytes.eq_ignore_ascii_case(b"string") {
		Ok(DataType::String)
	} else if bytes.eq_ignore_ascii_case(b"hash") {
		Ok(DataType::Hash)
	} else if bytes.eq_ignore_ascii_case(b"list") {
		Ok(DataType::List)
	} else if bytes.eq_ignore_ascii_case(b"set") {
		Ok(DataType::Set)
	} else if bytes.eq_ignore_ascii_case(b"zset") {
		Ok(DataType::ZSet)
	} else {
		Err(INVALID_DATA_TYPE_ERROR)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

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

	#[test]
	fn parses_single_typed_key_arguments() {
		let args = [Bytes::from_static(b"hash"), Bytes::from_static(b"key")];
		let (data_type, key) = TypedKeyArgs::parse(&args).unwrap().into_parts();

		assert_eq!(data_type, DataType::Hash);
		assert_eq!(key, Bytes::from_static(b"key"));
	}

	#[test]
	fn parses_multi_typed_key_arguments() {
		let args = [
			Bytes::from_static(b"SET"),
			Bytes::from_static(b"key-1"),
			Bytes::from_static(b"key-2"),
		];
		let parsed = TypedKeysArgs::parse(&args).unwrap();

		assert_eq!(parsed.data_type(), DataType::Set);
		assert_eq!(parsed.keys().collect::<Vec<_>>(), args[1..]);
	}
}
