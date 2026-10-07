use std::mem;

use crate::depot::map::priority::store::Pair;

pub(super) struct Node<K, V, P> {
    pub(in crate::depot::map::priority) pair: mem::MaybeUninit<Pair<K, V, P>>,
    pub(in crate::depot::map::priority) bucket: u32,
    pub(in crate::depot::map::priority) position: u32,
}

const _: () = {
    let word = mem::size_of::<usize>();
    assert!(mem::size_of::<&'static mut [mem::MaybeUninit<Node<u32, u32, u32>>]>() == 2 * word);
    assert!(
        mem::size_of::<
            crate::depot::heap::raw::Hole<
                &'static mut crate::depot::map::priority::Core<u32, u32, u32>,
            >,
        >() == 3 * word
    );
};
