use o3::depot::{
    arena::slab::key,
    index,
    map::flat::Table,
    queue::{
        indexed::{self, lanes},
        pool,
        round::Set,
    },
};

#[test]
fn keeps_thin_layouts() {
    assert_eq!(
        std::mem::size_of::<index::Stored<key::Handle>>(),
        std::mem::size_of::<u64>(),
    );
    assert_eq!(
        std::mem::align_of::<index::Stored<key::Handle>>(),
        std::mem::align_of::<u64>(),
    );
    assert_eq!(std::mem::size_of::<pool::Lanes<u8, ()>>(), 40);
    if usize::BITS == 64 {
        assert_eq!(std::mem::size_of::<Table<u64>>(), 40);
        assert_eq!(
            std::mem::size_of::<indexed::fifo::List<u64, u32, indexed::exact::Policy>>(),
            24
        );
        assert_eq!(std::mem::size_of::<lanes::Fifo<[u64; 2], 2>>(), 32);
        assert_eq!(std::mem::size_of::<lanes::runtime::Fifo<u64>>(), 24);
        assert_eq!(std::mem::size_of::<Set<()>>(), 48);
    }
}
