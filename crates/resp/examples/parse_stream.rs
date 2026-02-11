use bytes::BytesMut;
use resp::RespParseResult;
use resp::RespParser;

fn main() {
	println!("--- RESP Streaming Parse Example ---");

	// Simulate a TCP stream with fragmented data
	// We are sending:
	// - A Simple String: "+OK\r\n"
	// - An Integer: ":1000\r\n"
	// - An Array: "*2\r\n$3\r\nSET\r\n$3\r\nkey\r\n"
	// - But split into random chunks.
	let data_chunks = vec![
		b"+O".as_slice(),
		b"K\r\n:1".as_slice(),
		b"00".as_slice(),
		b"0\r\n*2\r\n$3\r\nSE".as_slice(),
		b"T\r\n$3\r\nk".as_slice(),
		b"ey\r\n".as_slice(),
	];

	let mut parser = RespParser::new();
	let mut buffer = BytesMut::new();

	for (i, chunk) in data_chunks.iter().enumerate() {
		println!(
			"\n[Stream] Received Chunk {}: {:?}",
			i,
			std::str::from_utf8(chunk).unwrap()
		);

		buffer.extend_from_slice(chunk);

		loop {
			// Attempt to parse
			match parser.parse(&mut buffer) {
				RespParseResult::Complete(value) => {
					println!("[Parser] Complete: {:?}", value);
					// Continue loop to see if there are more complete frames in
					// the buffer
				}
				RespParseResult::Incomplete => {
					println!("[Parser] Incomplete, waiting for more data...");
					break;
				}
				RespParseResult::Error(e) => {
					eprintln!("[Parser] Error: {:?}", e);
					break;
				}
			}
		}
	}
}
