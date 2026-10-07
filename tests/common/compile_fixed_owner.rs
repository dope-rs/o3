use o3::depot::{
    arena::{self, linked::slots::fixed::Fixed},
    array::copy,
};

fn keeps_owner<N: arena::raw::LinkedNode>(owner: &mut Fixed<N>, input: N::Input) {
    let Some(mut batch) = owner.reserve::<1>(&copy::Vec::from([input])) else {
        return;
    };
    let Some(index) = batch.take() else {
        return;
    };
    drop(owner.entry(index));
    drop(batch);
}

fn main() {}
