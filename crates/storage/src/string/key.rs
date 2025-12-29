use bytes::Bytes;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DecoderError {
    #[error("Invalid prefix for StringKey")]
    InvalidPrefix,
    #[error("Empty key, cannot decode")]
    Empty,
}

#[derive(Debug, PartialEq)]
pub struct StringKey {
    user_key: Bytes,
}

impl StringKey {
    pub fn new(user_key: impl Into<Bytes>) -> Self {
        Self {
            user_key: user_key.into(),
        }
    }

    pub fn encode(&self) -> Bytes {
        let mut bytes = Vec::with_capacity(1 + self.user_key.len());
        bytes.push(b's');
        bytes.extend_from_slice(&self.user_key);
        Bytes::from(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, DecoderError> {
        if bytes.is_empty() {
            return Err(DecoderError::Empty);
        }
        if bytes[0] != b's' {
            return Err(DecoderError::InvalidPrefix);
        }
        Ok(Self::new(Bytes::copy_from_slice(&bytes[1..])))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("mykey", b"smykey")]
    #[case("", b"s")]
    #[case("something else", b"ssomething else")]
    fn test_encode(#[case] key: &str, #[case] expected: &[u8]) {
        let key = StringKey::new(Bytes::copy_from_slice(key.as_bytes()));
        let encoded = key.encode();
        assert_eq!(&encoded[..], expected);
    }

    #[rstest]
    #[case(b"smykey", "mykey")]
    #[case(b"s", "")]
    #[case(b"ssomething else", "something else")]
    fn test_decode(#[case] encoded: &[u8], #[case] expected: &str) {
        let key = StringKey::decode(encoded).unwrap();
        assert_eq!(key.user_key, Bytes::copy_from_slice(expected.as_bytes()));
    }

    #[test]
    fn test_decode_invalid_prefix() {
        let encoded = b"xmykey";
        let err = StringKey::decode(encoded).unwrap_err();
        match err {
            DecoderError::InvalidPrefix => (),
            _ => panic!("Expected InvalidPrefix error"),
        }
    }

    #[test]
    fn test_decode_empty() {
        let encoded = b"";
        let err = StringKey::decode(encoded).unwrap_err();
        match err {
            DecoderError::Empty => (),
            _ => panic!("Expected Empty error"),
        }
    }
}
