use bytes::Bytes;
use nimbis_storage::segment::NimbisSegmentExtractor;
use nimbis_storage::segment::Segment;
use slatedb::PrefixExtractor;
use slatedb::PrefixTarget;

#[test]
fn segment_keys_are_one_byte_prefixes_over_existing_payloads() {
	let payload = Bytes::from_static(b"\x00\x04user\x00\x05field");
	let encoded = Segment::Hash.wrap(payload.clone());

	assert_eq!(encoded, Bytes::from_static(b"h\x00\x04user\x00\x05field"));
	assert_eq!(Segment::Hash.strip(&encoded), Some(payload));
	assert_eq!(Segment::Set.strip(&encoded), None);
}

#[test]
fn segment_extractor_routes_by_first_byte() {
	let extractor = NimbisSegmentExtractor;

	assert_eq!(extractor.name(), "nimbis-storage-segment-v1");
	assert_eq!(
		extractor.prefix_len(&PrefixTarget::Point(Bytes::from_static(b"h\x00\x04user"))),
		Some(1)
	);
	assert_eq!(
		extractor.prefix_len(&PrefixTarget::Prefix(Bytes::from_static(b"h"))),
		Some(1)
	);
	assert_eq!(
		extractor.prefix_len(&PrefixTarget::Prefix(Bytes::new())),
		None
	);
}
