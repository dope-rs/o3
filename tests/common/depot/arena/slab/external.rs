use o3::depot::arena::slab::key;

pub type Generation = key::Generation<3>;
pub type Wide = key::Generation;
pub type Epoch = key::Generation<{ u8::MAX as u32 }>;
