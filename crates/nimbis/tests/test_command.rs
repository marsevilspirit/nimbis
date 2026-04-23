mod mock;

use mock::MockNimbisServer;
use mock::utils::resp_error;
use resp::RespValue;
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
	assert!(client.exists("it:flushdb:string"));
	assert!(client.exists("it:flushdb:hash"));

	assert!(client.flushdb());
	assert!(!client.exists("it:flushdb:string"));
	assert!(!client.exists("it:flushdb:hash"));
}

#[test]
#[serial]
fn test_del_and_exists() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();

	client.set("it:del:key", "hello");
	assert!(client.exists("it:del:key"));
	assert_eq!(client.del("it:del:key"), 1);
	assert!(!client.exists("it:del:key"));
	assert_eq!(client.del("it:del:key"), 0);
	assert_eq!(client.get("it:del:key"), "");
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
	assert_eq!(client.ttl("it:ttl:key"), -1);

	// set expiry
	assert!(client.expire("it:ttl:key", 300));
	let ttl = client.ttl("it:ttl:key");
	assert!(ttl > 0 && ttl <= 300);

	// expire non-existent key
	assert!(!client.expire("it:ttl:missing", 100));

	// ttl of non-existent key
	assert_eq!(client.ttl("it:ttl:missing"), -2);
}

#[test]
#[serial]
fn test_del_across_types() {
	let server = MockNimbisServer::new();
	let mut client = server.get_client();

	client.set("it:cross:str", "v");
	client.hset("it:cross:hash", "f", "v");
	client.rpush("it:cross:list", &["a"]);
	client.sadd("it:cross:set", &["m"]);
	client.zadd("it:cross:zset", &[("1", "z")]);

	assert_eq!(client.del("it:cross:str"), 1);
	assert_eq!(client.del("it:cross:hash"), 1);
	assert_eq!(client.del("it:cross:list"), 1);
	assert_eq!(client.del("it:cross:set"), 1);
	assert_eq!(client.del("it:cross:zset"), 1);

	assert!(!client.exists("it:cross:str"));
	assert!(!client.exists("it:cross:hash"));
	assert!(!client.exists("it:cross:list"));
	assert!(!client.exists("it:cross:set"));
	assert!(!client.exists("it:cross:zset"));
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
