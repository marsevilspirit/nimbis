mod common;

use common::mock::MockNimbisClient;
use common::mock::MockNimbisServer;
use serial_test::serial;

fn connect_client() -> (MockNimbisServer, MockNimbisClient) {
	let server = MockNimbisServer::new();
	let client = server.connect_client();
	(server, client)
}

#[test]
#[serial]
fn test_string_command() {
	let (_server, mut nimbis) = connect_client();

	assert_eq!(nimbis.ping(), "PONG");
	assert_eq!(nimbis.set("it:string:key", "value-1"), "OK");
	assert_eq!(nimbis.get("it:string:key"), "value-1");
	assert_eq!(nimbis.set("it:string:key", "value-2"), "OK");
	assert_eq!(nimbis.get("it:string:key"), "value-2");
}

#[test]
#[serial]
fn test_del_and_exists() {
	let (_server, mut nimbis) = connect_client();

	nimbis.set("it:del:key", "hello");
	assert!(nimbis.exists("it:del:key"));
	assert_eq!(nimbis.del("it:del:key"), 1);
	assert!(!nimbis.exists("it:del:key"));
	assert_eq!(nimbis.del("it:del:key"), 0);
	assert_eq!(nimbis.get("it:del:key"), "");
}

#[test]
#[serial]
fn test_incr_decr() {
	let (_server, mut nimbis) = connect_client();

	assert_eq!(nimbis.incr("it:counter"), 1);
	assert_eq!(nimbis.incr("it:counter"), 2);
	assert_eq!(nimbis.incr("it:counter"), 3);
	assert_eq!(nimbis.decr("it:counter"), 2);
	assert_eq!(nimbis.decr("it:counter"), 1);
	assert_eq!(nimbis.decr("it:counter"), 0);
	assert_eq!(nimbis.decr("it:counter"), -1);
}

#[test]
#[serial]
fn test_append() {
	let (_server, mut nimbis) = connect_client();

	assert_eq!(nimbis.append("it:append:key", "hello"), 5);
	assert_eq!(nimbis.append("it:append:key", " world"), 11);
	assert_eq!(nimbis.get("it:append:key"), "hello world");
}

#[test]
#[serial]
fn test_hash_command() {
	let (_server, mut nimbis) = connect_client();

	assert_eq!(nimbis.hset("it:hash:user", "name", "alice"), 1);
	assert_eq!(nimbis.hset("it:hash:user", "age", "30"), 1);
	assert_eq!(nimbis.hget("it:hash:user", "name"), "alice");
	assert_eq!(nimbis.hget("it:hash:user", "age"), "30");
	assert_eq!(nimbis.hget("it:hash:user", "missing"), "");
	assert_eq!(nimbis.hlen("it:hash:user"), 2);

	// overwrite
	assert_eq!(nimbis.hset("it:hash:user", "name", "bob"), 0);
	assert_eq!(nimbis.hget("it:hash:user", "name"), "bob");

	// hdel
	assert_eq!(nimbis.hdel("it:hash:user", "age"), 1);
	assert_eq!(nimbis.hdel("it:hash:user", "age"), 0);
	assert_eq!(nimbis.hlen("it:hash:user"), 1);
}

#[test]
#[serial]
fn test_hmget() {
	let (_server, mut nimbis) = connect_client();

	nimbis.hset("it:hmget:h", "f1", "v1");
	nimbis.hset("it:hmget:h", "f2", "v2");
	nimbis.hset("it:hmget:h", "f3", "v3");

	let vals = nimbis.hmget("it:hmget:h", &["f1", "f3", "missing"]);
	assert_eq!(vals[0], "v1");
	assert_eq!(vals[1], "v3");
	assert_eq!(vals[2], "");
}

#[test]
#[serial]
fn test_hgetall() {
	let (_server, mut nimbis) = connect_client();

	nimbis.hset("it:hgetall:h", "k1", "v1");
	nimbis.hset("it:hgetall:h", "k2", "v2");

	let all = nimbis.hgetall("it:hgetall:h");
	assert_eq!(all.len(), 4); // [field, value, field, value]
	assert!(all.contains(&"k1".to_string()));
	assert!(all.contains(&"v1".to_string()));
	assert!(all.contains(&"k2".to_string()));
	assert!(all.contains(&"v2".to_string()));
}

#[test]
#[serial]
fn test_list_lpush_rpush() {
	let (_server, mut nimbis) = connect_client();

	assert_eq!(nimbis.rpush("it:list:q", &["a", "b"]), 2);
	assert_eq!(nimbis.lpush("it:list:q", &["z"]), 3);

	// order: z, a, b
	let items = nimbis.lrange("it:list:q", 0, -1);
	assert_eq!(items, vec!["z", "a", "b"]);
	assert_eq!(nimbis.llen("it:list:q"), 3);
}

#[test]
#[serial]
fn test_list_lpop_rpop() {
	let (_server, mut nimbis) = connect_client();

	nimbis.rpush("it:list:pop", &["1", "2", "3"]);

	assert_eq!(nimbis.lpop("it:list:pop"), "1");
	assert_eq!(nimbis.rpop("it:list:pop"), "3");
	assert_eq!(nimbis.llen("it:list:pop"), 1);
	assert_eq!(nimbis.lpop("it:list:pop"), "2");
	assert_eq!(nimbis.lpop("it:list:pop"), ""); // empty list
}

#[test]
#[serial]
fn test_lrange() {
	let (_server, mut nimbis) = connect_client();

	nimbis.rpush("it:list:range", &["a", "b", "c", "d", "e"]);

	assert_eq!(nimbis.lrange("it:list:range", 0, 2), vec!["a", "b", "c"]);
	assert_eq!(nimbis.lrange("it:list:range", -2, -1), vec!["d", "e"]);
	assert_eq!(nimbis.lrange("it:list:range", 1, 3), vec!["b", "c", "d"]);
}

#[test]
#[serial]
fn test_set_command() {
	let (_server, mut nimbis) = connect_client();

	assert_eq!(nimbis.sadd("it:set:s", &["a", "b", "c"]), 3);
	assert_eq!(nimbis.sadd("it:set:s", &["a"]), 0); // duplicate
	assert_eq!(nimbis.scard("it:set:s"), 3);

	assert!(nimbis.sismember("it:set:s", "a"));
	assert!(!nimbis.sismember("it:set:s", "x"));

	let members = nimbis.smembers("it:set:s");
	assert_eq!(members.len(), 3);
	assert!(members.contains(&"a".to_string()));
	assert!(members.contains(&"b".to_string()));
	assert!(members.contains(&"c".to_string()));
}

#[test]
#[serial]
fn test_srem() {
	let (_server, mut nimbis) = connect_client();

	nimbis.sadd("it:set:rem", &["x", "y", "z"]);

	assert_eq!(nimbis.srem("it:set:rem", &["x"]), 1);
	assert_eq!(nimbis.srem("it:set:rem", &["x"]), 0); // already removed
	assert_eq!(nimbis.scard("it:set:rem"), 2);
	assert!(!nimbis.sismember("it:set:rem", "x"));
}

#[test]
#[serial]
fn test_zset_command() {
	let (_server, mut nimbis) = connect_client();

	assert_eq!(
		nimbis.zadd(
			"it:zset:z",
			&[("1.0", "alice"), ("2.5", "bob"), ("1.5", "carol")]
		),
		3
	);
	assert_eq!(nimbis.zcard("it:zset:z"), 3);

	// zrange returns sorted by score
	let ranked = nimbis.zrange("it:zset:z", 0, -1);
	assert_eq!(ranked, vec!["alice", "carol", "bob"]);

	assert_eq!(nimbis.zscore("it:zset:z", "bob"), "2.5");
	assert_eq!(nimbis.zscore("it:zset:z", "missing"), "");
}

#[test]
#[serial]
fn test_zrem() {
	let (_server, mut nimbis) = connect_client();

	nimbis.zadd("it:zset:rem", &[("1", "a"), ("2", "b"), ("3", "c")]);

	assert_eq!(nimbis.zrem("it:zset:rem", &["b"]), 1);
	assert_eq!(nimbis.zrem("it:zset:rem", &["b"]), 0);
	assert_eq!(nimbis.zcard("it:zset:rem"), 2);

	let ranked = nimbis.zrange("it:zset:rem", 0, -1);
	assert_eq!(ranked, vec!["a", "c"]);
}

#[test]
#[serial]
fn test_expire_and_ttl() {
	let (_server, mut nimbis) = connect_client();

	nimbis.set("it:ttl:key", "temp");

	// no expiry set
	assert_eq!(nimbis.ttl("it:ttl:key"), -1);

	// set expiry
	assert!(nimbis.expire("it:ttl:key", 300));
	let ttl = nimbis.ttl("it:ttl:key");
	assert!(ttl > 0 && ttl <= 300);

	// expire non-existent key
	assert!(!nimbis.expire("it:ttl:missing", 100));

	// ttl of non-existent key
	assert_eq!(nimbis.ttl("it:ttl:missing"), -2);
}

#[test]
#[serial]
fn test_del_across_types() {
	let (_server, mut nimbis) = connect_client();

	nimbis.set("it:cross:str", "v");
	nimbis.hset("it:cross:hash", "f", "v");
	nimbis.rpush("it:cross:list", &["a"]);
	nimbis.sadd("it:cross:set", &["m"]);
	nimbis.zadd("it:cross:zset", &[("1", "z")]);

	assert_eq!(nimbis.del("it:cross:str"), 1);
	assert_eq!(nimbis.del("it:cross:hash"), 1);
	assert_eq!(nimbis.del("it:cross:list"), 1);
	assert_eq!(nimbis.del("it:cross:set"), 1);
	assert_eq!(nimbis.del("it:cross:zset"), 1);

	assert!(!nimbis.exists("it:cross:str"));
	assert!(!nimbis.exists("it:cross:hash"));
	assert!(!nimbis.exists("it:cross:list"));
	assert!(!nimbis.exists("it:cross:set"));
	assert!(!nimbis.exists("it:cross:zset"));
}

#[test]
#[serial]
fn test_client_command() {
	let (_server, mut nimbis) = connect_client();

	let client_id = nimbis.client_id();
	assert!(client_id > 0);

	assert_eq!(nimbis.client_getname(), "");
	assert_eq!(nimbis.client_setname("it-client"), "OK");
	assert_eq!(nimbis.client_getname(), "it-client");

	let client_list = nimbis.client_list();
	assert!(client_list.contains(&format!("id={}", client_id)));
	assert!(client_list.contains("name=it-client"));
}
