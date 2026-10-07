use crate::depot::queue::pool;

// SAFETY: Head retains this exact lane's current initialized head until projection.
unsafe impl<'a, T, S, R, const TRACK: bool> crate::depot::raw::Project<(), pool::Head>
    for (pool::Lane<'a, T, S, TRACK>, R)
{
    type Output = (pool::Front<'a, T, S, TRACK>, R);

    unsafe fn project(self, _: ()) -> Self::Output {
        let (lane, retained) = self;
        let index = lane.slot.queue.links.head_raw();
        (pool::Front::new(lane, index), retained)
    }
}
