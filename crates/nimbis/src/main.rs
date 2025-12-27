use bytes::BytesMut;
use nimbis::cmd::{Db, ParsedCmd};
use resp::{RespEncoder, RespValue, parse};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    telemetry::init();

    let addr = "127.0.0.1:6379";
    let listener = TcpListener::bind(addr).await?;
    info!("Nimbis server listening on {}", addr);

    let db: Db = Arc::new(RwLock::new(HashMap::new()));

    loop {
        let (socket, addr) = listener.accept().await?;
        info!("New client connected from {}", addr);
        let db = db.clone();

        tokio::spawn(async move {
            if let Err(e) = handle_client(socket, db).await {
                error!("Error handling client: {}", e);
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
                    let parsed_cmd: ParsedCmd = value.try_into()?;

                    // Log the command being executed
                    tracing::debug!(
                        command = %parsed_cmd.name,
                        args = ?parsed_cmd.args,
                        "Executing command"
                    );

                    // TODO: get cmd_table from member
                    let cmd_table = nimbis::cmd::get_cmd_table();
                    let response = match cmd_table.get(&parsed_cmd.name) {
                        Some(cmd) => cmd.execute(&db, &parsed_cmd.args).await,
                        None => RespValue::error(format!(
                            "ERR unknown command '{}'",
                            parsed_cmd.name.to_lowercase()
                        )),
                    };

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
