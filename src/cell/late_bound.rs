use std::cell::Cell;
use std::ptr::NonNull;
use std::rc::Rc;

pub struct LateBound<T: ?Sized> {
    cell: Rc<Cell<Option<NonNull<T>>>>,
}

impl<T: ?Sized> LateBound<T> {
    #[must_use]
    pub fn unbound() -> Self {
        Self {
            cell: Rc::new(Cell::new(None)),
        }
    }

    #[must_use]
    pub fn bound(ptr: NonNull<T>) -> Self {
        Self {
            cell: Rc::new(Cell::new(Some(ptr))),
        }
    }

    #[must_use]
    pub fn is_bound(&self) -> bool {
        self.cell.get().is_some()
    }

    #[must_use]
    pub fn as_ptr(&self) -> Option<NonNull<T>> {
        self.cell.get()
    }

    pub fn bind(&self, ptr: NonNull<T>) {
        self.cell.set(Some(ptr));
    }

    pub fn unbind(&self) {
        self.cell.set(None);
    }

    #[must_use]
    pub unsafe fn as_ref(&self) -> &T {
        unsafe { self.cell.get().unwrap_unchecked().as_ref() }
    }

    #[must_use]
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn as_mut(&self) -> &mut T {
        unsafe { self.cell.get().unwrap_unchecked().as_mut() }
    }
}

impl<T: ?Sized> Clone for LateBound<T> {
    fn clone(&self) -> Self {
        Self {
            cell: Rc::clone(&self.cell),
        }
    }
}

impl<T: ?Sized> Default for LateBound<T> {
    fn default() -> Self {
        Self::unbound()
    }
}
