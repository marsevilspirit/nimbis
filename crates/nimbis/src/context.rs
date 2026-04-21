use std::sync::Arc;
use std::sync::OnceLock;

use crate::client::ClientSessions;

pub struct GlobalContext {
	pub client_sessions: Arc<ClientSessions>,
}

impl GlobalContext {
	pub fn new(client_sessions: Arc<ClientSessions>) -> Self {
		Self { client_sessions }
	}
}

pub static GCTX: OnceLock<GlobalContext> = OnceLock::new();

pub fn init_global_context(client_sessions: Arc<ClientSessions>) {
	let _ = GCTX.set(GlobalContext::new(client_sessions));
}

#[macro_export]
macro_rules! GCTX {
	($field:ident) => {
		&$crate::context::GCTX
			.get()
			.expect("GlobalContext is not initialized")
			.$field
	};
}
