use bytes::BytesMut;
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
        // Read data from socket directly into buffer
        let n = socket.read_buf(&mut buffer).await?;

        if n == 0 {
            // Connection closed
            if buffer.is_empty() {
                return Ok(());
            } else {
                return Err("Connection closed with incomplete data".into());
            }
        }

        // Try to parse and process all complete messages in buffer
        loop {
            // Try to parse a RESP value from the buffer
            // parse will automatically advance the buffer
            match parse(&mut buffer) {
                Ok(value) => {
                    // Process command and get response
                    let response = process_command(value, &db).await;

                    // Encode and send response
                    let encoded = response.encode()?;
                    socket.write_all(&encoded).await?;
                }
                Err(e) => {
                    // Check if it's just incomplete data or a real error
                    if buffer.is_empty() || matches!(e, resp::ParseError::UnexpectedEOF) {
                        // Incomplete message, need more data
                        break;
                    } else {
                        // Real parse error
                        let error_response = RespValue::error(format!("ERR Protocol error: {}", e));
                        let encoded = error_response.encode()?;
                        socket.write_all(&encoded).await?;
                        return Err(e.into());
                    }
                }
            }
        }
    }
}

async fn process_command(value: RespValue, db: &Db) -> RespValue {
    let args = match value.as_array() {
        Some(arr) => arr,
        None => {
            return RespValue::error("ERR expected array");
        }
    };

    if args.is_empty() {
        return RespValue::error("ERR empty command");
    }

    let command = match args[0].as_str() {
        Some(cmd) => cmd.to_uppercase(),
        None => {
            return RespValue::error("ERR invalid command");
        }
    };

    match command.as_str() {
        "SET" => {
            if args.len() != 3 {
                return RespValue::error("ERR wrong number of arguments for 'set' command");
            }

            let key = match args[1].as_str() {
                Some(k) => k.to_string(),
                None => return RespValue::error("ERR invalid key"),
            };

            let value = match args[2].as_str() {
                Some(v) => v.to_string(),
                None => return RespValue::error("ERR invalid value"),
            };

            let mut db = db.write().await;
            db.insert(key, value);

            RespValue::simple_string("OK")
        }
        "GET" => {
            if args.len() != 2 {
                return RespValue::error("ERR wrong number of arguments for 'get' command");
            }

            let key = match args[1].as_str() {
                Some(k) => k,
                None => return RespValue::error("ERR invalid key"),
            };

            let db = db.read().await;
            match db.get(key) {
                Some(value) => RespValue::bulk_string(value.clone()),
                None => RespValue::Null,
            }
        }
        _ => RespValue::error(format!("ERR unknown command '{}'", command)),
    }
}
