mod common;

use bytes::Bytes;
use common::mock::MockNimbis;
use resp::RespValue;
use serial_test::serial;

#[tokio::test]
#[serial]
async fn test_string_command() {
	let mut nimbis = MockNimbis::new().await.expect("start nimbis");

	let pong = nimbis.ping().await.expect("ping response");
	assert_eq!(pong, RespValue::SimpleString(Bytes::from_static(b"PONG")));

	let set = nimbis
		.set("it:string:key", "value-1")
		.await
		.expect("set response");
	assert_eq!(set, RespValue::SimpleString(Bytes::from_static(b"OK")));

	let get = nimbis.get("it:string:key").await.expect("get response");
	assert_eq!(get, RespValue::BulkString(Bytes::from_static(b"value-1")));

	let set_overwrite = nimbis
		.set("it:string:key", "value-2")
		.await
		.expect("overwrite response");
	assert_eq!(
		set_overwrite,
		RespValue::SimpleString(Bytes::from_static(b"OK"))
	);

	let get_overwrite = nimbis
		.get("it:string:key")
		.await
		.expect("get overwritten response");
	assert_eq!(
		get_overwrite,
		RespValue::BulkString(Bytes::from_static(b"value-2"))
	);
}
