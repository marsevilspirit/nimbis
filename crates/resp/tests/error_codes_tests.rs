use bytes::BytesMut;
use resp::ParseError;
use resp::RespParseResult;
use resp::RespParser;
use resp::parse;

fn assert_parse_err(input: &[u8], matcher: impl FnOnce(ParseError)) {
	let mut buf = BytesMut::from(input);
	let err = parse(&mut buf).expect_err("expected parse to fail");
	matcher(err);
}

#[test]
fn unexpected_eof_for_incomplete_frames() {
	for input in [b"+OK".as_slice(), b"$5\r\nabc".as_slice()] {
		assert_parse_err(input, |err| {
			assert!(matches!(err, ParseError::UnexpectedEOF));
		});
	}
}

#[test]
fn invalid_type_marker_reports_marker_char() {
	assert_parse_err(b"\x01PING\r\n", |err| match err {
		ParseError::InvalidTypeMarker(marker) => assert_eq!(marker, '\x01'),
		other => panic!("expected InvalidTypeMarker, got {other:?}"),
	});

	assert_parse_err(b"\x7FPING\r\n", |err| match err {
		ParseError::InvalidTypeMarker(marker) => assert_eq!(marker, '\x7f'),
		other => panic!("expected InvalidTypeMarker, got {other:?}"),
	});
}

#[test]
fn invalid_format_cases_include_useful_message() {
	assert_parse_err(b"$3\r\nabc\rx", |err| match err {
		ParseError::InvalidFormat(msg) => assert!(msg.contains("Missing CRLF")),
		other => panic!("expected InvalidFormat, got {other:?}"),
	});

	assert_parse_err(b"#x\r\n", |err| match err {
		ParseError::InvalidFormat(msg) => assert!(msg.contains("Boolean")),
		other => panic!("expected InvalidFormat, got {other:?}"),
	});

	assert_parse_err(b"=4\r\ntext\r\n", |err| match err {
		ParseError::InvalidFormat(msg) => assert!(msg.contains("Verbatim string")),
		other => panic!("expected InvalidFormat, got {other:?}"),
	});
}

#[test]
fn invalid_integer_detected() {
	assert_parse_err(b":12x\r\n", |err| {
		assert!(matches!(err, ParseError::InvalidInteger(_)));
	});
}

#[test]
fn invalid_bulk_string_length_detected() {
	assert_parse_err(b"$-2\r\n", |err| {
		assert_eq!(err, ParseError::InvalidBulkStringLength(-2));
	});
}

#[test]
fn invalid_array_length_detected() {
	assert_parse_err(b"*-2\r\n", |err| {
		assert_eq!(err, ParseError::InvalidArrayLength(-2));
	});
}

#[test]
fn utf8_error_for_invalid_double_payload() {
	assert_parse_err(b",\xFF\r\n", |err| match err {
		ParseError::Utf8Error(_) => {}
		ParseError::InvalidFormat(msg) => assert!(msg.to_lowercase().contains("utf-8")),
		other => panic!("expected Utf8-related parse error, got {other:?}"),
	});
}

#[test]
fn invalid_double_detected() {
	assert_parse_err(b",1.2.3\r\n", |err| {
		assert!(matches!(err, ParseError::InvalidDouble(_)));
	});
}

#[test]
fn resp_parser_parse_returns_error_for_invalid_type_marker() {
	let mut parser = RespParser::new();
	let mut buf = BytesMut::from(&b"\x01PING\r\n"[..]);
	let result = parser.parse(&mut buf);
	assert!(matches!(
		result,
		RespParseResult::Error(resp::RespError::Parse(ParseError::InvalidTypeMarker(
			'\x01'
		)))
	));
}
