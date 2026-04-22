mod common;

use bytes::Bytes;
use common::mock::MockNimbis;
use resp::RespValue;

#[tokio::test]
async fn test_mock_nimbis_supports_basic_command_flow() {
	let mut nimbis = MockNimbis::new().await.expect("start nimbis");

	let pong = nimbis.ping().await.expect("ping response");
	assert_eq!(pong, RespValue::SimpleString(Bytes::from_static(b"PONG")));

	let set = nimbis
		.set("it:key", "it:value")
		.await
		.expect("set response");
	assert_eq!(set, RespValue::SimpleString(Bytes::from_static(b"OK")));

	let get = nimbis.get("it:key").await.expect("get response");
	assert_eq!(get, RespValue::BulkString(Bytes::from_static(b"it:value")));
}
