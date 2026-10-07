use crate::depot::{
    self,
    queue::{links, tagged, tagged::Endpoint},
};

// SAFETY: the cursor proof retains this exact head and all reachable initialized
// hooks for the complete source loan, including each yielded item.
unsafe impl<'a, S: depot::raw::TagStore>
    depot::raw::Project<usize, Endpoint<&'a S, Option<links::Index>>> for &'a S
{
    type Output = Endpoint<Self, Option<links::Index>>;

    unsafe fn project(self, index: usize) -> Self::Output {
        let lane = unsafe { depot::raw::Project::<usize, Endpoint<&'a S>>::project(self, index) };
        Endpoint {
            key: tagged::raw::Head::first(lane.head()),
            store: lane.store,
            endpoint: lane.endpoint,
        }
    }
}
