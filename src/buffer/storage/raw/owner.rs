use std::{ptr, rc};

use crate::buffer::{resident, storage};

const VEC_TAG: usize = 1;
const RESIDENT_TAG: usize = 2;
const TAG_MASK: usize = VEC_TAG | RESIDENT_TAG;

#[repr(transparent)]
#[derive(Clone, Copy)]
struct Tagged(ptr::NonNull<()>);

impl Tagged {
    fn from_allocation(allocation: storage::raw::Allocation<()>) -> Self {
        Self(allocation.into_owner().erase())
    }

    fn from_resident(allocation: storage::raw::Allocation<resident::Lease>) -> Self {
        let ptr = allocation.into_owner().erase();
        Self(ptr.map_addr(|address| address | RESIDENT_TAG))
    }

    fn from_vec(buf: rc::Rc<Vec<u8>>) -> Self {
        let ptr = rc::Rc::into_raw(buf).cast_mut();
        let ptr = unsafe { ptr::NonNull::new_unchecked(ptr) }.cast::<()>();
        Self(ptr.map_addr(|address| address | VEC_TAG))
    }

    fn tag(self) -> usize {
        self.0.addr().get() & TAG_MASK
    }

    fn untagged(self) -> ptr::NonNull<()> {
        self.0.map_addr(|address| unsafe {
            use std::num::NonZeroUsize;
            NonZeroUsize::new_unchecked(address.get() & !TAG_MASK)
        })
    }

    fn retain(self) {
        let ptr = self.untagged();
        if self.tag() == VEC_TAG {
            unsafe { rc::Rc::increment_strong_count(ptr.cast::<Vec<u8>>().as_ptr()) };
        } else {
            unsafe { ptr.cast::<storage::raw::Prefix>().as_ref() }
                .refs
                .retain();
        }
    }

    fn release(self) {
        let ptr = self.untagged();
        let tag = self.tag();
        if tag == VEC_TAG {
            unsafe { rc::Rc::decrement_strong_count(ptr.cast::<Vec<u8>>().as_ptr()) };
            return;
        }
        if !unsafe { ptr.cast::<storage::raw::Prefix>().as_ref() }
            .refs
            .release()
        {
            return;
        }
        match tag {
            0 => unsafe { storage::raw::Header::<()>::destroy(ptr.cast()) },
            RESIDENT_TAG => unsafe { storage::raw::Header::<resident::Lease>::destroy(ptr.cast()) },
            _ => unreachable!(),
        }
    }

    fn resident_bytes(self) -> usize {
        let ptr = self.untagged();
        if self.tag() == VEC_TAG {
            unsafe { ptr.cast::<Vec<u8>>().as_ref() }.capacity()
        } else {
            unsafe { ptr.cast::<storage::raw::Prefix>().as_ref() }.capacity as usize
        }
    }
}

const _: () = assert!(size_of::<Option<Tagged>>() == size_of::<ptr::NonNull<()>>());

pub(in crate::buffer) struct Owner {
    tagged: Option<Tagged>,
    _thread: crate::ThreadBound,
}

impl Owner {
    pub(in crate::buffer) const NONE: Self = {
        use crate::ThreadBound;
        Self {
            tagged: None,
            _thread: ThreadBound::NEW,
        }
    };

    pub(in crate::buffer) fn from_allocation(allocation: storage::raw::Allocation) -> Self {
        Self {
            tagged: Some(Tagged::from_allocation(allocation)),
            _thread: Default::default(),
        }
    }

    pub(in crate::buffer) fn from_resident(
        allocation: storage::raw::Allocation<resident::Lease>,
    ) -> Self {
        Self {
            tagged: Some(Tagged::from_resident(allocation)),
            _thread: Default::default(),
        }
    }

    pub(in crate::buffer) fn from_vec(buf: rc::Rc<Vec<u8>>) -> Self {
        Self {
            tagged: Some(Tagged::from_vec(buf)),
            _thread: Default::default(),
        }
    }

    pub(in crate::buffer) fn resident_bytes(&self) -> usize {
        self.tagged.map_or(0, Tagged::resident_bytes)
    }
}

impl Clone for Owner {
    fn clone(&self) -> Self {
        if let Some(tagged) = self.tagged {
            tagged.retain();
        }
        Self {
            tagged: self.tagged,
            _thread: Default::default(),
        }
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        if let Some(tagged) = self.tagged {
            tagged.release();
        }
    }
}
