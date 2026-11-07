pub(in crate::buffer::segments::chain) use shape::Shape;
use shape::Split;
use topology::{Original, Topology};

use crate::{
    buffer,
    buffer::{
        segments,
        segments::{Chain, Handle as _, Pool, assembly::total, handles},
    },
    depot,
    depot::{arena::linked::slots, capacity},
};

mod shape;
mod topology;

/// Exclusive access to the byte prefix of one retained chain.
struct Projection<'a, T> {
    target: Original<'a, T>,
    shape: Shape<'a, T>,
    count: segments::Count,
    len: usize,
    resident: usize,
    different_pool: bool,
}

struct Drain<'a, T: buffer::raw::Segment> {
    source: Topology<'a, T>,
    full: segments::Count,
    split: Option<Split<'a, T>>,
}

impl<'a, T: buffer::raw::Segment> Projection<'a, T> {
    fn try_new(
        target: &'a mut Chain<T, Pool<T>>,
        shape: Shape<'a, T>,
    ) -> Result<Self, segments::Error> {
        let target = Original::new(&target.handle, &mut target.key, |index| {
            // SAFETY: the exclusive target key owns its live single node.
            let owner = unsafe { handles::nodes(&target.handle).value(index) };
            (owner.len().get(), buffer::Residency::resident(owner))
        });
        let different_pool = !target.handle().is_same(shape.source);
        let (current, base_len, base_mem) = target.total();
        let count = total::Error::count(current, shape.count())?;
        let len = total::Error::logical(base_len, shape.amount)?;
        let resident = total::Error::resident(base_mem, shape.projected_resident)?;
        Ok(Self {
            target,
            shape,
            count,
            len,
            resident,
            different_pool,
        })
    }

    fn try_commit(self) -> Result<(), segments::Error> {
        if self.shape.amount == 0 {
            return Ok(());
        }
        let store = handles::nodes(self.target.handle());
        let reserve = slots::Input::try_new(store, self)?;
        let (nodes, mut output) = reserve.try_start()?;
        let source = nodes.run(|claim, value| {
            unsafe { claim.insert(value).append(&mut output.body.links) };
        });
        source.restore();
        output.restore();
        Ok(())
    }
}

// SAFETY: the exclusive shape retains exactly the selected full nodes and split owner.
unsafe impl<'a, T: buffer::raw::Segment> depot::raw::Input<segments::Item<T>>
    for Projection<'a, T>
{
    type Item = segments::Item<T>;
    type Error = segments::Error;
    type Context = Topology<'a, T>;
    type Output = Topology<'a, T>;

    fn count(&self) -> capacity::Count {
        capacity::Count::new(if self.different_pool {
            self.shape.count().into_raw()
        } else {
            u32::from(self.shape.split.is_some())
        })
    }

    fn reject(self, error: capacity::Shortage) -> Self::Error {
        segments::Error::SegmentCapacity(error)
    }
}

// SAFETY: topology admission precedes any node transfer and retains both owners.
unsafe impl<T: buffer::raw::Segment> depot::raw::TryStart<segments::Item<T>> for Projection<'_, T> {
    fn try_start(
        mut self,
    ) -> Result<
        (
            impl depot::raw::Exact<Item = segments::Item<T>, Output = Self::Output>,
            Self::Context,
        ),
        Self::Error,
    > {
        let input = handles::nodes(self.shape.source);
        let original = Original::new(self.shape.source, self.shape.key, |index| {
            let owner = match &self.shape.split {
                Some(split) => split.tail.as_ref(),
                // SAFETY: a whole single node has no outstanding partial borrow.
                None => unsafe { input.value(index) }.as_ref(),
            };
            (owner.as_ref().len(), buffer::Residency::resident(owner))
        });
        let (mut source, mut target) = Topology::try_pair(
            original,
            self.target,
            self.shape.remains,
            self.count,
            &self.different_pool,
        )?;
        let output = handles::nodes(target.body.handle);
        if !self.different_pool {
            while self.shape.full != segments::Count::EMPTY {
                let index = source.body.links.head_raw();
                let index = unsafe { source.body.links.detach_store(input, index) };
                unsafe { target.body.links.append_store(output, index) };
                self.shape.full = segments::Count::new(self.shape.full.into_raw().wrapping_sub(1));
            }
        }
        source.body.len -= self.shape.amount;
        source.body.resident -= self.shape.full_resident;
        target.body.len = self.len;
        target.body.resident = self.resident;
        Ok((
            Drain {
                source,
                full: self.shape.full,
                split: self.shape.split,
            },
            target,
        ))
    }
}

impl<T: buffer::raw::Segment> Iterator for Drain<'_, T> {
    type Item = segments::Item<T>;

    fn next(&mut self) -> Option<segments::Item<T>> {
        let store = handles::nodes(self.source.body.handle);
        if self.full != segments::Count::EMPTY {
            let index = self.source.body.links.head_raw();
            let index = unsafe { self.source.body.links.detach_store(store, index) };
            self.full = segments::Count::new(self.full.into_raw().wrapping_sub(1));
            return Some(unsafe { store.detached(index) }.take().0);
        }
        let split = self.split.take()?;
        split.tail.commit();
        Some(split.prefix)
    }
}

// SAFETY: every iteration consumes exactly one selected full node or the sole split.
unsafe impl<'a, T: buffer::raw::Segment> depot::raw::Exact for Drain<'a, T> {
    type Output = Topology<'a, T>;
    fn remaining(&self) -> capacity::Count {
        capacity::Count::new(
            self.full
                .into_raw()
                .wrapping_add(u32::from(self.split.is_some())),
        )
    }
    unsafe fn finish(self) -> Self::Output {
        self.source
    }
}

const _: () = {
    use buffer::{resident, storage};

    assert!(size_of::<Projection<'static, storage::Shared>>() == 19 * size_of::<usize>());
    assert!(size_of::<Projection<'static, resident::Shared<'static>>>() == 20 * size_of::<usize>());
};
