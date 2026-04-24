use bytes::BytesMut;
use resp::ParseError;
use resp::RespParseResult;
use resp::RespParser;
use resp::parse;
use rstest::rstest;

fn assert_parse_err(input: &[u8], matcher: impl FnOnce(ParseError)) {
	let mut buf = BytesMut::from(input);
	let err = parse(&mut buf).expect_err("expected parse to fail");
	matcher(err);
}

#[rstest]
#[case(b"+OK".as_slice())]
#[case(b"$5\r\nabc".as_slice())]
fn unexpected_eof_for_incomplete_frames(#[case] input: &[u8]) {
	assert_parse_err(input, |err| {
		assert!(matches!(err, ParseError::UnexpectedEOF));
	});
}

#[rstest]
#[case(b"\x01PING\r\n", '\x01')]
#[case(b"\x7FPING\r\n", '\x7f')]
fn invalid_type_marker_reports_marker_char(#[case] input: &[u8], #[case] expected_marker: char) {
	assert_parse_err(input, |err| match err {
		ParseError::InvalidTypeMarker(marker) => assert_eq!(marker, expected_marker),
		other => panic!("expected InvalidTypeMarker, got {other:?}"),
	});
}

#[rstest]
#[case(b"$3\r\nabc\rx", "Missing CRLF")]
#[case(b"#x\r\n", "Boolean")]
#[case(b"=4\r\ntext\r\n", "Verbatim string")]
fn invalid_format_cases_include_useful_message(
	#[case] input: &[u8],
	#[case] expected_msg_part: &str,
) {
	assert_parse_err(input, |err| match err {
		ParseError::InvalidFormat(msg) => assert!(msg.contains(expected_msg_part)),
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
