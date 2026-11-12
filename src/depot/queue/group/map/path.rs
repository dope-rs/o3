use crate::depot::queue::group::map;

pub(in crate::depot::queue::group::map) fn select<'a, const LANES: usize>(
    source: &'a map::Branches<LANES>,
    root: &map::Node<map::BranchTag>,
    key: u64,
) -> (&'a map::Node<map::Tag>, map::Place) {
    let mut index = root.bind(source).key().0;
    let mut parent = None;
    loop {
        // SAFETY: each branch link is initialized in this retained, unchanged source.
        let branch: &map::Branch = unsafe { crate::depot::raw::Proof::new((source, index)) }.into();
        match branch.child(key) {
            map::Link::Owner(owner) => return (owner, (index, parent)),
            map::Link::Branch(branch) => {
                parent = Some(index);
                index = branch.bind(source).key().0;
            }
        }
    }
}
