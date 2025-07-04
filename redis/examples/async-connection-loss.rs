//! This example will connect to Redis in one of three modes:
//!
//! - Regular async connection
//! - Async multiplexed connection
//! - Async connection manager
//!
//! It will then send a PING every 100 ms and print the result.

use std::env;
use std::process;
use std::time::{Duration, Instant};

use futures::future;
use redis::aio::ConnectionLike;
use redis::RedisResult;
use tokio::time::interval;

enum Mode {
    Default,
    Reconnect,
}

async fn run_multi<C: ConnectionLike + Clone>(mut con: C) -> RedisResult<()> {
    let start = Instant::now();
    let mut interval = interval(Duration::from_millis(100));
    loop {
        // finish after two minutes
        if Instant::now().checked_duration_since(start).unwrap() > Duration::from_secs(120) {
            return Ok(());
        }
        interval.tick().await;
        println!();
        println!("> PING");
        println!("> PING");
        println!("> PING");
        let results: (
            RedisResult<String>,
            RedisResult<String>,
            RedisResult<String>,
        ) = future::join3(
            redis::cmd("PING").query_async(&mut con.clone()),
            redis::cmd("PING").query_async(&mut con.clone()),
            redis::cmd("PING").query_async(&mut con),
        )
        .await;
        println!("< {:?}", results.0);
        println!("< {:?}", results.1);
        println!("< {:?}", results.2);
    }
}

#[tokio::main]
async fn main() -> RedisResult<()> {
    let mode = match env::args().nth(1).as_deref() {
        Some("default") => {
            println!("Using default connection mode\n");
            Mode::Default
        }
        Some("reconnect") => {
            println!("Using reconnect manager mode\n");
            Mode::Reconnect
        }
        Some(_) | None => {
            println!("Usage: reconnect-manager (default|multiplexed|reconnect)");
            process::exit(1);
        }
    };

    let client = redis::Client::open("redis://127.0.0.1/").unwrap();
    match mode {
        Mode::Default => run_multi(client.get_multiplexed_async_connection().await?).await?,
        Mode::Reconnect => run_multi(client.get_connection_manager().await?).await?,
    };
    Ok(())
}
