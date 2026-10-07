use std::marker::PhantomData;

pub struct Key<'d>(pub PhantomData<&'d mut ()>);
