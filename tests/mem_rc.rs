pub mod common;

use std::{
    cell::Cell,
    mem::{forget, transmute},
    ptr,
};

use common::mem::{Edge, Target};
use o3::mem::{self, rc};

#[test]
fn transfers_raw_edge() {
    let finished = Cell::new(Some(0));
    let owner = Target::<u32>::try_owner(&finished).expect("owner allocation");
    let pointer = owner.into_raw();
    assert_eq!(finished.get(), Some(0));
    // SAFETY: into_raw transferred this exact live edge without releasing it.
    let owner = unsafe { mem::raw::Owner::from_raw(pointer) };
    assert_eq!(owner.get().value, 17);
    let other = owner.try_clone().expect("second edge");
    // SAFETY: the clone transfers its exact live edge; owner retains the target.
    unsafe { mem::raw::Owner::release_raw(other.into_raw()) };
    assert_eq!(finished.get(), Some(0));
    // SAFETY: owner transfers the last edge with no outstanding target borrow.
    unsafe { mem::raw::Owner::release_raw(owner.into_raw()) };
    assert_eq!(finished.get(), Some(1));
}

#[test]
fn forgotten_edge_retains() {
    let finished = Cell::new(Some(0));
    let pointer = {
        let owner = Target::<u32>::try_owner(&finished).expect("owner allocation");
        let other = owner.try_clone().expect("second edge");
        assert_eq!(owner.as_ptr(), other.as_ptr());
        assert_eq!(owner.get().value, 17);
        let pointer = other.as_ptr();
        forget(other);
        pointer
    };
    assert_eq!(finished.get(), Some(0));
    // SAFETY: the forgotten edge still owns this exact initialized allocation.
    {
        let retained = unsafe { mem::raw::Owner::from_raw(pointer) };
        assert_eq!(retained.get().value, 17);
    }
    assert_eq!(finished.get(), Some(1));
}

#[test]
fn keeps_exhausted_owner() {
    let finished = Cell::new(Some(0));
    // SAFETY: Count is transparent over u32; the remaining edges model forgotten owners.
    let refs = unsafe { transmute::<u32, mem::raw::Count>(u32::MAX) };
    let target = Box::new(Target {
        refs,
        value: 23,
        finished: &finished,
    });
    let pointer = ptr::NonNull::from(Box::leak(target));
    // SAFETY: this is one live edge in the modelled saturated domain.
    let owner = unsafe { mem::raw::Owner::from_raw(pointer) };
    assert!(matches!(owner.try_clone(), Err(rc::Error::Exhausted)));
    assert_eq!(owner.get().value, 23);
    assert_eq!(finished.get(), Some(0));
    let pointer = owner.into_raw();
    // SAFETY: the test retires its complete synthetic domain after disarming the owner.
    let target = unsafe { Box::from_raw(pointer.as_ptr()) };
    assert_eq!(target.value, 23);
}

#[test]
fn keeps_pointer_niches() {
    assert_eq!(size_of::<rc::Error>(), 0);
    assert_eq!(size_of::<mem::raw::Owner<Target<'_>>>(), size_of::<usize>());
    assert_eq!(
        size_of::<Option<mem::raw::Owner<Target<'_>>>>(),
        size_of::<usize>()
    );
    assert_eq!(
        size_of::<Result<mem::raw::Owner<Target<'_>>, rc::Error>>(),
        size_of::<usize>()
    );
}

#[test]
fn tracks_ref_transitions() {
    let edge = Edge::<u32>(mem::raw::Count::empty());
    assert!(mem::raw::Edge::is_empty(&edge));
    // SAFETY: this test exclusively owns the empty raw count.
    unsafe { mem::raw::Edge::activate(&edge) };
    assert!(mem::raw::Edge::is_unique(&edge));
    // SAFETY: the test owns the live edge used to create a second edge.
    assert_eq!(unsafe { mem::raw::Edge::try_retain(&edge) }, Ok(()));
    assert!(!mem::raw::Edge::is_unique(&edge));
    // SAFETY: the test owns both edges and consumes one here.
    assert!((unsafe { mem::raw::Edge::release(&edge) }) == (rc::Release::Shared));
    assert!(mem::raw::Edge::is_unique(&edge));
    // SAFETY: the test consumes its final edge.
    assert!((unsafe { mem::raw::Edge::release(&edge) }) == (rc::Release::Last));
    assert!(mem::raw::Edge::is_empty(&edge));

    // SAFETY: the final release above returned exclusive empty ownership.
    unsafe { mem::raw::Edge::activate(&edge) };
    assert!(mem::raw::Edge::is_unique(&edge));
    // SAFETY: the reactivated count has exactly one edge, owned by this test.
    unsafe { mem::raw::Edge::retire(&edge) };
    assert!(mem::raw::Edge::is_empty(&edge));

    let one = Edge::<u32>(mem::raw::Count::one());
    assert!(mem::raw::Edge::is_unique(&one));
}

#[test]
fn preserves_saturated() {
    // SAFETY: Count is transparent over u32 and admits every bit pattern.
    // This models one live edge plus forgotten edges without billions of clones.
    let count = unsafe { transmute::<u32, mem::raw::Count>(u32::MAX - 1) };
    let domain = Edge(count);
    // SAFETY: the local domain owns one live edge for every attempted retain.
    assert_eq!(unsafe { mem::raw::Edge::try_retain(&domain) }, Ok(()));
    // SAFETY: saturation leaves the existing owner edge live.
    assert_eq!(
        unsafe { mem::raw::Edge::try_retain(&domain) },
        Err(rc::Error::Exhausted)
    );
    // SAFETY: this consumes the successful retain above, leaving the original edge.
    assert!((unsafe { mem::raw::Edge::release(&domain) }) == (rc::Release::Shared));
    // SAFETY: the original live owner can retain the released capacity again.
    assert_eq!(unsafe { mem::raw::Edge::try_retain(&domain) }, Ok(()));
    // SAFETY: both tracked edges remain live when saturation is checked again.
    assert_eq!(
        unsafe { mem::raw::Edge::try_retain(&domain) },
        Err(rc::Error::Exhausted)
    );
}

#[test]
fn finalizes_usize_owner() {
    let finished = Cell::new(Some(0));
    let owner = Target::<usize>::try_owner(&finished).expect("owner allocation");
    let retained = owner.try_clone().expect("second edge");
    drop(owner);
    assert_eq!(finished.get(), Some(0));
    assert_eq!(retained.get().value, 17);
    drop(retained);
    assert_eq!(finished.get(), Some(1));
}

#[test]
fn preserves_usize_domain() {
    // SAFETY: Count is transparent over usize; the synthetic domain models
    // one live owner with all remaining edges forgotten.
    let wider = Edge(unsafe { transmute::<usize, mem::raw::Count<usize>>(u32::MAX as usize) });
    // SAFETY: this domain owns a live edge, even at the narrower word's limit.
    assert_eq!(unsafe { mem::raw::Edge::try_retain(&wider) }, Ok(()));
    // SAFETY: consume the additional edge admitted beyond u32::MAX.
    assert!(unsafe { mem::raw::Edge::release(&wider) } == rc::Release::Shared);

    // SAFETY: this models the same live ownership at the complete usize limit.
    let full = Edge(unsafe { transmute::<usize, mem::raw::Count<usize>>(usize::MAX) });
    // SAFETY: rejection leaves the original live owner in place.
    assert_eq!(
        unsafe { mem::raw::Edge::try_retain(&full) },
        Err(rc::Error::Exhausted)
    );
    // SAFETY: retire one modelled edge, making one retain available again.
    assert!(unsafe { mem::raw::Edge::release(&full) } == rc::Release::Shared);
    // SAFETY: the remaining live domain can reuse that released capacity.
    assert_eq!(unsafe { mem::raw::Edge::try_retain(&full) }, Ok(()));
}
