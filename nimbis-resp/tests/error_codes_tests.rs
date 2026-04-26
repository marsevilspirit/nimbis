use bytes::BytesMut;
use nimbis_resp::ParseError;
use nimbis_resp::RespParseResult;
use nimbis_resp::RespParser;
use nimbis_resp::parse;
use rstest::rstest;

#[rstest]
#[case(b"+OK".as_slice())]
#[case(b"$5\r\nabc".as_slice())]
fn unexpected_eof_for_incomplete_frames(#[case] input: &[u8]) {
	let mut buf = BytesMut::from(input);
	let result = parse(&mut buf);
	assert_eq!(result, Err(ParseError::UnexpectedEOF));
}

#[rstest]
#[case(b"\x01PING\r\n", '\x01')]
#[case(b"\x7FPING\r\n", '\x7f')]
fn invalid_type_marker_reports_marker_char(#[case] input: &[u8], #[case] expected_marker: char) {
	let mut buf = BytesMut::from(input);
	let result = parse(&mut buf);
	assert_eq!(result, Err(ParseError::InvalidTypeMarker(expected_marker)));
}

#[rstest]
#[case(b"$3\r\nabc\rx", "Missing CRLF")]
#[case(b"#x\r\n", "Boolean")]
#[case(b"=4\r\ntext\r\n", "Verbatim string")]
fn invalid_format_cases_include_useful_message(
	#[case] input: &[u8],
	#[case] expected_msg_part: &str,
) {
	let mut buf = BytesMut::from(input);
	let result = parse(&mut buf);
	match result {
		Err(ParseError::InvalidFormat(msg)) => assert!(msg.contains(expected_msg_part)),
		other => panic!("expected InvalidFormat containing '{expected_msg_part}', got {other:?}"),
	}
}

#[test]
fn invalid_integer_detected() {
	let mut buf = BytesMut::from(&b":12x\r\n"[..]);
	let result = parse(&mut buf);
	assert_eq!(
		result,
		Err(ParseError::InvalidInteger("invalid digit: x".into()))
	);
}

#[test]
fn invalid_bulk_string_length_detected() {
	let mut buf = BytesMut::from(&b"$-2\r\n"[..]);
	let result = parse(&mut buf);
	assert_eq!(result, Err(ParseError::InvalidBulkStringLength(-2)));
}

#[test]
fn invalid_array_length_detected() {
	let mut buf = BytesMut::from(&b"*-2\r\n"[..]);
	let result = parse(&mut buf);
	assert_eq!(result, Err(ParseError::InvalidArrayLength(-2)));
}

#[test]
fn utf8_error_for_invalid_double_payload() {
	let mut buf = BytesMut::from(&b",\xFF\r\n"[..]);
	let result = parse(&mut buf);
	match result {
		Err(ParseError::Utf8Error(msg)) => assert!(!msg.is_empty()),
		other => panic!("expected Utf8Error, got {other:?}"),
	}
}

#[test]
fn invalid_double_detected() {
	let mut buf = BytesMut::from(&b",1.2.3\r\n"[..]);
	let result = parse(&mut buf);
	match result {
		Err(ParseError::InvalidDouble(msg)) => assert!(!msg.is_empty()),
		other => panic!("expected InvalidDouble, got {other:?}"),
	}
}

#[test]
fn resp_parser_parse_returns_error_for_invalid_type_marker() {
	let mut parser = RespParser::new();
	let mut buf = BytesMut::from(&b"\x01PING\r\n"[..]);
	let result = parser.parse(&mut buf);
	assert!(matches!(
		result,
		RespParseResult::Error(nimbis_resp::RespError::Parse(
			ParseError::InvalidTypeMarker('\x01')
		))
	));
}

#[test]
fn parse_int_error_converts_to_invalid_integer() {
	let int_err = "12x".parse::<i64>().expect_err("expected parse int error");
	let err = ParseError::from(int_err);
	assert_eq!(
		err,
		ParseError::InvalidInteger("invalid digit found in string".into())
	);
}

#[test]
fn parse_float_error_converts_to_invalid_double() {
	let float_err = "1.2.3"
		.parse::<f64>()
		.expect_err("expected parse float error");
	let err = ParseError::from(float_err);
	assert_eq!(
		err,
		ParseError::InvalidDouble("invalid float literal".into())
	);
}
