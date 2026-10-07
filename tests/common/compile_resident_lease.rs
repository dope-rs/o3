use o3::buffer::{
    pool::{Lease, owners},
    write::SpareExt,
};

fn mutate(lease: &mut Lease<owners::Resident<'_>>) {
    let _ = SpareExt::try_prefix::<1>(lease, 0);
}

fn main() {}
