use std::num::NonZero;

use crate::{
    buffer,
    buffer::{
        segments,
        segments::{Chain, Key, Pool, chain::view, handles},
        write::consume,
    },
    depot::arena::linked::slots,
};

pub(in crate::buffer::segments::chain) struct Shape<'a, T> {
    pub(in crate::buffer::segments::chain::prefix) source: &'a Pool<T>,
    pub(in crate::buffer::segments::chain::prefix) key: &'a mut Key,
    pub(in crate::buffer::segments::chain::prefix) amount: usize,
    pub(in crate::buffer::segments::chain::prefix) full: segments::Count,
    pub(in crate::buffer::segments::chain::prefix) remains: segments::Count,
    pub(in crate::buffer::segments::chain::prefix) split: Option<Split<'a, T>>,
    pub(in crate::buffer::segments::chain::prefix) full_resident: usize,
    pub(in crate::buffer::segments::chain::prefix) projected_resident: usize,
}

pub(in crate::buffer::segments::chain::prefix) struct Split<'a, T> {
    pub(in crate::buffer::segments::chain::prefix) prefix: segments::Item<T>,
    pub(in crate::buffer::segments::chain::prefix) tail: segments::Tail<'a, T, NonZero<usize>>,
}

impl<'a, T: buffer::raw::Segment> Shape<'a, T> {
    pub(in crate::buffer::segments::chain) fn try_new(
        sequence: &'a mut Chain<T, Pool<T>>,
        amount: usize,
    ) -> Result<Self, segments::Error> {
        use segments::Tail;

        consume::Error::check(amount, sequence.len()).map_err(segments::Error::Boundary)?;
        let (total, mut index) = match sequence.state() {
            view::State::Empty => (segments::Count::EMPTY, slots::Index::NONE),
            view::State::Single(index, _) => (segments::Count::ONE, index),
            view::State::Multi(desc) => (
                segments::Count::new(desc.count.get()),
                desc.links.head_raw(),
            ),
        };
        let source = &sequence.sequence.handle;
        let key = &mut sequence.sequence.key;
        let Some(mut rest) = NonZero::new(amount) else {
            return Ok(Self {
                full: segments::Count::EMPTY,
                remains: total,
                split: None,
                full_resident: 0,
                projected_resident: 0,
                amount,
                source,
                key,
            });
        };
        let mut resident = 0usize;
        let mut full = segments::Count::EMPTY;
        let store = handles::nodes(source);
        loop {
            // SAFETY: the exclusive original owns this live node; each full
            // step advances once, and a retained partial borrow ends the scan.
            let owner = unsafe { &mut *store.value_ptr(index) };
            let len = owner.len();
            let charge = buffer::Residency::resident(owner);
            if let Some(tail) = Tail::positive(owner, rest) {
                let prefix = segments::Item::from(&tail)
                    .try_retain()
                    .map_err(segments::Error::Retain)?;
                let added = buffer::Residency::resident(&prefix);
                return Ok(Self {
                    full,
                    remains: segments::Count::new(total.into_raw() - full.into_raw()),
                    split: Some(Split { prefix, tail }),
                    full_resident: resident,
                    projected_resident: resident.wrapping_add(added),
                    amount,
                    source,
                    key,
                });
            }
            full = segments::Count::new(full.into_raw().wrapping_add(1));
            resident = resident.wrapping_add(charge);
            if rest == len {
                break;
            }
            // SAFETY: shorter and exact prefixes returned above.
            rest = unsafe { NonZero::new_unchecked(rest.get() - len.get()) };
            // SAFETY: positive admitted bytes remain in the same original topology.
            index = unsafe { store.next(index) };
        }
        Ok(Self {
            full,
            remains: segments::Count::new(total.into_raw() - full.into_raw()),
            split: None,
            full_resident: resident,
            projected_resident: resident,
            amount,
            source,
            key,
        })
    }

    pub(in crate::buffer::segments::chain::prefix) fn count(&self) -> segments::Count {
        segments::Count::new(
            self.full
                .into_raw()
                .wrapping_add(u32::from(self.split.is_some())),
        )
    }
}

const _: () = {
    use buffer::{resident, storage};

    assert!(size_of::<Shape<'static, storage::Shared>>() == 11 * size_of::<usize>());
    assert!(
        size_of::<Result<Shape<'static, storage::Shared>, segments::Error>>()
            == 11 * size_of::<usize>()
    );
    assert!(size_of::<Option<Split<'static, storage::Shared>>>() == 5 * size_of::<usize>());
    assert!(size_of::<Shape<'static, resident::Shared<'static>>>() == 12 * size_of::<usize>());
    assert!(
        size_of::<Result<Shape<'static, resident::Shared<'static>>, segments::Error>>()
            == 12 * size_of::<usize>()
    );
    assert!(
        size_of::<Option<Split<'static, resident::Shared<'static>>>>() == 6 * size_of::<usize>()
    );
};

impl<T: buffer::raw::Segment> Shape<'_, T> {
    pub(in crate::buffer::segments::chain) fn try_take(self) -> Result<Chain<T>, segments::Error> {
        let mut target = Chain::new(self.source.try_clone().map_err(segments::Error::Retain)?);
        super::Projection::try_new(&mut target, self)?.try_commit()?;
        Ok(target)
    }

    pub(in crate::buffer::segments::chain) fn try_move_to(
        self,
        target: &mut Chain<T>,
    ) -> Result<(), segments::Error> {
        super::Projection::try_new(target, self)?.try_commit()
    }
}
