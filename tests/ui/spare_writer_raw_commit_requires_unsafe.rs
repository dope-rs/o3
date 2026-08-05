use o3::buffer::Owned;

fn main() {
    let mut owned = Owned::try_with_capacity(1).unwrap();
    let mut writer = owned.spare_writer();
    writer.try_commit_initialized(&[]).unwrap();
}
