/// Seals the primitive widths that bitmap words and hierarchy shapes admit.
pub trait Sealed {}

impl Sealed for usize {}
impl Sealed for u32 {}
