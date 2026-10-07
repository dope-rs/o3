pub mod common;

use std::{cell::Cell, collections::VecDeque, mem, ops::ControlFlow};

use common::{Counted, alloc::system::Tracker};
use o3::depot::{capacity::Count, heap::queue};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn keeps_callback_owners() {
    use common::depot::heap::observed::Priority;

    let compared = Cell::new(false);
    let drops = Cell::new(0);
    let values = Cell::new(Some(0));
    let compare = || {
        assert_eq!((drops.get(), values.get()), (0, Some(0)));
        compared.set(true);
    };
    let release = || drops.set(drops.get() + 1);
    let mut queue = queue::Min::try_new(Count::new(2)).expect("two residents allocate");
    for (priority, value) in [(10, 10), (5, 5)] {
        queue.vacant().expect("free slot").push_back(
            Priority::new(priority, &compare, &release),
            common::Owner {
                value,
                drops: Counted(&values),
            },
        );
    }
    assert!(compared.get());
    let removed = queue.min_entry().expect("new minimum").remove();
    assert_eq!((removed.0.value, removed.1.value), (5, 5));
    assert_eq!((drops.get(), values.get()), (0, Some(0)));
    assert_eq!(
        queue.front().map(|mut entry| entry.get_mut().value),
        Some(10)
    );
    drop(queue);
    assert_eq!((drops.get(), values.get()), (1, Some(1)));
    drop(removed);
    assert_eq!((drops.get(), values.get()), (2, Some(2)));
}

#[test]
fn owns_and_orders() {
    let (result, counts) = Tracker::measure(|| queue::Min::<u64, String>::try_new(Count::new(4)));
    let mut values = result.expect("the four-resident queue must allocate");
    assert_eq!(counts[0], 1);
    assert_ne!(counts[1], 0);
    assert_eq!(mem::size_of_val(&values), 6 * mem::size_of::<usize>());
    assert_eq!(
        mem::size_of::<queue::Vacant<'static, u64, String>>(),
        2 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<queue::Occupied<'static, u64, String>>(),
        2 * mem::size_of::<usize>()
    );
    let strings = [String::from("a"), String::from("b"), String::from("c")];
    {
        let (strings, counts) = Tracker::measure(|| {
            let [a, b, c] = strings;
            for (priority, value) in [(30, a), (10, b), (20, c)] {
                let entry = (values.vacant()).expect("three insertions must fit four residents");
                entry.push_back(priority, value);
            }
            let mut front = (values.front()).expect("three insertions must establish a FIFO front");
            assert_eq!((*front.priority(), front.get_mut().as_str()), (30, "a"));
            assert_eq!(values.min(), Some(&10));
            assert_eq!(values.min_entry().map(|entry| *entry.priority()), Some(10));
            assert_eq!(values.len(), 3);
            let front =
                (values.front()).expect("releasing the minimum borrow must retain the FIFO front");
            front.move_to_back();
            let mut front = (values.front()).expect("rotation must retain all three entries");
            assert_eq!(front.get_mut(), "b");
            let entry = (values.min_entry()).expect("three entries must have a minimum");
            assert_eq!(*entry.priority(), 10);
            let (priority, b) = entry.remove();
            assert_eq!((priority, b.as_str()), (10, "b"));
            let front =
                (values.front()).expect("removing one of three entries must leave a FIFO front");
            let (priority, c) = front.remove();
            assert_eq!((priority, c.as_str()), (20, "c"));
            let front = (values.front()).expect("one entry must remain after two removals");
            let (priority, a) = front.remove();
            assert_eq!((priority, a.as_str()), (30, "a"));
            [a, b, c]
        });
        assert_eq!(counts, [0; 4]);
        assert_eq!(strings, ["a", "b", "c"]);
    }
    assert!(values.is_empty());
}

#[test]
fn sweeps_only_admitted() {
    let mut values = queue::Min::try_new(Count::new(3)).expect("three residents allocate");
    for (priority, value) in [(30, 3), (10, 1), (20, 2)] {
        values
            .vacant()
            .expect("free slot")
            .push_back(priority, value);
    }
    let ((), counts) = Tracker::measure(|| {
        let pass = values
            .try_sweep(
                || Ok::<_, ()>(0),
                |seen, mut entry| {
                    assert_eq!(*entry.get_mut(), [3, 1, 2][seen]);
                    entry.move_to_back();
                    ControlFlow::<(), _>::Continue(seen + 1)
                },
            )
            .expect("bounded pass");
        assert_eq!(pass, Some(ControlFlow::Continue(3)));
        assert_eq!(values.min(), Some(&10));
        let pass = values
            .try_sweep(
                || Ok::<_, ()>(false),
                |second, entry| {
                    if second {
                        entry.move_to_back();
                        ControlFlow::Break(())
                    } else {
                        assert_eq!(entry.remove(), (30, 3));
                        ControlFlow::Continue(true)
                    }
                },
            )
            .expect("partial pass");
        assert_eq!(pass, Some(ControlFlow::Break(())));
        assert_eq!(values.len(), 2);
        let pass = values
            .try_sweep(
                || Ok::<_, ()>(0),
                |seen, entry| {
                    assert_eq!(entry.remove(), [(20, 2), (10, 1)][seen]);
                    ControlFlow::<(), _>::Continue(seen + 1)
                },
            )
            .expect("remaining pass");
        assert_eq!(pass, Some(ControlFlow::Continue(2)));
        assert_eq!(
            values.try_sweep(|| Err::<(), ()>(()), |(), _| ControlFlow::<()>::Break(())),
            Ok(None),
        );
    });
    assert_eq!(counts, [0; 4]);
}

#[test]
fn churn_matches_model() {
    let mut values = (queue::Min::<u64, usize>::try_new(Count::new(7)))
        .expect("heap storage must fit its declared domain");
    let mut model = VecDeque::<(u64, usize)>::new();
    for step in 0..2_000usize {
        match step % 5 {
            1 if model.len() > 1 => {
                let expected =
                    (model.pop_front()).expect("a model longer than one must have a front");
                model.push_back(expected);
                let front = (values.front()).expect("queue and model lengths must remain equal");
                front.move_to_back();
            }
            2 if !model.is_empty() => {
                let position = model
                    .iter()
                    .enumerate()
                    .min_by_key(|entry| (entry.1).0)
                    .map(|entry| entry.0);
                let expected = position.and_then(|position| model.remove(position));
                let entry = (values.min_entry()).expect("a nonempty queue must expose its minimum");
                assert_eq!(Some(*entry.priority()), expected.map(|entry| entry.0));
                let actual = Some(entry.remove());
                assert_eq!(actual, expected);
            }
            _ if (step % 5 != 0 || model.is_empty()) && model.len() < values.capacity() => {
                let entry = ((step * 17 + 3) as u64, step);
                let vacant = (values.vacant()).expect("the capacity guard must prove a vacancy");
                vacant.push_back(entry.0, entry.1);
                model.push_back(entry);
            }
            _ => {
                let expected = model.pop_front();
                let actual = values.front().map(|entry| entry.remove());
                assert_eq!(actual, expected);
            }
        }
        assert_eq!(
            values
                .front()
                .map(|mut front| (*front.priority(), *front.get_mut())),
            model.front().copied()
        );
        assert_eq!(values.len(), model.len());
        let room = model.len() < values.capacity();
        assert_eq!(values.vacant().is_some(), room);
    }
}

#[test]
fn drops_values_once() {
    let drops = Cell::new(Some(0));
    {
        let mut values = (queue::Min::try_new(Count::new(2)))
            .expect("heap storage must fit its declared domain");
        for priority in [2, 1] {
            let entry = (values.vacant()).expect("two insertions must fit two residents");
            entry.push_back(priority, Counted(&drops));
        }
        assert!(values.vacant().is_none());

        {
            let front = (values.front()).expect("two insertions must establish a FIFO front");
            let (_, removed) = front.remove();
            let entry = (values.vacant()).expect("removing one entry must create a vacancy");
            entry.push_back(3, Counted(&drops));
            while let Some(front) = values.front() {
                drop(front.remove());
            }
            assert_eq!(drops.get(), Some(2));
            assert_eq!(removed.0.get(), Some(2));
        }
        assert_eq!(drops.get(), Some(3));
        assert!(values.vacant().is_some());
        let entry = (values.vacant()).expect("clearing the queue must restore every vacancy");
        entry.push_back(0, Counted(&drops));
        assert_eq!(values.min(), Some(&0));
    }
    assert_eq!(drops.get(), Some(4));
}

#[test]
fn drops_sparse_residents() {
    use common::depot::model::drops::Tracked;

    let drops = [const { Cell::new(Some(0)) }; 4];
    let payloads = Cell::new(Some(0));
    {
        let mut queue = queue::Min::try_new(Count::new(4)).expect("queue allocation");
        for order in [3, 1, 2, 0] {
            queue.vacant().expect("free slot").push_back(
                Tracked::try_new(order, &drops).expect("priority counter"),
                Counted(&payloads),
            );
        }
        queue.front().expect("front").remove();
        queue.min_entry().expect("minimum").remove();
    }
    assert_eq!(drops.each_ref().map(Cell::get), [Some(1); 4]);
    assert_eq!(payloads.get(), Some(4));
}

#[test]
fn allocation_error() {
    use o3::depot::heap;

    let count = Count::new(3);
    let (result, activity) =
        Tracker::measure(|| Tracker::fail_after(0, || queue::Min::<u64, String>::try_new(count)));
    let (capacity, layout, error) = match (result)
        .err()
        .expect("allocation rejection must retain its exact context")
    {
        heap::Error::Allocation {
            capacity,
            layout,
            error,
        } => Some((capacity, layout, error)),
        _ => None,
    }
    .expect("allocation rejection must retain its exact context");
    assert!((capacity) == (count));
    assert_eq!(layout.align(), mem::align_of::<u64>());
    assert_eq!(error, o3::alloc::Exhausted);
    assert_eq!(activity, [0; 4]);
    let values = (queue::Min::<u64, String>::try_new(count))
        .expect("heap storage must fit its declared domain");

    let ((), activity) = Tracker::measure(move || assert_eq!(values.len(), 0));
    assert_eq!(activity, [0, 0, 1, layout.size()]);
}

#[test]
fn resident_limit_is_exact() {
    use std::alloc::Layout;

    use o3::depot::heap;

    type Wide = [u8; isize::MAX as usize / u32::MAX as usize + 1];

    let (result, activity) =
        Tracker::measure(|| Tracker::fail_after(0, || queue::Min::<(), Wide>::try_new(Count::MAX)));
    let (capacity, plane, entries, offsets, values, error) = match (result)
        .err()
        .expect("the third plane must overflow before allocation")
    {
        heap::Error::Layout {
            capacity,
            plane,
            entries,
            positions: offsets,
            residents: values,
            error,
        } => Some((capacity, plane, entries, offsets, values, error)),
        _ => None,
    }
    .expect("the third plane must overflow before allocation");
    assert!((capacity) == (Count::MAX));
    assert!((plane) == (heap::Plane::Residents));
    assert_eq!(entries, Layout::new::<u32>());
    assert_eq!(offsets, Layout::new::<u32>());
    assert!(values.size() >= mem::size_of::<Wide>());
    assert_eq!(error, o3::alloc::Overflow);
    assert_eq!(activity, [0; 4]);

    let align = entries.align().max(values.align());
    let bound = (isize::MAX as usize) & !(align - 1);
    let stride = entries.size() + offsets.size() + values.size();
    let count = Count::new((bound / stride) as u32);
    let (result, activity) =
        Tracker::measure(|| Tracker::fail_after(0, || queue::Min::<(), Wide>::try_new(count)));
    let (capacity, layout, error) = match (result)
        .err()
        .expect("the exact representable limit must reach allocation")
    {
        heap::Error::Allocation {
            capacity,
            layout,
            error,
        } => Some((capacity, layout, error)),
        _ => None,
    }
    .expect("the exact representable limit must reach allocation");
    assert!((capacity) == (count));
    assert_eq!(
        layout.size(),
        (stride * count.get()).next_multiple_of(align)
    );
    assert_eq!(layout.align(), align);
    assert_eq!(error, o3::alloc::Exhausted);
    assert_eq!(activity, [0; 4]);

    let next = Count::new(count.raw() + 1);
    let (result, activity) =
        Tracker::measure(|| Tracker::fail_after(0, || queue::Min::<(), Wide>::try_new(next)));
    assert!(matches!(result, Err(heap::Error::Layout {
        capacity, plane: heap::Plane::Residents, error: o3::alloc::Overflow, ..
    }) if capacity == next));
    assert_eq!(activity, [0; 4]);
}
