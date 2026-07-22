use o3::collections::{LinkedPool, LinkedPoolChain};

fn main() {
    let chain: LinkedPoolChain<'_, u8>;
    {
        let pool = Box::pin(LinkedPool::with_capacity(1));
        chain = pool.as_ref().chain();
    }
    drop(chain);
}
