/// Unique slots retain their exclusive empty reference domain until frozen.
/// # Safety
/// Each unique slot must have no shared reference edges until freezing transfers that exact initialized owner into its shared reference domain.
pub unsafe trait PoolSharing<T: crate::mem::raw::Edge>:
    crate::depot::raw::PoolOwner<T>
{
}
