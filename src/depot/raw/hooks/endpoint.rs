use std::cell::Cell;

use crate::depot::queue::{links, tagged};

/// Encodings used by one compact endpoint topology.
pub(in crate::depot) trait EndpointWord: Copy + Eq {
    const EMPTY: Self;
    fn node(index: links::Index) -> Self;
    fn raw(self) -> u32;
}

impl EndpointWord for u32 {
    const EMPTY: Self = links::Index::END;

    fn node(index: links::Index) -> Self {
        index.raw()
    }

    fn raw(self) -> u32 {
        self
    }
}

/// Borrowed hook cells retain their source's word representation.
pub(in crate::depot) struct EndpointHook<'a, W> {
    pub(in crate::depot) previous: &'a Cell<W>,
    pub(in crate::depot) next: &'a Cell<W>,
}

impl<'a> From<&'a links::Hook> for EndpointHook<'a, u32> {
    fn from(hook: &'a links::Hook) -> Self {
        Self {
            previous: &hook.previous,
            next: &hook.next,
        }
    }
}

/// One observed link and its resolved neighbor under the original owner loan.
pub(in crate::depot) struct Neighbor<'a, W, H = links::Chain> {
    link: W,
    target: Target<'a, W, H>,
}

enum Target<'a, W, H> {
    Node(EndpointHook<'a, W>),
    Head(&'a H),
}

impl<'a, W, H> Neighbor<'a, W, H> {
    pub(in crate::depot) fn node(link: W, hook: EndpointHook<'a, W>) -> Self {
        Self {
            link,
            target: Target::Node(hook),
        }
    }

    pub(in crate::depot) fn head(link: W, head: &'a H) -> Self {
        Self {
            link,
            target: Target::Head(head),
        }
    }
}

impl<W: EndpointWord> EndpointHook<'_, W> {
    /// # Safety
    /// The hook is exclusively vacant at `index`; head, endpoint and the
    /// optional adjacent node retain the same reciprocal lane and owner.
    pub(in crate::depot) unsafe fn push<const FRONT: bool, H: tagged::raw::Head>(
        self,
        head: &H,
        index: links::Index,
        endpoint: W,
        adjacent: Option<(links::Index, EndpointHook<'_, W>)>,
    ) {
        const { assert!(FRONT || H::TAIL) };
        let (outer, inner) = if FRONT {
            (self.previous, self.next)
        } else {
            (self.next, self.previous)
        };
        outer.set(endpoint);
        match adjacent {
            Some((adjacent, hook)) => {
                inner.set(W::node(adjacent));
                let neighbor = if FRONT { hook.previous } else { hook.next };
                neighbor.set(W::node(index));
            }
            None => {
                inner.set(endpoint);
                // SAFETY: the empty lane receives this exact first and last node.
                if FRONT {
                    if let Some(tail) = head.tail() {
                        tail.set_last(Some(index));
                    }
                } else {
                    unsafe { head.set_first(Some(index)) };
                }
            }
        }
        // SAFETY: the retained lane's adjacent edge was repaired above.
        if FRONT {
            unsafe { head.set_first(Some(index)) };
        } else if let Some(tail) = head.tail() {
            tail.set_last(Some(index));
        }
    }

    /// # Safety
    /// The hook is the exclusive live member at `index`; both neighbors
    /// resolve its unchanged reciprocal links in the same owner and lane.
    pub(in crate::depot) unsafe fn detach<H: tagged::raw::Head>(
        self,
        index: links::Index,
        previous: Neighbor<'_, W, H>,
        next: Neighbor<'_, W, H>,
    ) {
        let before = previous.link;
        let after = next.link;
        match previous.target {
            Target::Node(hook) => hook.next.set(after),
            // SAFETY: after the head is a live node or the same endpoint.
            Target::Head(head) => unsafe {
                head.set_first((after != before).then(|| links::Index::new_raw(after.raw())))
            },
        }
        match next.target {
            Target::Node(hook) => hook.previous.set(before),
            // SAFETY: before the tail is a live node or the same endpoint.
            Target::Head(head) => {
                if let Some(tail) = head.tail() {
                    tail.set_last(
                        (before != after).then(|| unsafe { links::Index::new_raw(before.raw()) }),
                    );
                }
            }
        }
        // SAFETY: the reciprocal neighbors have been repaired for this exact hook.
        unsafe { self.clear(index) };
    }

    /// # Safety
    /// The hook at `index` is exclusively detached from its original lane.
    pub(in crate::depot) unsafe fn clear(self, index: links::Index) {
        self.previous.set(W::node(index));
        self.next.set(W::EMPTY);
    }
}
