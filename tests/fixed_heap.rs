use o3::collections::FixedHeap;

struct NotOrd;

#[test]
fn storage_operations_do_not_require_ordering() {
    let mut heap = FixedHeap::<NotOrd>::with_capacity(1);
    assert_eq!(heap.capacity(), 1);
    assert_eq!(heap.len(), 0);
    assert!(heap.peek().is_none());
    heap.clear();
}

#[test]
fn fixed_heap_orders_without_growing() {
    let mut heap = FixedHeap::with_capacity(3);
    assert_eq!(heap.capacity(), 3);
    assert_eq!(heap.push(2), Ok(()));
    assert_eq!(heap.push(1), Ok(()));
    assert_eq!(heap.push(3), Ok(()));
    assert_eq!(heap.push(4), Err(4));
    assert_eq!(heap.pop_if(|value| *value < 3), None);
    assert_eq!(heap.peek(), Some(&3));
    assert_eq!(heap.pop_if(|value| *value == 3), Some(3));
    assert_eq!(heap.pop(), Some(2));
    assert_eq!(heap.pop(), Some(1));
    assert_eq!(heap.pop(), None);
}

#[test]
fn fixed_heap_drops_every_value() {
    use std::cell::Cell;
    use std::cmp::Ordering;
    use std::rc::Rc;

    #[derive(Debug)]
    struct Value(u8, Rc<Cell<usize>>);
    impl PartialEq for Value {
        fn eq(&self, other: &Self) -> bool {
            self.0 == other.0
        }
    }
    impl Eq for Value {}
    impl PartialOrd for Value {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
            Some(self.cmp(other))
        }
    }
    impl Ord for Value {
        fn cmp(&self, other: &Self) -> Ordering {
            self.0.cmp(&other.0)
        }
    }
    impl Drop for Value {
        fn drop(&mut self) {
            self.1.set(self.1.get() + 1);
        }
    }

    let drops = Rc::new(Cell::new(0));
    {
        let mut heap = FixedHeap::with_capacity(3);
        heap.push(Value(1, drops.clone()))
            .expect("three-slot heap must accept its first value");
        heap.push(Value(3, drops.clone()))
            .expect("three-slot heap must accept its second value");
        heap.push(Value(2, drops.clone()))
            .expect("three-slot heap must accept its third value");
        drop(heap.pop());
    }
    assert_eq!(drops.get(), 3);
}
