use o3::cell::{BrandToken, RegionCell};

fn main() {
    BrandToken::scope(|mut token| {
        let cell = RegionCell::new(0_u8);
        *cell.borrow_mut(&mut token) = 1;
    });
}
