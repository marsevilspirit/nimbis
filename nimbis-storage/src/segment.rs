use slatedb::PrefixExtractor;
use slatedb::PrefixTarget;

pub const EXTRACTOR_NAME: &str = "nimbis-storage-segment-v1";

pub const META_PREFIX: u8 = b'm';
pub const HASH_PREFIX: u8 = b'h';
pub const LIST_PREFIX: u8 = b'l';
pub const SET_PREFIX: u8 = b'S';
pub const ZSET_PREFIX: u8 = b'z';

#[derive(Debug, Default)]
pub struct NimbisSegmentExtractor;

impl PrefixExtractor for NimbisSegmentExtractor {
	fn name(&self) -> &str {
		EXTRACTOR_NAME
	}

	fn prefix_len(&self, target: &PrefixTarget) -> Option<usize> {
		let bytes = match target {
			PrefixTarget::Point(bytes) | PrefixTarget::Prefix(bytes) => bytes,
		};
		(!bytes.is_empty()).then_some(1)
	}
}
