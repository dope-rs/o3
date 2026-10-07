use crate::depot::index::set;

/// The exact packed allocation retained by an internal source.
pub trait Storage<Tag, Raw, const N: usize, K = ()> {
    type Mode;
    fn state(&self) -> &set::State<Tag, Raw, N, Self::Mode>;
}
impl<S: crate::depot::raw::SetSource<Tag, Raw, N>, Tag, Raw, const N: usize>
    Storage<Tag, Raw, N, set::Planar> for S
{
    type Mode = set::Planar;
    fn state(&self) -> &set::State<Tag, Raw, N> {
        self.planes()
    }
}
impl<Tag, Raw, const N: usize, M> Storage<Tag, Raw, N> for &set::State<Tag, Raw, N, M> {
    type Mode = M;
    fn state(&self) -> &set::State<Tag, Raw, N, M> {
        self
    }
}
