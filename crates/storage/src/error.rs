use thiserror::Error;

#[derive(Debug, Error)]
pub enum DecoderError {
	#[error("Empty key, cannot decode")]
	Empty,
	#[error("Invalid type code")]
	InvalidType,
	#[error("Invalid data length")]
	InvalidLength,
}
