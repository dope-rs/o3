use o3::buffer;

fn main() {
    let pool = buffer::Pool::<buffer::Uninitialized>::try_new(1, 8).unwrap();
    let mut lease = pool.try_acquire().unwrap();
    let _ = lease.spare_mut();
}
