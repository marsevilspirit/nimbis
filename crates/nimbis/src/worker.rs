use std::sync::Arc;
use std::thread;

use bytes::Bytes;
use resp::RespValue;
use storage::Storage;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use crate::cmd::CmdTable;

pub struct CmdRequest {
	pub(crate) cmd_name: String,
	pub(crate) args: Vec<Bytes>,
	pub(crate) resp_tx: oneshot::Sender<RespValue>,
}

pub struct Worker {
	pub(crate) tx: mpsc::UnboundedSender<CmdRequest>,
}

impl Worker {
	pub fn new(storage: Arc<Storage>, cmd_table: Arc<CmdTable>) -> Self {
		let (tx, mut rx) = mpsc::unbounded_channel::<CmdRequest>();

		thread::spawn(move || {
			let rt = tokio::runtime::Builder::new_current_thread()
				.enable_all()
				.build()
				.unwrap();

			rt.block_on(async move {
				while let Some(req) = rx.recv().await {
					let response = match cmd_table.get_cmd(&req.cmd_name) {
						Some(cmd) => cmd.execute(&storage, &req.args).await,
						None => RespValue::error(format!(
							"ERR unknown command '{}'",
							req.cmd_name.to_lowercase()
						)),
					};

					let _ = req.resp_tx.send(response);
				}
			});
		});

		Self { tx }
	}
}
