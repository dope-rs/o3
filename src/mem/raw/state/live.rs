use crate::mem::{self, rc};

/// The nonzero value of an already owned local reference domain.
#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub(crate) struct Live<W: mem::raw::Counter = u32>(W::NonZero);

impl<W: mem::raw::Counter> Live<W> {
    pub(crate) const ONE: Self = Self(W::LIVE);

    /// # Safety
    /// The caller already owns a live edge, so value is nonzero.
    pub(in crate::mem::raw) unsafe fn new(value: W) -> Self {
        // SAFETY: the caller's live edge proves this exact count is nonzero.
        Self(unsafe { value.nonzero() })
    }

    pub(in crate::mem::raw) fn get(self) -> W {
        W::get(self.0)
    }

    pub(crate) fn try_retain(self) -> Result<Self, rc::Error> {
        let value = self.get().checked_add(W::ONE).ok_or(rc::Error::Exhausted)?;
        // SAFETY: adding one to a nonzero count without overflow remains nonzero.
        Ok(unsafe { Self::new(value) })
    }

    pub(crate) fn release(self) -> Option<Self> {
        let value = self.get().wrapping_sub(W::ONE);
        if value == W::ZERO {
            None
        } else {
            // SAFETY: the last-owner zero case returned above.
            Some(unsafe { Self::new(value) })
        }
    }
}

const _: () = {
    assert!(size_of::<Live>() == size_of::<u32>());
    assert!(size_of::<Option<Live>>() == size_of::<Live>());
    assert!(size_of::<Live<usize>>() == size_of::<usize>());
    assert!(size_of::<Option<Live<usize>>>() == size_of::<usize>());
};
