use bytes::BytesMut;
use nimbis_server::cmd::{Cmd, CmdType};
use resp::{RespEncoder, RespValue, parse};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;

type Db = Arc<RwLock<HashMap<String, String>>>;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = "127.0.0.1:6379";
    let listener = TcpListener::bind(addr).await?;
    println!("Nimbis server listening on {}", addr);

    let db = Arc::new(RwLock::new(HashMap::new()));

    loop {
        let (socket, _) = listener.accept().await?;
        let db = db.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_client(socket, db).await {
                eprintln!("Error handling client: {}", e);
            }
        });
    }
}

async fn handle_client(
    mut socket: TcpStream,
    db: Db,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut buffer = BytesMut::with_capacity(4096);

    loop {
        let n = socket.read_buf(&mut buffer).await?;

        if n == 0 {
            // Connection closed
            if buffer.is_empty() {
                return Ok(());
            } else {
                return Err("Connection closed with incomplete data".into());
            }
        }

        while !buffer.is_empty() {
            match parse(&mut buffer) {
                Ok(value) => {
                    let cmd: Cmd = value.try_into()?;
                    let response = process_command(cmd, &db).await;

                    let encoded = response.encode()?;
                    socket.write_all(&encoded).await?;
                }
                Err(e) => {
                    let error_response = RespValue::error(format!("ERR Protocol error: {}", e));
                    let encoded = error_response.encode()?;
                    socket.write_all(&encoded).await?;
                    return Err(e.into());
                }
            }
        }
    }
}

async fn process_command(cmd: Cmd, db: &Db) -> RespValue {
    match cmd.typ {
        CmdType::SET => {
            if cmd.args.len() != 2 {
                return RespValue::error("ERR wrong number of arguments for 'set' command");
            }

            let key = cmd.args[0].clone();
            let value = cmd.args[1].clone();

            let mut db = db.write().await;
            db.insert(key, value);

            RespValue::simple_string("OK")
        }
        CmdType::GET => {
            if cmd.args.len() != 1 {
                return RespValue::error("ERR wrong number of arguments for 'get' command");
            }

            let key = &cmd.args[0];

            let db = db.read().await;
            match db.get(key) {
                Some(value) => RespValue::bulk_string(value.clone()),
                None => RespValue::Null,
            }
        }
    }
}
