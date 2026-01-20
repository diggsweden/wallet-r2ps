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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_works() {
        let bv = ByteVector::new(vec![0xaa, 0x55, 0x00, 0xbb]);
        // verify that fmt::Debug works
        assert_eq!(format!("{:?}", bv), "ByteVector(aa5500bb)");
    }

    #[test]
    fn deref_works() {
        let bv = ByteVector::new(vec![1, 2, 3]);

        // Test deref coercion to slice
        let slice: &[u8] = &bv;
        assert_eq!(slice, &[1, 2, 3]);
    }

    #[test]
    fn as_ref_works() {
        let bv = ByteVector::new(vec![4, 5, 6]);

        // Test AsRef trait
        let as_ref: &[u8] = bv.as_ref();
        assert_eq!(as_ref, &[4, 5, 6]);
    }
}