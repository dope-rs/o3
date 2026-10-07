use crate::depot::{
    map,
    queue::{indexed::planes::map::Table, links},
};

unsafe impl<K, V, const N: usize> map::raw::Mutation for &mut Table<K, V, N> {
    unsafe fn table_mut(&mut self) -> &mut map::raw::Table<links::Index> {
        &mut self.directory
    }

    unsafe fn shift(&mut self, hole: &map::raw::Vacant, next: &map::raw::Occupied, hash: u64) {
        let (bucket, index) = unsafe { self.directory.shift(hole, next, hash) };
        let index = *index;
        let node = unsafe { self.core.node(index) };
        unsafe { (*node.record.get()).assume_init_mut() }.bucket = bucket;
    }
}
