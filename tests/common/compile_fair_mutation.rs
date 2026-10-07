use o3::{
    base::cell::region::{Identity, Token},
    depot::queue::pool::fair::Binding,
};

fn bypass<'id>(binding: Binding<'_, 'id, u8>, token: &mut Token<Identity<'id>>) {
    let _ = binding.lane_mut(token).push_back(1);
}

fn main() {}
