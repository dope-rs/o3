use crate::depot::queue::{
    indexed::{lanes, lanes::runtime},
    links, tagged,
};

// SAFETY: construction initializes all nodes from the retained shape and inseparable N heads.
unsafe impl<T, const N: usize, C: crate::depot::raw::Count> crate::depot::raw::TagStore
    for lanes::Fifo<T, N, C>
{
    type Head = links::Chain;
    type Heads = [links::Chain; N];
    fn heads(&self) -> &Self::Heads {
        self.nodes.state()
    }
    unsafe fn hook(&self, index: links::Index) -> &tagged::Hook {
        &unsafe { self.nodes().get_unchecked(index.get()) }.hook
    }
}

// SAFETY: the fixed owner initializes every hook and publishes payload before membership.
unsafe impl<T, const N: usize, C: crate::depot::raw::Count> crate::depot::raw::LaneStore
    for lanes::Fifo<T, N, C>
{
    type Item = T;
    fn nodes(&self) -> &[lanes::Value<T>] {
        &self.nodes
    }
}

// SAFETY: the transition is called only after writing this exact detached payload.
unsafe impl<'a, T, const N: usize, const L: usize, C: crate::depot::raw::Count>
    crate::depot::raw::LaneInit for lanes::Position<'a, T, N, L, C>
{
    type Ready = lanes::Position<'a, T, N, L, C, true>;
    unsafe fn ready(self) -> Self::Ready {
        lanes::Position {
            store: self.store,
            index: self.index,
        }
    }
}

// SAFETY: construction retains the allocation's validated immutable shape and initializes every hook.
unsafe impl<T, C: crate::depot::raw::Count, L: crate::depot::raw::Count, H: tagged::raw::Head>
    crate::depot::raw::TagStore for runtime::Fifo<T, C, L, H>
{
    type Head = H;
    type Heads = [H];
    fn heads(&self) -> &Self::Heads {
        self.block.tail()
    }
    fn is_node(&self, word: u32) -> bool {
        (word as usize) < self.block.extent().shape().capacity().count().get()
    }
    unsafe fn hook(&self, index: links::Index) -> &tagged::Hook {
        &unsafe { self.nodes().get_unchecked(index.get()) }.hook
    }
}

// SAFETY: this owner initializes every hook and publishes values before membership.
unsafe impl<T, C: crate::depot::raw::Count, L: crate::depot::raw::Count, H: tagged::raw::Head>
    crate::depot::raw::LaneStore for runtime::Fifo<T, C, L, H>
{
    type Item = T;
    fn nodes(&self) -> &[lanes::Value<T>] {
        self.block.as_ref()
    }
}

// SAFETY: the exact runtime lane, node, and exclusive owner survive initialization.
unsafe impl<'a, T, C: crate::depot::raw::Count, L: crate::depot::raw::Count, H: tagged::raw::Head>
    crate::depot::raw::LaneInit
    for tagged::Endpoint<&'a mut runtime::Fifo<T, C, L, H>, links::Index, false>
{
    type Ready = tagged::Endpoint<&'a mut runtime::Fifo<T, C, L, H>, links::Index>;
    unsafe fn ready(self) -> Self::Ready {
        let index = self.parts().1;
        unsafe { self.at(index) }
    }
}

// SAFETY: the initialized lane proof retains this exact owner's live payload.
unsafe impl<'a, T, C: crate::depot::raw::Count, L: crate::depot::raw::Count, H: tagged::raw::Head>
    crate::depot::raw::Project<links::Index, links::Chain> for &'a runtime::Fifo<T, C, L, H>
{
    type Output = &'a T;

    unsafe fn project(self, index: links::Index) -> Self::Output {
        unsafe { (*self.nodes().get_unchecked(index.get()).value.get()).assume_init_ref() }
    }
}

mod init;
mod store;
mod undo;
pub(in crate::depot) use init::LaneInit;
pub(in crate::depot) use store::LaneStore;
pub(in crate::depot) use undo::LaneUndo;
