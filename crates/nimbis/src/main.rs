use bytes::BytesMut;
use nimbis::cmd::{Cmd, Db};
use resp::{RespEncoder, RespValue, parse};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = "127.0.0.1:6379";
    let listener = TcpListener::bind(addr).await?;
    println!("Nimbis server listening on {}", addr);

    let db: Db = Arc::new(RwLock::new(HashMap::new()));

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

                    // 将 Cmd 转换为 CmdExecutor 并执行
                    let response = match cmd.into_executor() {
                        Ok(executor) => executor.execute(&db).await,
                        Err(e) => RespValue::error(e),
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
