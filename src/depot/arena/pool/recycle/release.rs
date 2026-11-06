use std::{cell, marker, mem, ptr};

use crate::{
    alloc::tail,
    depot::{
        arena::pool::{self, recycle},
        capacity,
    },
};

pub trait Context<'owner, T>: Sized + 'owner
where
    T: recycle::Recycle + 'owner,
{
    fn released(&self, result: T::Release);
}

/// Fixed pinned storage which returns each recycle result to its context.
#[repr(transparent)]
pub struct Store<T: recycle::Recycle, C> {
    block: Storage<T, C>,
}

impl<T: recycle::Recycle, C> Store<T, C> {
    pub fn try_init(
        capacity: capacity::Count,
        context: C,
        seed: T::Seed,
    ) -> Result<Self, crate::alloc::Error> {
        Ok(Self {
            block: pool::Entry::init(
                tail::Plan::try_new(capacity)?.try_allocate()?,
                recycle::Header {
                    free: cell::Cell::new(ptr::null_mut()),
                    context: mem::ManuallyDrop::new(context),
                },
                |_| T::from_seed(seed),
            ),
        })
    }

    pub fn vacancy<'owner>(&'owner self) -> Option<pool::Vacant<'owner, T, C, Notify<'owner>>>
    where
        T: 'owner,
        C: Context<'owner, T>,
    {
        let header = tail::View::new(&self.block).header();
        pool::Vacant::new(header)
    }
}

impl<T: recycle::Recycle, C> Drop for Store<T, C> {
    fn drop(&mut self) {
        // SAFETY: the context is initialized once and wrapped solely so it is
        // destroyed before tail entries that may be retained by that context.
        unsafe { mem::ManuallyDrop::drop(&mut self.block.header_mut().context) };
    }
}

pub struct Notify<'a>(marker::PhantomData<fn(&'a ()) -> &'a ()>);

/// A published pinned value whose mutable reservation has been consumed.
#[derive(Debug)]
pub struct Frozen;

// SAFETY: the policy is installed only for the initialized slots of this owner.
unsafe impl<'owner, T, C> crate::depot::raw::PoolPolicy<T, C> for Notify<'owner>
where
    T: recycle::Recycle + 'owner,
    C: Context<'owner, T>,
{
    fn retire(mut entry: pool::Terminal<'_, T, C>) {
        let result = T::recycle(entry.get_mut());
        entry.recycle().context.released(result);
    }
}

// SAFETY: the exclusive pinned reservation excludes every other lease of this slot.
unsafe impl<'owner, T, C> crate::depot::raw::PoolMutable<T, C> for Notify<'owner>
where
    T: recycle::Recycle + 'owner,
    C: Context<'owner, T>,
{
}

type Storage<T, C> = tail::Block<recycle::Header<T, C>, pool::Entry<T, C>, capacity::Count>;
