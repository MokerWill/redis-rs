#![cfg(feature = "streams")]

use redis::streams::*;
use redis::{Connection, ToRedisArgs, TypedCommands};

mod support;
use crate::support::*;

use std::collections::BTreeMap;
use std::str;
use std::thread::sleep;
use std::time::Duration;

fn xadd(con: &mut Connection) {
    con.xadd("k1", "1000-0", &[("hello", "world"), ("redis", "streams")])
        .unwrap();
    con.xadd("k1", "1000-1", &[("hello", "world2")]).unwrap();
    con.xadd("k2", "2000-0", &[("hello", "world")]).unwrap();
    con.xadd("k2", "2000-1", &[("hello", "world2")]).unwrap();
}

fn xadd_keyrange(con: &mut Connection, key: &str, start: i32, end: i32) {
    for _i in start..end {
        con.xadd(key, "*", &[("h", "w")]).unwrap();
    }
}

#[test]
fn test_cmd_options() {
    // Tests the following command option builders....
    // xclaim_options
    // xread_options
    // maxlen enum

    // test read options

    let empty = StreamClaimOptions::default();
    assert_eq!(ToRedisArgs::to_redis_args(&empty).len(), 0);

    let empty = StreamReadOptions::default();
    assert_eq!(ToRedisArgs::to_redis_args(&empty).len(), 0);

    let opts = StreamClaimOptions::default()
        .idle(50)
        .time(500)
        .retry(3)
        .with_force()
        .with_justid();

    assert_args!(
        &opts,
        "IDLE",
        "50",
        "TIME",
        "500",
        "RETRYCOUNT",
        "3",
        "FORCE",
        "JUSTID"
    );

    // test maxlen options

    assert_args!(StreamMaxlen::Approx(10), "MAXLEN", "~", "10");
    assert_args!(StreamMaxlen::Equals(10), "MAXLEN", "=", "10");

    // test read options

    let opts = StreamReadOptions::default()
        .noack()
        .block(100)
        .count(200)
        .group("group-name", "consumer-name");

    assert_args!(
        &opts,
        "GROUP",
        "group-name",
        "consumer-name",
        "BLOCK",
        "100",
        "COUNT",
        "200",
        "NOACK"
    );

    // should skip noack because of missing group(,)
    let opts = StreamReadOptions::default().noack().block(100).count(200);

    assert_args!(&opts, "BLOCK", "100", "COUNT", "200");
}

#[test]
fn test_assorted_1() {
    // Tests the following commands....
    // xadd
    // xadd_map (skip this for now)
    // xadd_maxlen
    // xread
    // xlen

    let ctx = TestContext::new();
    let mut con = ctx.connection();

    xadd(&mut con);

    // smoke test that we get the same id back
    let result = con.xadd("k0", "1000-0", &[("x", "y")]).unwrap();
    assert_eq!(result.unwrap(), "1000-0");

    // xread reply
    let reply = con
        .xread(&["k1", "k2", "k3"], &["0", "0", "0"])
        .unwrap()
        .unwrap();

    // verify reply contains 2 keys even though we asked for 3
    assert_eq!(&reply.keys.len(), &2usize);

    // verify first key & first id exist
    assert_eq!(&reply.keys[0].key, "k1");
    assert_eq!(&reply.keys[0].ids.len(), &2usize);
    assert_eq!(&reply.keys[0].ids[0].id, "1000-0");

    // lookup the key in StreamId map
    let hello = reply.keys[0].ids[0].get("hello");
    assert_eq!(hello, Some("world".to_string()));

    // verify the second key was written
    assert_eq!(&reply.keys[1].key, "k2");
    assert_eq!(&reply.keys[1].ids.len(), &2usize);
    assert_eq!(&reply.keys[1].ids[0].id, "2000-0");

    // test xadd_map
    let mut map = BTreeMap::new();
    map.insert("ab", "cd");
    map.insert("ef", "gh");
    map.insert("ij", "kl");
    con.xadd_map("k3", "3000-0", map).unwrap();

    let reply = con.xrange_all("k3").unwrap();
    assert!(reply.ids[0].contains_key("ab"));
    assert!(reply.ids[0].contains_key("ef"));
    assert!(reply.ids[0].contains_key("ij"));

    // test xadd w/ maxlength below...

    // add 100 things to k4
    xadd_keyrange(&mut con, "k4", 0, 100);

    // test xlen.. should have 100 items
    let result = con.xlen("k4");
    assert_eq!(result, Ok(100));

    // test xadd_maxlen

    con.xadd_maxlen("k4", StreamMaxlen::Equals(10), "*", &[("h", "w")])
        .unwrap();
    let result = con.xlen("k4");
    assert_eq!(result, Ok(10));
}

#[test]
fn test_xgroup_create() {
    // Tests the following commands....
    // xadd
    // xinfo_stream
    // xgroup_create
    // xinfo_groups

    let ctx = TestContext::new();
    let mut con = ctx.connection();

    xadd(&mut con);

    // no key exists... this call breaks the connection pipe for some reason
    let reply = con.xinfo_stream("k10");
    assert!(
        matches!(&reply, Err(e) if e.kind() == redis::ErrorKind::ResponseError
            && e.code() == Some("ERR")
            && e.detail() == Some("no such key"))
    );

    // redo the connection because the above error
    con = ctx.connection();

    // key should exist
    let reply = con.xinfo_stream("k1").unwrap();
    assert_eq!(&reply.first_entry.id, "1000-0");
    assert_eq!(&reply.last_entry.id, "1000-1");
    assert_eq!(&reply.last_generated_id, "1000-1");

    // xgroup create (existing stream)
    let result = con.xgroup_create("k1", "g1", "$");
    assert!(result.is_ok());

    // xinfo groups (existing stream)
    let result = con.xinfo_groups("k1");
    assert!(result.is_ok());
    let reply = result.unwrap();
    assert_eq!(&reply.groups.len(), &1);
    assert_eq!(&reply.groups[0].name, &"g1");
}

#[test]
fn test_xgroup_createconsumer() {
    // Tests the following command....
    // xgroup_createconsumer

    let ctx = TestContext::new();
    let mut con = ctx.connection();

    xadd(&mut con);

    // key should exist
    let reply = con.xinfo_stream("k1").unwrap();
    assert_eq!(&reply.first_entry.id, "1000-0");
    assert_eq!(&reply.last_entry.id, "1000-1");
    assert_eq!(&reply.last_generated_id, "1000-1");

    // xgroup create (existing stream)
    let result = con.xgroup_create("k1", "g1", "$");
    assert!(result.is_ok());

    // xinfo groups (existing stream)
    let result = con.xinfo_groups("k1");
    assert!(result.is_ok());
    let reply = result.unwrap();
    assert_eq!(&reply.groups.len(), &1);
    assert_eq!(&reply.groups[0].name, &"g1");

    // xinfo consumers (consumer does not exist)
    let result = con.xinfo_consumers("k1", "g1");
    assert!(result.is_ok());
    let reply = result.unwrap();
    assert_eq!(&reply.consumers.len(), &0);

    // xgroup_createconsumer
    let result = con.xgroup_createconsumer("k1", "g1", "c1");
    assert!(matches!(result, Ok(true)));

    // xinfo consumers (consumer was created)
    let result = con.xinfo_consumers("k1", "g1");
    assert!(result.is_ok());
    let reply = result.unwrap();
    assert_eq!(&reply.consumers.len(), &1);
    assert_eq!(&reply.consumers[0].name, &"c1");

    // second call will not create consumer
    let result = con.xgroup_createconsumer("k1", "g1", "c1");
    assert!(matches!(result, Ok(false)));

    // xinfo consumers (consumer still exists)
    let result = con.xinfo_consumers("k1", "g1");
    assert!(result.is_ok());
    let reply = result.unwrap();
    assert_eq!(&reply.consumers.len(), &1);
    assert_eq!(&reply.consumers[0].name, &"c1");
}

#[test]
fn test_assorted_2() {
    // Tests the following commands....
    // xadd
    // xinfo_stream
    // xinfo_groups
    // xinfo_consumer
    // xgroup_create_mkstream
    // xread_options
    // xack
    // xpending
    // xpending_count
    // xpending_consumer_count

    let ctx = TestContext::new();
    let mut con = ctx.connection();

    xadd(&mut con);

    // test xgroup create w/ mkstream @ 0
    let result = con.xgroup_create_mkstream("k99", "g99", "0");
    assert!(result.is_ok());

    // Since nothing exists on this stream yet,
    // it should have the defaults returned by the client
    let result = con.xinfo_groups("k99");
    assert!(result.is_ok());
    let reply = result.unwrap();
    assert_eq!(&reply.groups.len(), &1);
    assert_eq!(&reply.groups[0].name, &"g99");
    assert_eq!(&reply.groups[0].last_delivered_id, &"0-0");
    if let Some(lag) = reply.groups[0].lag {
        assert_eq!(lag, 0);
    }

    // call xadd on k99 just so we can read from it
    // using consumer g99 and test xinfo_consumers
    let _ = con.xadd("k99", "1000-0", &[("a", "b"), ("c", "d")]);
    let _ = con.xadd("k99", "1000-1", &[("e", "f"), ("g", "h")]);

    // Two messages have been added but not acked:
    // this should give us a `lag` of 2 (if the server supports it)
    let result = con.xinfo_groups("k99");
    assert!(result.is_ok());
    let reply = result.unwrap();
    assert_eq!(&reply.groups.len(), &1);
    assert_eq!(&reply.groups[0].name, &"g99");
    if let Some(lag) = reply.groups[0].lag {
        assert_eq!(lag, 2);
    }

    // test empty PEL
    let empty_reply = con.xpending("k99", "g99").unwrap();

    assert_eq!(empty_reply.count(), 0);
    if let StreamPendingReply::Empty = empty_reply {
        // looks good
    } else {
        panic!("Expected StreamPendingReply::Empty but got Data");
    }

    // passing options  w/ group triggers XREADGROUP
    // using ID=">" means all undelivered ids
    // otherwise, ID="0 | ms-num" means all pending already
    // sent to this client
    let reply = con
        .xread_options(
            &["k99"],
            &[">"],
            &StreamReadOptions::default().group("g99", "c99"),
        )
        .unwrap()
        .unwrap();
    assert_eq!(reply.keys[0].ids.len(), 2);

    // read xinfo consumers again, should have 2 messages for the c99 consumer
    let reply = con.xinfo_consumers("k99", "g99").unwrap();
    assert_eq!(reply.consumers[0].pending, 2);

    // ack one of these messages
    let result = con.xack("k99", "g99", &["1000-0"]);
    assert_eq!(result, Ok(1));

    // get pending messages already seen by this client
    // we should only have one now..
    let reply = con
        .xread_options(
            &["k99"],
            &["0"],
            &StreamReadOptions::default().group("g99", "c99"),
        )
        .unwrap()
        .unwrap();
    assert_eq!(reply.keys.len(), 1);

    // we should also have one pending here...
    let reply = con.xinfo_consumers("k99", "g99").unwrap();
    assert_eq!(reply.consumers[0].pending, 1);

    // add more and read so we can test xpending
    let _ = con.xadd("k99", "1001-0", &[("i", "j"), ("k", "l")]);
    let _ = con.xadd("k99", "1001-1", &[("m", "n"), ("o", "p")]);
    let _ = con
        .xread_options(
            &["k99"],
            &[">"],
            &StreamReadOptions::default().group("g99", "c99"),
        )
        .unwrap();

    // call xpending here...
    // this has a different reply from what the count variations return
    let data_reply = con.xpending("k99", "g99").unwrap();

    assert_eq!(data_reply.count(), 3);

    if let StreamPendingReply::Data(data) = data_reply {
        assert_stream_pending_data(data)
    } else {
        panic!("Expected StreamPendingReply::Data but got Empty");
    }

    // both count variations have the same reply types
    let reply = con.xpending_count("k99", "g99", "-", "+", 10).unwrap();
    assert_eq!(reply.ids.len(), 3);

    let reply = con
        .xpending_consumer_count("k99", "g99", "-", "+", 10, "c99")
        .unwrap();
    assert_eq!(reply.ids.len(), 3);

    for StreamPendingId {
        id,
        consumer,
        times_delivered,
        last_delivered_ms: _,
    } in reply.ids
    {
        assert!(!id.is_empty());
        assert!(!consumer.is_empty());
        assert!(times_delivered > 0);
    }
}

fn assert_stream_pending_data(data: StreamPendingData) {
    assert_eq!(data.start_id, "1000-1");
    assert_eq!(data.end_id, "1001-1");
    assert_eq!(data.consumers.len(), 1);
    assert_eq!(data.consumers[0].name, "c99");
}

#[test]
fn test_xadd_maxlen_map() {
    let ctx = TestContext::new();
    let mut con = ctx.connection();

    for i in 0..10 {
        let mut map = BTreeMap::new();
        let idx = i.to_string();
        map.insert("idx", &idx);
        let _ = con.xadd_maxlen_map("maxlen_map", StreamMaxlen::Equals(3), "*", map);
    }

    let result = con.xlen("maxlen_map");
    assert_eq!(result, Ok(3));
    let reply = con.xrange_all("maxlen_map").unwrap();

    assert_eq!(reply.ids[0].get("idx"), Some("7".to_string()));
    assert_eq!(reply.ids[1].get("idx"), Some("8".to_string()));
    assert_eq!(reply.ids[2].get("idx"), Some("9".to_string()));
}

#[test]
fn test_xadd_options() {
    let ctx = TestContext::new();
    let mut con = ctx.connection();

    // NoMKStream will return a nil when the stream does not exist
    let result = con.xadd_options(
        "k1",
        "*",
        &[("h", "w")],
        &StreamAddOptions::default().nomkstream(),
    );
    assert_eq!(result, Ok(None));

    let result = con.xinfo_stream("k1");
    assert!(
        matches!(&result, Err(e) if e.kind() == redis::ErrorKind::ResponseError
            && e.code() == Some("ERR")
            && e.detail() == Some("no such key"))
    );

    fn setup_stream(con: &mut Connection) {
        let _ = con.del("k1");

        for i in 0..10 {
            let _ = con.xadd_options(
                "k1",
                format!("1-{i}"),
                &[("h", "w")],
                &StreamAddOptions::default(),
            );
        }
    }

    // test trim by maxlen
    setup_stream(&mut con);

    let _ = con.xadd_options(
        "k1",
        "2-1",
        &[("h", "w")],
        &StreamAddOptions::default().trim(StreamTrimStrategy::maxlen(StreamTrimmingMode::Exact, 4)),
    );

    let info = con.xinfo_stream("k1").unwrap();
    assert_eq!(info.length, 4);
    assert_eq!(info.first_entry.id, "1-7");

    // test with trim by minid
    setup_stream(&mut con);

    let _ = con.xadd_options(
        "k1",
        "2-1",
        &[("h", "w")],
        &StreamAddOptions::default()
            .trim(StreamTrimStrategy::minid(StreamTrimmingMode::Exact, "1-5")),
    );
    let info = con.xinfo_stream("k1").unwrap();
    assert_eq!(info.length, 6);
    assert_eq!(info.first_entry.id, "1-5");

    // test adding from a map
    let mut map = BTreeMap::new();
    map.insert("ab", "cd");
    map.insert("ef", "gh");
    map.insert("ij", "kl");
    let _ = con.xadd_options("k1", "3-1", map, &StreamAddOptions::default());

    let info = con.xinfo_stream("k1").unwrap();
    assert_eq!(info.length, 7);
    assert_eq!(info.first_entry.id, "1-5");
    assert_eq!(info.last_entry.id, "3-1");
}

#[test]
fn test_xread_options_deleted_pel_entry() {
    // Test xread_options behaviour with deleted entry
    let ctx = TestContext::new();
    let mut con = ctx.connection();
    let result = con.xgroup_create_mkstream("k1", "g1", "$");
    assert!(result.is_ok());
    let _ = con.xadd_maxlen("k1", StreamMaxlen::Equals(1), "*", &[("h1", "w1")]);
    // read the pending items for this key & group
    let result = con
        .xread_options(
            &["k1"],
            &[">"],
            &StreamReadOptions::default().group("g1", "c1"),
        )
        .unwrap()
        .unwrap();

    let _ = con.xadd_maxlen("k1", StreamMaxlen::Equals(1), "*", &[("h2", "w2")]);
    let result_deleted_entry = con
        .xread_options(
            &["k1"],
            &["0"],
            &StreamReadOptions::default().group("g1", "c1"),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        result.keys[0].ids.len(),
        result_deleted_entry.keys[0].ids.len()
    );
    assert_eq!(
        result.keys[0].ids[0].id,
        result_deleted_entry.keys[0].ids[0].id
    );
}

#[test]
fn test_xautoclaim() {
    // Tests the following command....
    // xautoclaim_options
    let ctx = TestContext::new();
    let mut con = ctx.connection();

    // xautoclaim test basic idea:
    // 1. we need to test adding messages to a group
    // 2. then xreadgroup needs to define a consumer and read pending
    //    messages without acking them
    // 3. then we need to sleep 5ms and call xautoclaim to claim message
    //    past the idle time and read them from a different consumer

    // create the group
    let result = con.xgroup_create_mkstream("k1", "g1", "$");
    assert!(result.is_ok());

    // add some keys
    xadd_keyrange(&mut con, "k1", 0, 10);

    // read the pending items for this key & group
    let reply = con
        .xread_options(
            &["k1"],
            &[">"],
            &StreamReadOptions::default().group("g1", "c1"),
        )
        .unwrap()
        .unwrap();
    // verify we have 10 ids
    assert_eq!(reply.keys[0].ids.len(), 10);

    // save this StreamId for later
    let claim = &reply.keys[0].ids[0];
    let claim_1 = &reply.keys[0].ids[1];

    // sleep for 5ms
    sleep(Duration::from_millis(10));

    // grab this id if > 4ms
    let reply = con
        .xautoclaim_options(
            "k1",
            "g1",
            "c2",
            4,
            claim.id.clone(),
            StreamAutoClaimOptions::default().count(2),
        )
        .unwrap();
    assert_eq!(reply.claimed.len(), 2);
    assert_eq!(reply.claimed[0].id, claim.id);
    assert!(!reply.claimed[0].map.is_empty());
    assert_eq!(reply.claimed[1].id, claim_1.id);
    assert!(!reply.claimed[1].map.is_empty());

    // sleep for 5ms
    sleep(Duration::from_millis(5));

    // let's test some of the xautoclaim_options
    // call force on the same claim.id
    let reply = con
        .xautoclaim_options(
            "k1",
            "g1",
            "c3",
            4,
            claim.id.clone(),
            StreamAutoClaimOptions::default().count(5).with_justid(),
        )
        .unwrap();

    // we just claimed the first original 5 ids
    // and only returned the ids
    assert_eq!(reply.claimed.len(), 5);
    assert_eq!(reply.claimed[0].id, claim.id);
    assert!(reply.claimed[0].map.is_empty());
    assert_eq!(reply.claimed[1].id, claim_1.id);
    assert!(reply.claimed[1].map.is_empty());
}

#[test]
fn test_xclaim() {
    // Tests the following commands....
    // xclaim
    // xclaim_options
    let ctx = TestContext::new();
    let mut con = ctx.connection();

    // xclaim test basic idea:
    // 1. we need to test adding messages to a group
    // 2. then xreadgroup needs to define a consumer and read pending
    //    messages without acking them
    // 3. then we need to sleep 5ms and call xpending
    // 4. from here we should be able to claim message
    //    past the idle time and read them from a different consumer

    // create the group
    let result = con.xgroup_create_mkstream("k1", "g1", "$");
    assert!(result.is_ok());

    // add some keys
    xadd_keyrange(&mut con, "k1", 0, 10);

    // read the pending items for this key & group
    let reply = con
        .xread_options(
            &["k1"],
            &[">"],
            &StreamReadOptions::default().group("g1", "c1"),
        )
        .unwrap()
        .unwrap();
    // verify we have 10 ids
    assert_eq!(reply.keys[0].ids.len(), 10);

    // save this StreamId for later
    let claim = &reply.keys[0].ids[0];
    let claim_justids = &reply.keys[0]
        .ids
        .iter()
        .map(|msg| &msg.id)
        .collect::<Vec<&String>>();

    // sleep for 5ms
    sleep(Duration::from_millis(5));

    // grab this id if > 4ms
    let reply = con
        .xclaim("k1", "g1", "c2", 4, &[claim.id.clone()])
        .unwrap();
    assert_eq!(reply.ids.len(), 1);
    assert_eq!(reply.ids[0].id, claim.id);

    // grab all pending ids for this key...
    // we should 9 in c1 and 1 in c2
    let reply = con.xpending("k1", "g1").unwrap();
    if let StreamPendingReply::Data(data) = reply {
        assert_eq!(data.consumers[0].name, "c1");
        assert_eq!(data.consumers[0].pending, 9);
        assert_eq!(data.consumers[1].name, "c2");
        assert_eq!(data.consumers[1].pending, 1);
    }

    // sleep for 5ms
    sleep(Duration::from_millis(5));

    // lets test some of the xclaim_options
    // call force on the same claim.id
    let _: StreamClaimReply = con
        .xclaim_options(
            "k1",
            "g1",
            "c3",
            4,
            &[claim.id.clone()],
            StreamClaimOptions::default().with_force(),
        )
        .unwrap();

    let reply = con.xpending("k1", "g1").unwrap();
    // we should have 9 w/ c1 and 1 w/ c3 now
    if let StreamPendingReply::Data(data) = reply {
        assert_eq!(data.consumers[1].name, "c3");
        assert_eq!(data.consumers[1].pending, 1);
    }

    // sleep for 5ms
    sleep(Duration::from_millis(5));

    // claim and only return JUSTID
    let claimed: Vec<String> = con
        .xclaim_options(
            "k1",
            "g1",
            "c5",
            4,
            claim_justids,
            StreamClaimOptions::default().with_force().with_justid(),
        )
        .unwrap();
    // we just claimed the original 10 ids
    // and only returned the ids
    assert_eq!(claimed.len(), 10);
}

#[test]
fn test_xclaim_last_id() {
    let ctx = TestContext::new();
    let mut con = ctx.connection();

    let result = con.xgroup_create_mkstream("k1", "g1", "$");
    assert!(result.is_ok());

    // add some keys
    xadd_keyrange(&mut con, "k1", 0, 10);

    let reply = con
        .xread_options(&["k1"], &["0"], &StreamReadOptions::default())
        .unwrap()
        .unwrap();
    // verify we have 10 ids
    assert_eq!(reply.keys[0].ids.len(), 10);

    let claim_early_id = &reply.keys[0].ids[3];
    let claim_middle_id = &reply.keys[0].ids[5];
    let claim_late_id = &reply.keys[0].ids[8];

    // get read up to the middle record
    let _ = con
        .xread_options(
            &["k1"],
            &[">"],
            &StreamReadOptions::default().count(6).group("g1", "c1"),
        )
        .unwrap();

    let info = con.xinfo_groups("k1").unwrap();
    assert_eq!(info.groups[0].last_delivered_id, claim_middle_id.id.clone());

    // sleep for 5ms
    sleep(Duration::from_millis(5));

    let _: Vec<String> = con
        .xclaim_options(
            "k1",
            "g1",
            "c2",
            4,
            &[claim_middle_id.id.clone()],
            StreamClaimOptions::default()
                .with_justid()
                .with_lastid(claim_early_id.id.as_str()),
        )
        .unwrap();

    // lastid is kept at the 6th entry as the 4th entry is OLDER than the last_delivered_id
    let info = con.xinfo_groups("k1").unwrap();
    assert_eq!(info.groups[0].last_delivered_id, claim_middle_id.id.clone());

    // sleep for 5ms
    sleep(Duration::from_millis(5));

    let _: Vec<String> = con
        .xclaim_options(
            "k1",
            "g1",
            "c1",
            4,
            &[claim_middle_id.id.clone()],
            StreamClaimOptions::default()
                .with_justid()
                .with_lastid(claim_late_id.id.as_str()),
        )
        .unwrap();

    // lastid is moved to the 8th entry as it is NEWER than the last_delivered_id
    let info = con.xinfo_groups("k1").unwrap();
    assert_eq!(info.groups[0].last_delivered_id, claim_late_id.id.clone());
}

#[test]
fn test_xdel() {
    // Tests the following commands....
    // xdel
    let ctx = TestContext::new();
    let mut con = ctx.connection();

    // add some keys
    xadd(&mut con);

    // delete the first stream item for this key
    let result = con.xdel("k1", &["1000-0"]);
    // returns the number of items deleted
    assert_eq!(result, Ok(1));

    let result = con.xdel("k2", &["2000-0", "2000-1", "2000-2"]);
    // should equal 2 since the last id doesn't exist
    assert_eq!(result, Ok(2));
}

#[test]
fn test_xtrim() {
    // Tests the following commands....
    // xtrim
    let ctx = TestContext::new();
    let mut con = ctx.connection();

    // add some keys
    xadd_keyrange(&mut con, "k1", 0, 100);

    // trim key to 50
    // returns the number of items remaining in the stream
    let result = con.xtrim("k1", StreamMaxlen::Equals(50));
    assert_eq!(result, Ok(50));
    // we should end up with 40 after this call
    let result = con.xtrim("k1", StreamMaxlen::Equals(10));
    assert_eq!(result, Ok(40));
}

#[test]
fn test_xtrim_options() {
    // Tests the following commands....
    // xtrim_options
    let ctx = TestContext::new();
    let mut con = ctx.connection();

    // add some keys
    xadd_keyrange(&mut con, "k1", 0, 100);

    // trim key to 50
    // returns the number of items deleted from the stream
    let result = con.xtrim_options(
        "k1",
        &StreamTrimOptions::maxlen(StreamTrimmingMode::Exact, 50),
    );
    assert_eq!(result, Ok(50));

    // we should end up with 40 deleted this call
    let result = con.xtrim_options(
        "k1",
        &StreamTrimOptions::maxlen(StreamTrimmingMode::Exact, 10),
    );
    assert_eq!(result, Ok(40));

    let _ = con.del("k1");

    for i in 1..100 {
        let _ = con.xadd("k1", format!("1-{i}"), &[("h", "w")]);
    }

    // Trim to id 1-26
    let result = con.xtrim_options(
        "k1",
        &StreamTrimOptions::minid(StreamTrimmingMode::Exact, "1-26"),
    );
    assert_eq!(result, Ok(25));

    // we should end up with 50 deleted this call
    let result = con.xtrim_options(
        "k1",
        &StreamTrimOptions::minid(StreamTrimmingMode::Exact, "1-76"),
    );
    assert_eq!(result, Ok(50));
}

#[test]
fn test_xgroup() {
    // Tests the following commands....
    // xgroup_create_mkstream
    // xgroup_destroy
    // xgroup_delconsumer

    let ctx = TestContext::new();
    let mut con = ctx.connection();

    // test xgroup create w/ mkstream @ 0
    let result = con.xgroup_create_mkstream("k1", "g1", "0");
    assert!(result.is_ok());

    // destroy this new stream group
    let result = con.xgroup_destroy("k1", "g1");
    assert_eq!(result, Ok(true));

    // add some keys
    xadd(&mut con);

    // create the group again using an existing stream
    let result = con.xgroup_create("k1", "g1", "0");
    assert!(result.is_ok());

    // read from the group so we can register the consumer
    let reply = con
        .xread_options(
            &["k1"],
            &[">"],
            &StreamReadOptions::default().group("g1", "c1"),
        )
        .unwrap()
        .unwrap();
    assert_eq!(reply.keys[0].ids.len(), 2);

    let result = con.xgroup_delconsumer("k1", "g1", "c1");
    // returns the number of pending message this client had open
    assert_eq!(result, Ok(2));

    let result = con.xgroup_destroy("k1", "g1");
    assert_eq!(result, Ok(true));
}

#[test]
fn test_xrange() {
    // Tests the following commands....
    // xrange (-/+ variations)
    // xrange_all
    // xrange_count

    let ctx = TestContext::new();
    let mut con = ctx.connection();

    xadd(&mut con);

    // xrange replies
    let reply = con.xrange_all("k1").unwrap();
    assert_eq!(reply.ids.len(), 2);

    let reply = con.xrange("k1", "1000-1", "+").unwrap();
    assert_eq!(reply.ids.len(), 1);

    let reply = con.xrange("k1", "-", "1000-0").unwrap();
    assert_eq!(reply.ids.len(), 1);

    let reply = con.xrange_count("k1", "-", "+", 1).unwrap();
    assert_eq!(reply.ids.len(), 1);
}

#[test]
fn test_xrevrange() {
    // Tests the following commands....
    // xrevrange (+/- variations)
    // xrevrange_all
    // xrevrange_count

    let ctx = TestContext::new();
    let mut con = ctx.connection();

    xadd(&mut con);

    // xrange replies
    let reply = con.xrevrange_all("k1").unwrap();
    assert_eq!(reply.ids.len(), 2);

    let reply = con.xrevrange("k1", "1000-1", "-").unwrap();
    assert_eq!(reply.ids.len(), 2);

    let reply = con.xrevrange("k1", "+", "1000-1").unwrap();
    assert_eq!(reply.ids.len(), 1);

    let reply = con.xrevrange_count("k1", "+", "-", 1).unwrap();
    assert_eq!(reply.ids.len(), 1);
}
