use tracing::debug;

/// Debug logs payload data, formatting as JSON if it appears to be JSON, otherwise as hex
pub fn debug_log_payload(payload: &[u8], context: &str) {
    match payload.first() == Some(&b'{') && payload.last() == Some(&b'}') {
        true => debug!("{} (JSON): {}", context, String::from_utf8_lossy(payload)),
        false => debug!("{} (hex): {:02X?}", context, payload),
    }
}

/// Macro to create distinct New Type wrappers around Vec<u8> with common behavior
#[macro_export]
macro_rules! define_byte_vector {
    ($name:ident) => {
        #[derive(Clone)]
        pub struct $name(Vec<u8>);

        impl $name {
            pub fn new(x: Vec<u8>) -> Self {
                $name(x)
            }

            pub fn to_vec(self) -> Vec<u8> {
                self.0
            }
        }

        impl std::ops::Deref for $name {
            type Target = Vec<u8>;

            fn deref(&self) -> &Vec<u8> {
                &self.0
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}({})", stringify!($name), hex::encode(&self.0))
            }
        }

        impl AsRef<[u8]> for $name {
            fn as_ref(&self) -> &[u8] {
                &self.0
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                use base64::{Engine as _, engine::general_purpose::STANDARD};
                serializer.serialize_str(&STANDARD.encode(&self.0))
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                use base64::{Engine as _, engine::general_purpose::STANDARD};
                let s = String::deserialize(deserializer)?;
                let bytes = STANDARD.decode(&s).map_err(serde::de::Error::custom)?;
                Ok($name(bytes))
            }
        }
    };
}

// Define generic ByteVector using the macro
define_byte_vector!(ByteVector);

#[cfg(test)]
mod tests {

    use super::ByteVector;

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
