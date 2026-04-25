use opentelemetry_sdk::export::trace::{SpanData, SpanExporter};
use opentelemetry_sdk::error::OTelSdkResult;
use std::future::Future;
use std::time::Duration;

#[derive(Debug)]
pub struct ChunkedSpanExporter<T: SpanExporter> {
    inner: T,
    chunk_size: usize,
}

impl<T: SpanExporter> SpanExporter for ChunkedSpanExporter<T> {
    fn export(
        &self,
        mut batch: Vec<SpanData>,
    ) -> impl Future<Output = OTelSdkResult> + Send {
        async move {
            let mut result = Ok(());
            while !batch.is_empty() {
                let chunk_len = std::cmp::min(self.chunk_size, batch.len());
                let chunk: Vec<_> = batch.drain(..chunk_len).collect();
                if let Err(e) = self.inner.export(chunk).await {
                    result = Err(e);
                }
            }
            result
        }
    }
}
