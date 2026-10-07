/// Borrowed payload access supported by the retained slot state.
/// The retained `KeyOwner` supplies slot and topology validity for every borrowed operation.
pub trait Access<P = ()>: crate::depot::raw::KeyOwner<P> {
    type Read<'a>
    where
        Self: 'a;
    type Edit<'a>
    where
        Self: 'a;

    fn get(&mut self) -> Self::Read<'_>;
    fn edit(&mut self) -> Self::Edit<'_>;
}
