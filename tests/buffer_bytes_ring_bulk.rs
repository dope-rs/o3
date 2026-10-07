pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{collections::VecDeque, num::NonZero};

use common::alloc::system::Tracker;
use o3::buffer::bytes::window::reserve;

#[test]
fn bulk_wraps() {
    for capacity in 1..=13 {
        let capacity = NonZero::new(capacity).expect("nonzero capacity");
        let plan = reserve::Plan::<0>::new(capacity).expect("small allocation");
        let mut ring = reserve::Ring::try_new(plan).expect("ring allocation");
        let mut model = VecDeque::with_capacity(capacity.get());
        let ((), activity) = Tracker::count(|| {
            for turn in 0..64 {
                let bytes = [turn as u8; 13];
                let added = capacity.get() - model.len();
                let (first, last) = bytes[..added].split_at(added / 2);
                reserve::Append::try_new(
                    &mut ring,
                    [&[][..], first, &[][..], last, &[][..]],
                    usize::MAX,
                )
                .expect("all spare space is admitted")
                .commit();
                model.extend(bytes[..added].iter().copied());
                assert_eq!(ring.len(), model.len());
                let (front, back) = ring.as_slices();
                assert!(front.iter().chain(back).eq(model.iter()));

                reserve::Append::try_new(&mut ring, [], usize::MAX)
                    .expect("an empty source array fits a full ring")
                    .commit();
                ring.finish(&[]);
                let taken = turn % (capacity.get() + 1);
                let prefix = reserve::Prefix::new(&mut ring, taken);
                let (front, back) = prefix.as_slices();
                assert!(front.iter().chain(back).eq(model.iter().take(taken)));
                prefix.commit();
                model.drain(..taken);
                assert_eq!(ring.len(), model.len());
            }
            reserve::Prefix::new(&mut ring, usize::MAX).commit();
            reserve::Append::try_new(&mut ring, [&[7][..]], usize::MAX)
                .expect("an empty ring reuses its origin")
                .commit();
            assert_eq!(ring.as_slices(), (&[7][..], &[][..]));
        });
        assert_eq!(activity, 0);
    }
}
