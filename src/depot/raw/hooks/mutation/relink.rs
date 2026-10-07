use crate::depot::queue::links;

pub(in crate::depot) struct Relink<'a> {
    source: &'a links::Chain<()>,
    target: &'a links::Chain<()>,
    edge: links::Edge,
    hook: &'a links::Hook,
    previous: Option<&'a links::Hook>,
    next: Option<&'a links::Hook>,
    tail: u32,
    tail_hook: Option<&'a links::Hook>,
}

impl<'a> Relink<'a> {
    /// # Safety
    /// The target is not `index`; all loaded source and target edges are reciprocal.
    pub(in crate::depot) unsafe fn new<H: crate::depot::raw::Hooks + ?Sized>(
        source: &'a links::Chain<()>,
        target: &'a links::Chain<()>,
        hooks: &'a H,
        index: links::Index,
    ) -> Self {
        // SAFETY: caller guarantees index is a live source hook and that the
        // target does not alias it.
        let hook = unsafe { hooks.hook_raw(index) };
        let previous = hook.previous.get();
        let next = hook.next.get();
        let prior = optional_hook(hooks, previous);
        let after = optional_hook(hooks, next);
        let tail = (unsafe { target.parts() }).1.get();
        let last = optional_hook(hooks, tail);
        Self {
            source,
            target,
            edge: links::Edge::new(index, previous, next),
            hook,
            previous: prior,
            next: after,
            tail,
            tail_hook: last,
        }
    }

    pub(in crate::depot) fn commit(self) {
        if let Some(previous) = self.previous {
            previous.next.set(self.edge.links().1);
        } else {
            (unsafe { self.source.parts() }).0.set(self.edge.links().1);
        }
        if let Some(next) = self.next {
            next.previous.set(self.edge.links().0);
        } else {
            (unsafe { self.source.parts() }).1.set(self.edge.links().0);
        }
        self.hook.previous.set(self.tail);
        self.hook.next.set(links::Index::END);
        if let Some(tail) = self.tail_hook {
            tail.next.set(self.edge.key().raw());
        } else {
            (unsafe { self.target.parts() })
                .0
                .set(self.edge.key().raw());
        }
        (unsafe { self.target.parts() })
            .1
            .set(self.edge.key().raw());
    }
}

fn optional_hook<H: crate::depot::raw::Hooks + ?Sized>(
    hooks: &H,
    index: u32,
) -> Option<&'_ links::Hook> {
    // SAFETY: relink retains every decoded edge's live hook in this exact store.
    links::Index::decode(index).map(|index| unsafe { hooks.hook_raw(index) })
}
