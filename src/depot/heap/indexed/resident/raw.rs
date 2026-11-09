use std::{marker, mem, ptr};

use crate::{
    alloc,
    base::marker::ThreadBound,
    depot::{
        self,
        heap::{
            self, indexed,
            indexed::resident::{
                Borrowed, Control, Entry, Min, Owned, Peek, Phase, Slot, Vacant, Value, Values,
                phase,
            },
        },
        index::Initialized,
        queue::links,
    },
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    Common,
    Control,
    Data,
    Active,
}

#[repr(C)]
pub(super) union Stored<T: Values> {
    scheduled: mem::ManuallyDrop<T::Scheduled>,
    idle: mem::ManuallyDrop<T::Idle>,
    control: mem::ManuallyDrop<T::Control>,
    data: mem::ManuallyDrop<T::Data>,
    active: mem::ManuallyDrop<T::Active>,
}

impl<T: Values> Stored<T> {
    fn new(value: Owned<T>) -> Self {
        match value {
            Value::Scheduled(value) => Self {
                scheduled: mem::ManuallyDrop::new(value),
            },
            Value::Idle(value) => Self {
                idle: mem::ManuallyDrop::new(value),
            },
            Value::Control(value) => Self {
                control: mem::ManuallyDrop::new(value),
            },
            Value::Data(value) => Self {
                data: mem::ManuallyDrop::new(value),
            },
            Value::Active(value) => Self {
                active: mem::ManuallyDrop::new(value),
            },
        }
    }

    // SAFETY: callers retain the exact initialized field selected by the owner.
    unsafe fn get<F>(&self) -> &F {
        unsafe { &*ptr::from_ref(self).cast::<F>() }
    }

    unsafe fn get_mut<F>(&mut self) -> &mut F {
        unsafe { &mut *ptr::from_mut(self).cast::<F>() }
    }

    unsafe fn take<F>(&mut self) -> F {
        unsafe { ptr::read(ptr::from_mut(self).cast::<F>()) }
    }

    unsafe fn drop_field<F>(&mut self) {
        unsafe { ptr::drop_in_place(ptr::from_mut(self).cast::<F>()) };
    }
}

pub(super) struct Node<T: Values> {
    pub(super) common: T::Common,
    hook: links::Hook,
    tag: Tag,
    value: Stored<T>,
}

impl<T: Values> Node<T> {
    pub(super) fn scheduled(index: usize, common: T::Common, value: T::Scheduled) -> Self {
        Self {
            common,
            hook: links::Hook::vacant(index as u32, links::Index::END),
            tag: Tag::Common,
            value: Stored::new(Value::Scheduled(value)),
        }
    }
}

pub(super) struct Core<T: Values> {
    pub(super) nodes: Box<[Node<T>]>,
    pub(super) control: links::Chain,
    pub(super) data: links::Chain,
    pub(super) active: u32,
    _thread: ThreadBound,
}

impl<T: Values> Core<T> {
    pub(super) fn new(nodes: Box<[Node<T>]>) -> Self {
        Self {
            nodes,
            control: links::Chain::new(),
            data: links::Chain::new(),
            active: 0,
            _thread: ThreadBound::NEW,
        }
    }
}

// SAFETY: the fixed original node allocation retains one distinct stable hook per slot.
unsafe impl<T: Values> depot::raw::Hooks for Core<T> {
    unsafe fn hook_raw(&self, index: links::Index) -> &links::Hook {
        unsafe { &self.nodes.get_unchecked(index.get()).hook }
    }
}

impl<'a, P: Ord, T: Values, H, F: Phase<T>, C: alloc::raw::Extent> Slot<'a, P, T, H, F, C> {
    // SAFETY: both disjoint loans retain the same owner's exact index, heap state and F field.
    unsafe fn new(
        order: &'a mut indexed::Min<P, C>,
        core: &'a mut Core<T>,
        index: links::Index,
        state: H,
    ) -> Self {
        Self {
            order,
            core,
            index,
            state,
            field: marker::PhantomData,
        }
    }

    pub fn index(&self) -> usize {
        self.index.get()
    }

    pub fn get(&self) -> (&T::Common, &F::Item) {
        // SAFETY: the retained slot admits exactly this resident and field.
        let node = unsafe { self.core.nodes.get_unchecked(self.index.get()) };
        (&node.common, unsafe { node.value.get() })
    }

    pub fn get_mut(&mut self) -> (&mut T::Common, &mut F::Item) {
        // SAFETY: the exclusive owner prevents field replacement during this loan.
        let node = unsafe { self.core.nodes.get_unchecked_mut(self.index.get()) };
        (&mut node.common, unsafe { node.value.get_mut() })
    }

    pub fn change<G: Phase<T> + super::Retained<H>>(
        self,
        value: G::Item,
    ) -> (Slot<'a, P, T, H, G, C>, F::Item)
    where
        H: Copy,
    {
        let state = self.state;
        self.transition::<G, _>(state, value)
    }

    fn transition<G: Phase<T>, N>(
        self,
        state: N,
        value: G::Item,
    ) -> (Slot<'a, P, T, N, G, C>, F::Item) {
        let value = Stored::new(G::value(value));
        if F::TAG != G::TAG {
            // SAFETY: F owns this slot's sole membership; the successor cannot share its hook.
            unsafe {
                match F::TAG {
                    Tag::Control => {
                        self.core.control.unlink(self.core, self.index);
                    }
                    Tag::Data => {
                        self.core.data.unlink(self.core, self.index);
                    }
                    Tag::Active => {
                        // SAFETY: this retained Active field contributes one distinct physical slot.
                        self.core.active -= 1;
                    }
                    Tag::Common => {}
                }
                match G::TAG {
                    Tag::Control => self.core.control.link_back(self.core, self.index),
                    Tag::Data => self.core.data.link_back(self.core, self.index),
                    Tag::Active => {
                        // SAFETY: this retained non-Active slot is excluded from active. The complete
                        // compact domain bounds active + 1 by its admitted u32 capacity.
                        self.core.active += 1;
                    }
                    Tag::Common => {}
                }
                self.core.nodes.get_unchecked_mut(self.index.get()).tag = G::TAG;
            }
        }
        // SAFETY: F is the sole initialized field; transition consumes it exactly once.
        let node = unsafe { self.core.nodes.get_unchecked_mut(self.index.get()) };
        let old = unsafe { node.value.take::<F::Item>() };
        node.value = value;
        (
            Slot {
                order: self.order,
                core: self.core,
                index: self.index,
                state,
                field: marker::PhantomData,
            },
            old,
        )
    }
}

impl<'a, P: Ord, T: Values, F: Phase<T>, C: alloc::raw::Extent> Slot<'a, P, T, usize, F, C> {
    pub fn replace(mut self, priority: P) -> (Self, P) {
        // SAFETY: this exclusive slot retains its exact initialized heap position.
        let entry = unsafe { heap::Entry::new(&mut *self.order, self.state, ()) };
        let (order, old) = indexed::Occupied { entry }.replace(priority);
        self.state = order.entry.position;
        (self, old)
    }

    pub fn retire(self, value: T::Idle) -> (Vacant<'a, P, T, phase::Idle, C>, F::Item, P) {
        // SAFETY: this exclusive slot retains its exact initialized heap position.
        let entry = unsafe { heap::Entry::new(&mut *self.order, self.state, ()) };
        let (vacant, priority) = indexed::Occupied { entry }.remove();
        let direct = vacant.entry.value;
        let (slot, old) = self.transition::<phase::Idle, _>(direct, value);
        (slot, old, priority)
    }
}

impl<'a, P: Ord, T: Values, F: Phase<T>, C: alloc::raw::Extent> Slot<'a, P, T, heap::Direct, F, C> {
    pub fn schedule(
        self,
        priority: P,
        value: T::Control,
    ) -> (Slot<'a, P, T, usize, phase::Control, C>, F::Item) {
        // SAFETY: the exclusive absent identity retains spare capacity in this exact heap.
        let vacant = unsafe { heap::Vacancy::new(&mut *self.order, self.state) };
        let entry = indexed::Vacant { entry: vacant }.insert(priority);
        let position = entry.entry.position;
        self.transition::<phase::Control, _>(position, value)
    }
}

pub(super) fn view<P: Ord, T: Values, C: alloc::raw::Extent>(
    owner: &Min<P, T, C>,
    index: usize,
) -> Option<(&T::Common, Borrowed<'_, T>)> {
    use heap::raw::Store as _;
    let node = owner.core.nodes.get(index)?;
    // SAFETY: the single tag and exact heap membership determine the sole initialized field.
    let value = unsafe {
        match node.tag {
            Tag::Common
                if *owner.order.positions().get_unchecked(index)
                    != <u32 as depot::raw::Link>::NONE =>
            {
                Value::Scheduled(node.value.get())
            }
            Tag::Common => Value::Idle(node.value.get()),
            Tag::Control => Value::Control(node.value.get()),
            Tag::Data => Value::Data(node.value.get()),
            Tag::Active => Value::Active(node.value.get()),
        }
    };
    Some((&node.common, value))
}

pub(super) fn peek<P: Ord, T: Values, C: alloc::raw::Extent>(
    owner: &Min<P, T, C>,
) -> Option<Peek<'_, P, T>> {
    let (index, priority) = owner.order.peek()?;
    // SAFETY: the actual heap root retains a represented full resident, whose tag can only
    // be Common/Scheduled or Control. Data and Active transitions keep the heap vacant.
    let node = unsafe { owner.core.nodes.get_unchecked(index) };
    let value = if node.tag == Tag::Control {
        None
    } else {
        Some(unsafe { node.value.get() })
    };
    Some((index, priority, &node.common, value))
}

unsafe fn vacant<'a, P: Ord, T: Values, C: alloc::raw::Extent, F: Phase<T>>(
    order: &'a mut indexed::Min<P, C>,
    core: &'a mut Core<T>,
    index: links::Index,
) -> Vacant<'a, P, T, F, C> {
    // SAFETY: caller retains the exact F field and its vacant heap position in this owner.
    let live: depot::raw::Proof<_, Initialized<indexed::Empty>> =
        unsafe { depot::raw::Proof::new(((order, core), index.position())) };
    let (order, core) = live.entry();
    unsafe { Slot::new(order.entry.owner, core, index, order.entry.value) }
}

pub(super) fn entry<P: Ord, T: Values, C: alloc::raw::Extent>(
    owner: &mut Min<P, T, C>,
    index: usize,
) -> Option<Entry<'_, P, T, C>> {
    let tag = owner.core.nodes.get(index)?.tag;
    // SAFETY: the fixed node allocation admits this exact compact heap position.
    let index = unsafe { links::Index::new_usize(index) };
    let Min { order, core } = owner;
    Some(match tag {
        // SAFETY: only vacant typed transitions can publish Data or Active fields.
        Tag::Data => Entry::Data(unsafe { vacant(order, core, index) }),
        Tag::Active => Entry::Active(unsafe { vacant(order, core, index) }),
        Tag::Common | Tag::Control => {
            // SAFETY: the original node owner retains this exact initialized reverse-map slot.
            let live: depot::raw::Proof<_, Initialized<()>> =
                unsafe { depot::raw::Proof::new(((order, core), index.position())) };
            let (order, core) = live.entry();
            match (tag, order) {
                (Tag::Control, indexed::Entry::Occupied(order)) => {
                    Entry::Control(Control::Full(unsafe {
                        Slot::new(order.entry.owner, core, index, order.entry.position)
                    }))
                }
                (Tag::Control, indexed::Entry::Vacant(order)) => {
                    Entry::Control(Control::Empty(unsafe {
                        Slot::new(order.entry.owner, core, index, order.entry.value)
                    }))
                }
                (_, indexed::Entry::Occupied(order)) => Entry::Scheduled(unsafe {
                    Slot::new(order.entry.owner, core, index, order.entry.position)
                }),
                (_, indexed::Entry::Vacant(order)) => Entry::Idle(unsafe {
                    Slot::new(order.entry.owner, core, index, order.entry.value)
                }),
            }
        }
    })
}

pub(super) fn control<P: Ord, T: Values, C: alloc::raw::Extent>(
    owner: &mut Min<P, T, C>,
) -> Option<Control<'_, P, T, C>> {
    let Min { order, core } = owner;
    let index = core.control.first()?;
    // SAFETY: this exact control queue retains the node's Control field and represented
    // heap position. Both full and empty Control are legitimate scheduled outcomes.
    let live: depot::raw::Proof<_, Initialized<()>> =
        unsafe { depot::raw::Proof::new(((order, core), index.position())) };
    let (order, core) = live.entry();
    Some(match order {
        indexed::Entry::Occupied(order) => Control::Full(unsafe {
            Slot::new(order.entry.owner, core, index, order.entry.position)
        }),
        indexed::Entry::Vacant(order) => {
            Control::Empty(unsafe { Slot::new(order.entry.owner, core, index, order.entry.value) })
        }
    })
}

pub(super) fn data<P: Ord, T: Values, C: alloc::raw::Extent>(
    owner: &mut Min<P, T, C>,
) -> Option<Vacant<'_, P, T, phase::Data, C>> {
    let Min { order, core } = owner;
    let index = core.data.first()?;
    // SAFETY: this exact queue admits only Data fields; their owned transitions retain
    // the same vacant heap slot until this exclusive source loan is consumed.
    Some(unsafe { vacant(order, core, index) })
}

pub(super) fn clear<P: Ord, T: Values, C: alloc::raw::Extent>(
    owner: &mut Min<P, T, C>,
    mut convert: impl FnMut(&mut T::Common, T::Scheduled) -> T::Idle,
) {
    use heap::raw::Store as _;
    for (node, &position) in owner.core.nodes.iter_mut().zip(owner.order.positions()) {
        if position != <u32 as depot::raw::Link>::NONE && node.tag == Tag::Common {
            // SAFETY: Common plus full membership owns the Scheduled field. Control fields
            // remain untouched; the subsequent heap clear changes only their deadline phase.
            let old = unsafe { node.value.take::<T::Scheduled>() };
            node.value = Stored::new(Value::Idle(convert(&mut node.common, old)));
        }
    }
    owner.order.clear();
}

pub(super) fn destroy<P: Ord, T: Values, C: alloc::raw::Extent>(owner: &mut Min<P, T, C>) {
    use heap::raw::Store as _;
    for (node, &position) in owner.core.nodes.iter_mut().zip(owner.order.positions()) {
        // SAFETY: terminal ownership retains the sole field selected by tag and membership;
        // each payload is dropped once, then normal field drop releases Common and storage.
        unsafe {
            match node.tag {
                Tag::Common if position != <u32 as depot::raw::Link>::NONE => {
                    node.value.drop_field::<T::Scheduled>()
                }
                Tag::Common => node.value.drop_field::<T::Idle>(),
                Tag::Control => node.value.drop_field::<T::Control>(),
                Tag::Data => node.value.drop_field::<T::Data>(),
                Tag::Active => node.value.drop_field::<T::Active>(),
            }
        }
    }
}
