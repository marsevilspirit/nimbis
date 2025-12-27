use crate::cmd::{CmdTable, Db, ParsedCmd};
use bytes::BytesMut;
use resp::{RespEncoder, RespValue, parse};
use std::sync::Arc;
use storage::ObjectStorage;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{error, info};

pub struct Server {
    addr: String,
    db: Db,
    cmd_table: Arc<CmdTable>,
}

impl Server {
    /// Create a new server instance
    pub async fn new(
        addr: impl Into<String>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Ensure data directory exists
        let data_path = "./nimbis_data";
        std::fs::create_dir_all(data_path)?;

        // Open database
        let db = ObjectStorage::open(data_path).await?;
        Ok(Self {
            addr: addr.into(),
            db: Arc::new(db),
            cmd_table: Arc::new(CmdTable::new()),
        })
    }

    pub async fn run(self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let listener = TcpListener::bind(&self.addr).await?;
        info!("Nimbis server listening on {}", self.addr);

        loop {
            match listener.accept().await {
                Ok((socket, addr)) => {
                    info!("New client connected from {}", addr);
                    let db = self.db.clone();
                    let cmd_table = self.cmd_table.clone();

                    tokio::spawn(async move {
                        if let Err(e) = handle_client(socket, db, cmd_table).await {
                            error!("Error handling client: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Error accepting connection: {}", e);
                }
            }
        }
    }
}

async fn handle_client(
    mut socket: TcpStream,
    db: Db,
    cmd_table: Arc<CmdTable>,
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

                    let response = match cmd_table.get_cmd(&parsed_cmd.name) {
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
