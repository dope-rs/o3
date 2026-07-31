use o3::buffer::{SharedPool, Uninitialized};

fn main() {
    let pool = SharedPool::<Uninitialized>::try_new(1, 8).unwrap();
    let mut lease = pool.try_acquire().unwrap();
    let _ = lease.spare_mut();
}
