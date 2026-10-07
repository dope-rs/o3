use o3::depot::{
    arena::{self, linked::slots::fixed::Fixed},
    array::copy,
};

fn rejects_capacity<N: arena::raw::LinkedNode>(owner: &mut Fixed<N>, input: N::Input) {
    let inputs = copy::Vec::<_, 2>::from([input; 2]);
    let _ = owner.reserve::<1>(&inputs);
}

fn main() {}
