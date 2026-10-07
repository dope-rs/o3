pub mod common;

use std::{
    cell::{Cell, RefCell},
    mem, ptr,
    rc::Rc,
    slice,
};

use common::depot::arena::pool::{Pinned, Reentry, Releases, Value};
use o3::depot::{
    arena::pool::recycle::{self, release},
    capacity::Count,
};

#[test]
fn reuses_pinned_value() {
    let pool = recycle::Store::<Pinned>::try_new(Count::new(1)).expect("one pinned slot");
    let address = {
        let mut first = pool.take().expect("the slot is initially free");
        assert!(pool.take().is_none());
        assert_eq!(mem::size_of_val(&first.get().pin), 0);
        first.get_mut().value.set(7);
        assert_eq!(first.get().value.get(), 7);
        ptr::from_ref(first.get().get_ref())
    };

    {
        let second = (pool.take()).expect("the released slot is free");
        assert_eq!(ptr::from_ref(second.get().get_ref()), address);
        assert_eq!(second.get().value.get(), 7);
        assert!(pool.take().is_none());
    }
}

#[test]
fn fills_disjoint_borrows() {
    let mut values = [0; 4];
    let pool = {
        let pointer = values.as_mut_ptr();
        let mut next = 0;
        let pool = recycle::Store::try_from_fn(Count::new(2), |index| {
            assert_eq!(index, next);
            next += 1;
            // SAFETY: the two admitted positions partition these four elements once.
            unsafe { slice::from_raw_parts_mut(pointer.add(index as usize * 2), 2) }
        })
        .expect("two borrowed residents");
        assert_eq!(next, 2);
        pool
    };
    let header = pool.vacancies();
    {
        let mut first = header.take().expect("first resident");
        let mut second = header.take().expect("second resident");
        first.get_mut().get_mut().copy_from_slice(&[10, 11]);
        second.get_mut().get_mut().copy_from_slice(&[20, 21]);
        assert!(header.take().is_none());
    }
    drop(pool);
    assert_eq!(values, [10, 11, 20, 21]);
    let mut calls = 0;
    let empty = recycle::Store::try_from_fn(Count::EMPTY, |index| {
        calls += 1;
        index
    })
    .expect("zero resident allocation");
    assert_eq!(calls, 0);
    assert!(empty.take().is_none());
}

#[test]
fn keeps_pointer_layout() {
    use o3::depot::arena::pool::{Shared, shared};
    type Owner = Shared<'static, shared::Resident<u64>, shared::Header<u64>, shared::Initialized>;
    assert_eq!(
        mem::size_of::<Result<Owner, o3::mem::rc::Error>>(),
        mem::size_of::<Owner>()
    );
    type Static = Value<'static>;
    type Notify = Releases<'static>;
    assert_eq!(
        mem::size_of::<recycle::Store<Static>>(),
        mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<o3::depot::arena::pool::Unique<'static, Static, recycle::Header<Static>>>(),
        mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<
            Option<o3::depot::arena::pool::Unique<'static, Static, recycle::Header<Static>>>,
        >(),
        mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<release::Store<Static, Notify>>(),
        mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<
            o3::depot::arena::pool::Vacant<
                'static,
                Static,
                release::Header<Static, Notify>,
                release::Notify<'static>,
            >,
        >(),
        2 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<
            Option<
                o3::depot::arena::pool::Vacant<
                    'static,
                    Static,
                    release::Header<Static, Notify>,
                    release::Notify<'static>,
                >,
            >,
        >(),
        2 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<
            o3::depot::arena::pool::Unique<
                'static,
                Static,
                release::Header<Static, Notify>,
                release::Notify<'static>,
            >,
        >(),
        mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<
            o3::depot::arena::pool::Unique<
                'static,
                Static,
                release::Header<Static, Notify>,
                release::Notify<'static>,
                release::Frozen,
            >,
        >(),
        mem::size_of::<usize>()
    );
}

#[test]
fn observes_each_release() {
    let resets = Cell::new(Some(0));
    let seen = Cell::new((Some(0), true));
    let closed = Cell::new(Some(0));
    {
        let result = release::Store::try_init(
            Count::new(1),
            Releases {
                closed: &closed,
                released: &seen,
            },
            &resets,
        );
        let pool = result.expect("the observed store must admit one slot");
        {
            let vacancy = (pool.vacancy()).expect("the slot is initially vacant");
            let claim = vacancy.reserve();
            assert_eq!(claim.get().value.get(), 0);
            assert_eq!(resets.get(), Some(0));
            assert_eq!(seen.get(), (Some(0), true));
        }
        assert_eq!(resets.get(), Some(1));
        assert_eq!(seen.get(), (Some(1), true));
        {
            let vacancy = (pool.vacancy()).expect("the recycled slot is vacant");
            let mut claim = vacancy.reserve();
            assert_eq!(claim.get().value.get(), 0);
            let (stored, current) = claim.get().bind();
            assert_eq!(stored, current);
            claim.get_mut().set(7);
            let lease = claim.commit();
            assert_eq!(lease.get().value.get(), 7);
            assert_eq!(resets.get(), Some(1));
            assert_eq!(seen.get(), (Some(1), true));
        }
        assert_eq!(resets.get(), Some(2));
        assert_eq!(seen.get(), (Some(2), true));
        assert_eq!(closed.get(), Some(0));
    }
    assert_eq!(closed.get(), Some(1));
}

#[test]
fn publishes_before_notify() {
    let call = Rc::new(RefCell::new(None::<Box<dyn Fn()>>));
    let closed = Rc::new(Cell::new(Some(0)));
    let ran = Rc::new(Cell::new(false));
    let status = Rc::new(Cell::new(Ok(())));
    {
        let result = release::Store::try_init(
            Count::new(1),
            Reentry {
                callback: Rc::clone(&call),
                closed: Rc::clone(&closed),
                entered: Rc::clone(&ran),
                status: Rc::clone(&status),
            },
            (),
        );
        let pool = result.expect("the reentrant store must admit one slot");
        let pool = Rc::new(pool);
        let weak = Rc::downgrade(&pool);
        call.borrow_mut().replace(Box::new(move || {
            let pool = (weak.upgrade()).expect("live pool");
            {
                let vacancy =
                    (pool.vacancy()).expect("the slot must be published before its callback");
                {
                    assert!(pool.vacancy().is_none());
                    let claim = vacancy.reserve();
                    assert!(pool.vacancy().is_none());
                    drop(claim);
                }
            }
        }));

        {
            let vacancy =
                (pool.vacancy()).expect("the slot must be vacant before the first callback");
            {
                assert!(pool.vacancy().is_none());
                let claim = vacancy.reserve();
                assert!(pool.vacancy().is_none());
                drop(claim);
            }
        }
        assert!(ran.get());
        assert_eq!(status.get(), Ok(()));
        assert!(pool.vacancy().is_some());
        assert_eq!(closed.get(), Some(0));
    }
    assert_eq!(closed.get(), Some(1));
    assert_eq!(status.get(), Ok(()));
}
