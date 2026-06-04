use bytes::Bytes;
use bytes::BytesMut;
use slatedb::PrefixExtractor;
use slatedb::PrefixTarget;

pub const EXTRACTOR_NAME: &str = "nimbis-storage-segment-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Segment {
	Internal = 0,
	Meta = b'm',
	Hash = b'h',
	List = b'l',
	Set = b'S',
	ZSet = b'z',
}

impl Segment {
	pub fn prefix(self) -> u8 {
		self as u8
	}

	pub fn wrap(self, payload: Bytes) -> Bytes {
		let mut key = BytesMut::with_capacity(1 + payload.len());
		key.extend_from_slice(&[self.prefix()]);
		key.extend_from_slice(&payload);
		key.freeze()
	}

	pub fn strip(self, encoded: &Bytes) -> Option<Bytes> {
		if encoded.first().copied()? != self.prefix() {
			return None;
		}
		Some(encoded.slice(1..))
	}
}

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
