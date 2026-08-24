mod mock;

use std::thread;

use mock::KeyType;
use mock::MockNimbisServer;
use mock::utils::resp_error;
use nimbis_resp::RespValue;
use serial_test::serial;

#[test]
#[serial]
fn test_string_command() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();

	assert_eq!(client.ping(), "PONG");
	assert_eq!(client.set("it:string:key", "value-1"), "OK");
	assert_eq!(client.get("it:string:key"), "value-1");
	assert_eq!(client.set("it:string:key", "value-2"), "OK");
	assert_eq!(client.get("it:string:key"), "value-2");
}

#[test]
#[serial]
fn test_raw_command_helpers() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();

	assert_eq!(
		client.execute(&["PING"]),
		RespValue::SimpleString("PONG".into())
	);

	assert_eq!(
		resp_error(client.execute(&["NO_SUCH_CMD"])),
		"ERR unknown command 'no_such_cmd'"
	);
	assert_eq!(
		resp_error(client.execute(&["GET"])),
		"ERR wrong number of arguments for 'get' command"
	);
}

#[test]
#[serial]
fn test_flushdb_helper() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();

	assert_eq!(client.set("it:flushdb:string", "value"), "OK");
	assert_eq!(client.hset("it:flushdb:hash", "field", "value"), 1);
	assert!(client.exists(KeyType::String, "it:flushdb:string"));
	assert!(client.exists(KeyType::Hash, "it:flushdb:hash"));

	assert!(client.flushdb());
	assert!(!client.exists(KeyType::String, "it:flushdb:string"));
	assert!(!client.exists(KeyType::Hash, "it:flushdb:hash"));
}

#[test]
#[serial]
fn test_del_and_exists() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();

	client.set("it:del:key", "hello");
	assert!(client.exists(KeyType::String, "it:del:key"));
	assert_eq!(client.del(KeyType::String, "it:del:key"), 1);
	assert!(!client.exists(KeyType::String, "it:del:key"));
	assert_eq!(client.del(KeyType::String, "it:del:key"), 0);
	assert_eq!(client.get("it:del:key"), "");
}

#[test]
#[serial]
fn test_del_and_exists_multi_key() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();

	assert_eq!(client.set("it:del:key:1", "hello"), "OK");
	assert_eq!(client.set("it:del:key:2", "world"), "OK");

	assert_eq!(
		client.exists_many(
			KeyType::String,
			&["it:del:key:1", "it:del:key:2", "missing"]
		),
		2
	);
	assert_eq!(
		client.del_many(
			KeyType::String,
			&["it:del:key:1", "it:del:key:2", "missing"]
		),
		2
	);
	assert_eq!(
		client.exists_many(KeyType::String, &["it:del:key:1", "it:del:key:2"]),
		0
	);
}

#[test]
#[serial]
fn test_pipeline_response_order() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();

	let responses = client.execute_pipeline(&[
		&["SET", "it:pipeline:key", "1"],
		&["INCR", "it:pipeline:key"],
		&["GET", "it:pipeline:key"],
	]);

	assert_eq!(responses[0], RespValue::SimpleString("OK".into()));
	assert_eq!(responses[1], RespValue::Integer(2));
	assert_eq!(responses[2], RespValue::bulk_string("2"));
}

#[test]
#[serial]
fn test_concurrent_incr_from_multiple_clients() {
	let server = MockNimbisServer::new();
	let mut setup_client = server.get_client();
	assert_eq!(setup_client.set("it:runtime:counter", "0"), "OK");
	drop(setup_client);

	thread::scope(|scope| {
		for _ in 0..8 {
			scope.spawn(|| {
				let mut client = server.get_client();
				for _ in 0..100 {
					client.incr("it:runtime:counter");
				}
			});
		}
	});

	let mut verify_client = server.get_client();
	assert_eq!(verify_client.get("it:runtime:counter"), "800");
}

#[test]
#[serial]
fn test_incr_decr() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();

	assert_eq!(client.incr("it:counter"), 1);
	assert_eq!(client.incr("it:counter"), 2);
	assert_eq!(client.incr("it:counter"), 3);
	assert_eq!(client.decr("it:counter"), 2);
	assert_eq!(client.decr("it:counter"), 1);
	assert_eq!(client.decr("it:counter"), 0);
	assert_eq!(client.decr("it:counter"), -1);
}

#[test]
#[serial]
fn test_append() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();

	assert_eq!(client.append("it:append:key", "hello"), 5);
	assert_eq!(client.append("it:append:key", " world"), 11);
	assert_eq!(client.get("it:append:key"), "hello world");
}

#[test]
#[serial]
fn test_hash_command() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();

	assert_eq!(client.hset("it:hash:user", "name", "alice"), 1);
	assert_eq!(client.hset("it:hash:user", "age", "30"), 1);
	assert_eq!(client.hget("it:hash:user", "name"), "alice");
	assert_eq!(client.hget("it:hash:user", "age"), "30");
	assert_eq!(client.hget("it:hash:user", "missing"), "");
	assert_eq!(client.hlen("it:hash:user"), 2);

	// overwrite
	assert_eq!(client.hset("it:hash:user", "name", "bob"), 0);
	assert_eq!(client.hget("it:hash:user", "name"), "bob");

	// hdel
	assert_eq!(client.hdel("it:hash:user", "age"), 1);
	assert_eq!(client.hdel("it:hash:user", "age"), 0);
	assert_eq!(client.hlen("it:hash:user"), 1);
}

#[test]
#[serial]
fn test_hmget() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();

	client.hset("it:hmget:h", "f1", "v1");
	client.hset("it:hmget:h", "f2", "v2");
	client.hset("it:hmget:h", "f3", "v3");

	let vals = client.hmget("it:hmget:h", &["f1", "f3", "missing"]);
	assert_eq!(vals[0], "v1");
	assert_eq!(vals[1], "v3");
	assert_eq!(vals[2], "");
}

#[test]
#[serial]
fn test_hgetall() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();

	client.hset("it:hgetall:h", "k1", "v1");
	client.hset("it:hgetall:h", "k2", "v2");

	let all = client.hgetall("it:hgetall:h");
	assert_eq!(all.len(), 4); // [field, value, field, value]
	assert!(all.contains(&"k1".to_string()));
	assert!(all.contains(&"v1".to_string()));
	assert!(all.contains(&"k2".to_string()));
	assert!(all.contains(&"v2".to_string()));
}

#[test]
#[serial]
fn test_list_lpush_rpush() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();

	assert_eq!(client.rpush("it:list:q", &["a", "b"]), 2);
	assert_eq!(client.lpush("it:list:q", &["z"]), 3);

	// order: z, a, b
	let items = client.lrange("it:list:q", 0, -1);
	assert_eq!(items, vec!["z", "a", "b"]);
	assert_eq!(client.llen("it:list:q"), 3);
}

#[test]
#[serial]
fn test_list_lpop_rpop() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();

	client.rpush("it:list:pop", &["1", "2", "3"]);

	assert_eq!(client.lpop("it:list:pop"), "1");
	assert_eq!(client.rpop("it:list:pop"), "3");
	assert_eq!(client.llen("it:list:pop"), 1);
	assert_eq!(client.lpop("it:list:pop"), "2");
	assert_eq!(client.lpop("it:list:pop"), ""); // empty list
}

#[test]
#[serial]
fn test_lrange() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();

	client.rpush("it:list:range", &["a", "b", "c", "d", "e"]);

	assert_eq!(client.lrange("it:list:range", 0, 2), vec!["a", "b", "c"]);
	assert_eq!(client.lrange("it:list:range", -2, -1), vec!["d", "e"]);
	assert_eq!(client.lrange("it:list:range", 1, 3), vec!["b", "c", "d"]);
}

#[test]
#[serial]
fn test_set_command() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();

	assert_eq!(client.sadd("it:set:s", &["a", "b", "c"]), 3);
	assert_eq!(client.sadd("it:set:s", &["a"]), 0); // duplicate
	assert_eq!(client.scard("it:set:s"), 3);

	assert!(client.sismember("it:set:s", "a"));
	assert!(!client.sismember("it:set:s", "x"));

	let members = client.smembers("it:set:s");
	assert_eq!(members.len(), 3);
	assert!(members.contains(&"a".to_string()));
	assert!(members.contains(&"b".to_string()));
	assert!(members.contains(&"c".to_string()));
}

#[test]
#[serial]
fn test_srem() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();

	client.sadd("it:set:rem", &["x", "y", "z"]);

	assert_eq!(client.srem("it:set:rem", &["x"]), 1);
	assert_eq!(client.srem("it:set:rem", &["x"]), 0); // already removed
	assert_eq!(client.scard("it:set:rem"), 2);
	assert!(!client.sismember("it:set:rem", "x"));
}

#[test]
#[serial]
fn test_zset_command() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();

	assert_eq!(
		client.zadd(
			"it:zset:z",
			&[("1.0", "alice"), ("2.5", "bob"), ("1.5", "carol")]
		),
		3
	);
	assert_eq!(client.zcard("it:zset:z"), 3);

	// zrange returns sorted by score
	let ranked = client.zrange("it:zset:z", 0, -1);
	assert_eq!(ranked, vec!["alice", "carol", "bob"]);

	assert_eq!(client.zscore("it:zset:z", "bob"), "2.5");
	assert_eq!(client.zscore("it:zset:z", "missing"), "");
}

#[test]
#[serial]
fn test_zrem() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();

	client.zadd("it:zset:rem", &[("1", "a"), ("2", "b"), ("3", "c")]);

	assert_eq!(client.zrem("it:zset:rem", &["b"]), 1);
	assert_eq!(client.zrem("it:zset:rem", &["b"]), 0);
	assert_eq!(client.zcard("it:zset:rem"), 2);

	let ranked = client.zrange("it:zset:rem", 0, -1);
	assert_eq!(ranked, vec!["a", "c"]);
}

#[test]
#[serial]
fn test_expire_and_ttl() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();

	client.set("it:ttl:key", "temp");

	// no expiry set
	assert_eq!(client.ttl(KeyType::String, "it:ttl:key"), -1);

	// set expiry
	assert!(client.expire(KeyType::String, "it:ttl:key", 300));
	let ttl = client.ttl(KeyType::String, "it:ttl:key");
	assert!(ttl > 0 && ttl <= 300);

	// expire non-existent key
	assert!(!client.expire(KeyType::String, "it:ttl:missing", 100));

	// ttl of non-existent key
	assert_eq!(client.ttl(KeyType::String, "it:ttl:missing"), -2);
}

#[test]
#[serial]
fn test_del_across_types() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();

	let key = "it:cross:key";
	client.set(key, "v");
	client.hset(key, "f", "v");
	client.rpush(key, &["a"]);
	client.sadd(key, &["m"]);
	client.zadd(key, &[("1", "z")]);

	for key_type in [
		KeyType::String,
		KeyType::Hash,
		KeyType::List,
		KeyType::Set,
		KeyType::ZSet,
	] {
		assert!(client.exists(key_type, key));
	}

	for (index, key_type) in [
		KeyType::Hash,
		KeyType::List,
		KeyType::Set,
		KeyType::ZSet,
		KeyType::String,
	]
	.into_iter()
	.enumerate()
	{
		assert_eq!(client.del(key_type, key), 1);
		assert!(!client.exists(key_type, key));
		for remaining_type in [
			KeyType::Hash,
			KeyType::List,
			KeyType::Set,
			KeyType::ZSet,
			KeyType::String,
		]
		.into_iter()
		.skip(index + 1)
		{
			assert!(client.exists(remaining_type, key));
		}
	}
}

#[test]
#[serial]
fn test_typed_expire_and_ttl_do_not_cross_namespaces() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();
	let key = "it:typed:ttl";

	client.set(key, "string");
	client.hset(key, "field", "hash");
	assert_eq!(client.ttl(KeyType::String, key), -1);
	assert_eq!(client.ttl(KeyType::Hash, key), -1);

	assert_eq!(
		client.execute(&["EXPIRE", "hash", key, "300"]),
		RespValue::Integer(1)
	);
	let hash_ttl = client
		.execute(&["TTL", "HaSh", key])
		.as_integer()
		.expect("TTL should return integer");
	assert!(hash_ttl > 0 && hash_ttl <= 300);
	assert_eq!(client.ttl(KeyType::String, key), -1);

	assert_eq!(client.del(KeyType::Hash, key), 1);
	assert!(!client.exists(KeyType::Hash, key));
	assert!(client.exists(KeyType::String, key));
}

#[test]
#[serial]
fn test_typed_key_commands_reject_legacy_and_invalid_forms() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();

	for command in [
		vec!["DEL", "key"],
		vec!["EXISTS", "key"],
		vec!["EXPIRE", "key", "10"],
		vec!["TTL", "key"],
	] {
		let name = command[0].to_lowercase();
		assert_eq!(
			resp_error(client.execute(&command)),
			format!("ERR wrong number of arguments for '{name}' command")
		);
	}

	for command in [
		vec!["DEL", "STREAM", "key"],
		vec!["EXISTS", "ALL", "key"],
		vec!["EXPIRE", "STR", "key", "10"],
		vec!["TTL", "ZSET_ALIAS", "key"],
	] {
		assert_eq!(
			resp_error(client.execute(&command)),
			"ERR invalid key type; expected STRING, HASH, LIST, SET, or ZSET"
		);
	}

	assert_eq!(
		resp_error(client.execute(&["EXPIRE", "STRING", "key", "-1"])),
		"ERR value is not an integer or out of range"
	);
	assert_eq!(
		resp_error(client.execute(&["EXPIRE", "STRING", "key", "18446744073709551615",])),
		"ERR value is not an integer or out of range"
	);
	assert_eq!(
		resp_error(client.execute(&["EXPIRE", "STRING", "key", "9223372036854775"])),
		"ERR value is not an integer or out of range"
	);
}

#[test]
#[serial]
fn test_client_command() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();

	assert!(client.id() > 0);
	assert_eq!(client.client_id(), client.id());

	assert_eq!(client.client_getname(), "");
	assert_eq!(client.client_setname("it-client"), "OK");
	assert_eq!(client.client_getname(), "it-client");

	let client_list = client.client_list();
	assert!(client_list.contains(&format!("id={}", client.id())));
	assert!(client_list.contains("name=it-client"));
}
