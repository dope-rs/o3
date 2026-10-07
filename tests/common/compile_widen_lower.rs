use o3::base::num::bounded::U32;

fn main() {
    let _ = U32::<3, 7>::exact::<5>().widen::<4, 8>();
}
