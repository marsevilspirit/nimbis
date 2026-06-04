use bytes::Bytes;
use nimbis_storage::hash::field_key::HashFieldKey;
use nimbis_storage::segment::HASH_PREFIX;
use nimbis_storage::segment::NimbisSegmentExtractor;
use nimbis_storage::segment::SET_PREFIX;
use nimbis_storage::set::member_key::SetMemberKey;
use slatedb::PrefixExtractor;
use slatedb::PrefixTarget;

#[test]
fn typed_keys_encode_segment_prefix_directly() {
	let hash_key =
		HashFieldKey::new(Bytes::from_static(b"user"), 7, Bytes::from_static(b"field")).encode();
	let set_key = SetMemberKey::new(
		Bytes::from_static(b"user"),
		7,
		Bytes::from_static(b"member"),
	)
	.encode();

	assert_eq!(hash_key[0], HASH_PREFIX);
	assert_eq!(set_key[0], SET_PREFIX);
	assert_eq!(&hash_key[1..3], &4u16.to_be_bytes());
	assert_eq!(&set_key[1..3], &4u16.to_be_bytes());
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
