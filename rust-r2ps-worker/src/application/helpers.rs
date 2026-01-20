use std::fmt;
use std::ops::Deref;

#[derive(Clone)]
pub struct ByteVector(Vec<u8>);

impl ByteVector {
    pub fn new(x: Vec<u8>) -> ByteVector {
        ByteVector(x)
    }
}

impl Deref for ByteVector {
    type Target = Vec<u8>;

    fn deref(&self) -> &Vec<u8> {
        &self.0
    }
}

impl fmt::Debug for ByteVector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ByteVector({})", hex::encode(&self.0))
    }
}

impl AsRef<[u8]> for ByteVector {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}
