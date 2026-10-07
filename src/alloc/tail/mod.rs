pub use block::Block;
pub use entry::Entry;
pub use plan::Plan;
pub(in crate::alloc::tail) use plan::Prefix;
pub use uninit::Uninit;
pub use view::View;

const _: () = {
    assert!(std::mem::size_of::<Block<usize, usize>>() == std::mem::size_of::<usize>());
    assert!(std::mem::size_of::<Option<Block<usize, usize>>>() == std::mem::size_of::<usize>());
    assert!(std::mem::size_of::<View<'static, usize, usize>>() == std::mem::size_of::<usize>());
};

mod block;
mod entry;
mod plan;
mod uninit;
mod view;
