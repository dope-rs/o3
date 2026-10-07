use o3::depot::map::compact::{Error, Planes};

fn main() -> Result<(), Error> {
    let (planes, _) = Planes::try_new([1], 0)?;
    let _ = planes.layout::<1>();
    Ok(())
}
