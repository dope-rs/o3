use o3::cell::{BrandToken, Region};

fn main() {
    BrandToken::scope(|mut token| {
        let cell = Region::new(0_u8);
        *cell.borrow_mut(&mut token) = 1;
    });
}
