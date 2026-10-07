use std::mem;

use o3::buffer::{self, bytes::window, pool, storage};

#[test]
fn carriers_fit_words() {
    let word = mem::size_of::<usize>();
    assert_eq!(mem::size_of::<storage::provided::Root<'static, ()>>(), word);
    assert_eq!(
        mem::size_of::<storage::Owned<(), { buffer::BLOCK_CAPACITY }>>(),
        2 * word
    );
    assert_eq!(mem::size_of::<window::Snapshot<64>>(), 3 * word);
    assert_eq!(mem::size_of::<window::Snapshot<1_048_576>>(), 3 * word);
    assert_eq!(mem::size_of::<Option<storage::Shared>>(), 3 * word);
    assert_eq!(
        mem::size_of::<
            o3::buffer::storage::Shared<
                pool::owners::Edge<pool::owners::Plain, o3::buffer::pool::Borrowed<'static>>,
            >,
        >(),
        3 * word
    );
    assert_eq!(mem::size_of::<storage::Shared>(), 3 * word);
    assert_eq!(
        mem::size_of::<Result<storage::Shared, o3::mem::rc::Error>>(),
        mem::size_of::<storage::Shared>()
    );
}
