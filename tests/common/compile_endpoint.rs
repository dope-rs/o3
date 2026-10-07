use o3::{
    base::cell::region::{Identity, Token},
    depot::queue::indexed::lanes::runtime::shared::Head,
};

fn replace<'id>(head: Head<'_, 'id, ()>, token: &mut Token<Identity<'id>>) {
    let _ = head.borrow(token).into_store();
}

fn main() {}
