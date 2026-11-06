pub use block::Block;
pub use plan::Plan;
pub(in crate::alloc::tail) use plan::Prefix;
pub use uninit::Uninit;
pub use view::View;

const _: () = {
    assert!(std::mem::size_of::<Block<usize, usize>>() == std::mem::size_of::<usize>());
    assert!(std::mem::size_of::<Option<Block<usize, usize>>>() == std::mem::size_of::<usize>());
    assert!(std::mem::size_of::<View<'static, usize, usize>>() == std::mem::size_of::<usize>());
    assert!(std::mem::size_of::<Uninit<usize, usize>>() == 2 * std::mem::size_of::<usize>());
    assert!(
        std::mem::size_of::<Option<Uninit<usize, usize>>>() == 2 * std::mem::size_of::<usize>()
    );
    assert!(
        std::mem::size_of::<Result<Plan<usize, usize>, crate::alloc::Overflow>>()
            == 2 * std::mem::size_of::<usize>()
    );
};

mod block;
mod plan;
mod uninit;
mod view;
