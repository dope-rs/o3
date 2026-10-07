/// Exact projected and resident lengths for a rejected admission.
#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub struct LengthError {
    projected: usize,
    resident: u32,
}

impl LengthError {
    pub(in crate::buffer) const fn new(len: usize, resident: u32) -> Self {
        Self {
            projected: len,
            resident,
        }
    }

    pub const fn projected(self) -> usize {
        self.projected
    }

    pub const fn resident(self) -> u32 {
        self.resident
    }
}
