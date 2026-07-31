#![forbid(unsafe_code)]

use o3::buffer::InlineBytes;

#[derive(Clone, Debug, PartialEq, Eq)]
enum WirePrefix<const INLINE: usize> {
    Inline(InlineBytes<INLINE>),
    Owned(Vec<u8>),
}

impl<const INLINE: usize> WirePrefix<INLINE> {
    fn with_capacity(capacity: usize) -> Self {
        if capacity <= INLINE {
            Self::Inline(InlineBytes::new())
        } else {
            Self::Owned(Vec::with_capacity(capacity))
        }
    }

    fn push(&mut self, byte: u8) {
        match self {
            Self::Inline(inline) => {
                if inline.try_push(byte).is_ok() {
                    return;
                }
                let mut owned = Vec::with_capacity(INLINE.saturating_mul(2));
                owned.extend_from_slice(inline.as_slice());
                owned.push(byte);
                *self = Self::Owned(owned);
            }
            Self::Owned(owned) => owned.push(byte),
        }
    }

    fn as_slice(&self) -> &[u8] {
        match self {
            Self::Inline(inline) => inline.as_slice(),
            Self::Owned(owned) => owned,
        }
    }
}

impl<const INLINE: usize> Extend<u8> for WirePrefix<INLINE> {
    fn extend<T: IntoIterator<Item = u8>>(&mut self, iter: T) {
        for byte in iter {
            self.push(byte);
        }
    }
}

#[test]
fn sark_wire_prefix_capacity_is_a_compile_time_choice() {
    let mut prefix = WirePrefix::<8>::with_capacity(8);
    prefix.extend(*b"12345678");

    assert!(matches!(prefix, WirePrefix::Inline(_)));
    assert_eq!(prefix.as_slice(), b"12345678");
}

#[test]
fn sark_wire_prefix_promotes_only_after_inline_capacity() {
    let mut prefix = WirePrefix::<8>::with_capacity(8);
    prefix.extend(*b"123456789");

    assert!(matches!(prefix, WirePrefix::Owned(_)));
    assert_eq!(prefix.as_slice(), b"123456789");
}

#[test]
fn sark_known_large_wire_prefix_starts_owned() {
    let prefix = WirePrefix::<8>::with_capacity(9);

    assert!(matches!(prefix, WirePrefix::Owned(_)));
}
