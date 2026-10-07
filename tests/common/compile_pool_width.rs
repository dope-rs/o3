use o3::buffer::pool::{CreateError, Pool, Width, owners, state};

fn main() -> Result<(), CreateError> {
    let layout = Width::<16>::new(1).map_err(CreateError::Layout)?;
    let pool = Pool::<state::Initialized, owners::Plain<_>>::try_from(layout)
        .map_err(CreateError::Exhausted)?;
    let _ = pool.copy([b"body"; 3]);
    Ok(())
}
