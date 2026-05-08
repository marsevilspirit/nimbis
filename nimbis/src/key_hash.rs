/// Simple FNV-1a 64-bit hash for key-based worker selection.
#[inline]
pub(crate) fn hash_key(key: &[u8]) -> u64 {
	let mut hasher: u64 = 0xcbf29ce484222325;
	for &byte in key {
		hasher ^= byte as u64;
		hasher = hasher.wrapping_mul(0x100000001b3);
	}
	hasher
}
