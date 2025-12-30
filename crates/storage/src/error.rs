use thiserror::Error;

#[derive(Debug, Error)]
pub enum DecoderError {
	#[error("Invalid prefix for StringKey")]
	InvalidPrefix,
	#[error("Empty key, cannot decode")]
	Empty,
}
