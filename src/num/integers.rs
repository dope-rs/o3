macro_rules! bounded_unsigned {
    ($name:ident, $raw:ty) => {
        #[doc = concat!(
                                            "A `",
                                            stringify!($raw),
                                            "` proven to lie in the inclusive `MIN..=MAX` range."
                                        )]
        #[repr(transparent)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name<const MIN: $raw, const MAX: $raw>($raw);

        impl<const MIN: $raw, const MAX: $raw> $name<MIN, MAX> {
            pub const fn new(value: $raw) -> Option<Self> {
                if MIN <= MAX && value >= MIN && value <= MAX {
                    Some(Self(value))
                } else {
                    None
                }
            }

            pub const fn from_usize(value: usize) -> Option<Self> {
                if usize::BITS <= <$raw>::BITS || value <= <$raw>::MAX as usize {
                    Self::new(value as $raw)
                } else {
                    None
                }
            }

            pub const fn get(self) -> $raw {
                self.0
            }

            pub const fn min() -> $raw {
                MIN
            }

            pub const fn max() -> $raw {
                MAX
            }
        }

        impl<const MIN: $raw, const MAX: $raw> From<$name<MIN, MAX>> for $raw {
            fn from(value: $name<MIN, MAX>) -> Self {
                value.get()
            }
        }
    };
}

bounded_unsigned!(BoundedU32, u32);
bounded_unsigned!(BoundedU64, u64);
