use bytes::Bytes;
use bytes::BytesMut;
use resp::ParseError;
use resp::RespParseResult;
use resp::RespParser;
use resp::RespValue;

#[test]
fn test_legacy_parse_incomplete() {
	let mut buf = BytesMut::new();
	buf.extend_from_slice(b"+HEL");

	// Legacy generic parse should return Error due to insufficient data
	let result = resp::parse(&mut buf);
	assert!(matches!(result, Err(ParseError::UnexpectedEOF)));

	// Try again with full data
	buf.extend_from_slice(b"LO\r\n");
	let result = resp::parse(&mut buf);
	match result {
		Ok(RespValue::SimpleString(s)) => assert_eq!(s, "HELLO"),
		_ => panic!("Expected SimpleString(HELLO), got {:?}", result),
	}
}

#[test]
fn test_streaming_parse_success() {
	let mut parser = RespParser::new();
	let mut buf = BytesMut::new();

	// Partial write
	buf.extend_from_slice(b"+HEL");
	let result = parser.parse(&mut buf);
	assert!(matches!(result, RespParseResult::Incomplete));

	// Buffer should still contain "+HEL" because peek_line doesn't consume partial
	assert_eq!(&buf[..], b"+HEL");

	// Complete the write
	buf.extend_from_slice(b"LO\r\n");
	let result = parser.parse(&mut buf);
	if let RespParseResult::Complete(RespValue::SimpleString(s)) = result {
		assert_eq!(s, "HELLO");
	} else {
		panic!("Expected Complete(SimpleString), got {:?}", result);
	}
}

#[test]
fn test_streaming_array_split() {
	let mut parser = RespParser::new();
	let mut buf = BytesMut::new();

	// *2\r\n$3\r\nfoo\r\n$3\r\nbar\r\n

	// Write header *2\r\n
	buf.extend_from_slice(b"*2\r\n");
	// Write first element partial $3\r\nf
	buf.extend_from_slice(b"$3\r\nf");

	let result = parser.parse(&mut buf);
	assert!(matches!(result, RespParseResult::Incomplete));

	// Write rest of first element oo\r\n
	buf.extend_from_slice(b"oo\r\n");

	let result = parser.parse(&mut buf);
	// Still incomplete because we need the second element
	assert!(matches!(result, RespParseResult::Incomplete));

	// Finish array
	buf.extend_from_slice(b"$3\r\nbar\r\n");

	let result = parser.parse(&mut buf);
	if let RespParseResult::Complete(RespValue::Array(arr)) = result {
		assert_eq!(arr.len(), 2);
		assert_eq!(arr[0], RespValue::BulkString(Bytes::from("foo")));
		assert_eq!(arr[1], RespValue::BulkString(Bytes::from("bar")));
	} else {
		panic!("Expected Complete(Array), got {:?}", result);
	}
}
