use crate::{
    buffer::{
        segments,
        segments::{
            Chain, Key, Pool,
            chain::{edit, edit::join},
            handles,
        },
    },
    depot,
    depot::{
        arena::linked::{fifo::ends, slots},
        capacity,
    },
};

pub(in crate::buffer::segments::chain::edit::join) struct Incoming<'a, T> {
    pub(in crate::buffer::segments::chain::edit::join) source: &'a mut Chain<T, Pool<T>>,
    pub(in crate::buffer::segments::chain::edit::join) input: join::Input,
}

struct Drain<'a, T> {
    store: &'a slots::Store<segments::Item<T>>,
    key: &'a mut Key,
    links: ends::Exclusive<slots::Index>,
    remaining: capacity::Count,
}

// SAFETY: the input variant and count belong to this exclusively borrowed source.
unsafe impl<T> depot::raw::Input<segments::Item<T>> for Incoming<'_, T> {
    type Item = segments::Item<T>;
    type Error = edit::Cause;
    type Context = ();
    type Output = ();

    fn count(&self) -> capacity::Count {
        capacity::Count::new(match self.input {
            join::Input::Single(_) => 1,
            join::Input::Multi { count, .. } => count.get(),
        })
    }

    fn reject(self, error: capacity::Shortage) -> Self::Error {
        edit::Cause::Nodes(error)
    }
}

// SAFETY: the exclusively retained input already owns every node to detach.
unsafe impl<T> depot::raw::Start<segments::Item<T>> for Incoming<'_, T> {
    fn start(
        self,
    ) -> (
        impl depot::raw::Exact<Item = segments::Item<T>, Output = ()>,
        (),
    ) {
        let store = handles::nodes(&self.source.sequence.handle);
        let (links, count) = match self.input {
            join::Input::Single(index) => {
                let mut links = ends::Exclusive::empty();
                unsafe { links.append_store(store, index) };
                (links, 1)
            }
            join::Input::Multi { index, count } => (
                unsafe { handles::descriptors(&self.source.sequence.handle).detached(index) }
                    .take()
                    .0
                    .links,
                count.get(),
            ),
        };
        (
            Drain {
                store,
                key: &mut self.source.sequence.key,
                links,
                remaining: capacity::Count::new(count),
            },
            (),
        )
    }
}

impl<T> Iterator for Drain<'_, T> {
    type Item = segments::Item<T>;

    fn next(&mut self) -> Option<segments::Item<T>> {
        // SAFETY: admission transferred this exact source partition into the drain.
        let entry = unsafe { slots::Front::new(self.store, &mut self.links) }?;
        self.remaining = capacity::Count::new(self.remaining.raw().wrapping_sub(1));
        Some(entry.pop())
    }
}

// SAFETY: the links and count are detached together from the admitted source.
unsafe impl<T> depot::raw::Exact for Drain<'_, T> {
    type Output = ();
    fn remaining(&self) -> capacity::Count {
        self.remaining
    }
    unsafe fn finish(self) {
        *self.key = Key::EMPTY;
    }
}
