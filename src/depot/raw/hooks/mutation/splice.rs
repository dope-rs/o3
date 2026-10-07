use crate::depot::queue::links;

pub(in crate::depot) struct Splice<'a> {
    target: &'a links::Chain<()>,
    source: &'a links::Chain<()>,
    source_head: u32,
    source_tail: u32,
    source_head_hook: &'a links::Hook,
    target_tail: u32,
    target_tail_hook: Option<&'a links::Hook>,
}

impl<'a> Splice<'a> {
    /// # Safety
    /// `source` is non-empty, distinct from `target`, and both endpoint pairs
    /// are reciprocal in `hooks`.
    pub(in crate::depot) unsafe fn new<H: crate::depot::raw::Hooks + ?Sized>(
        target: &'a links::Chain<()>,
        source: &'a links::Chain<()>,
        hooks: &'a H,
    ) -> Self {
        let dst_tail = (unsafe { target.parts() }).1.get();
        let (src_head, src_tail) = (
            (unsafe { source.parts() }).0.get(),
            (unsafe { source.parts() }).1.get(),
        );
        // SAFETY: caller guarantees the nonempty source head is a live endpoint.
        let src_hook = unsafe { hooks.hook_raw(links::Index::new_raw(src_head)) };
        let dst_hook = if dst_tail == links::Index::END {
            None
        } else {
            // SAFETY: caller guarantees the target's non-END tail is live and
            // reciprocal in this exact store.
            Some(unsafe { hooks.hook_raw(links::Index::new_raw(dst_tail)) })
        };
        Self {
            target,
            source,
            source_head: src_head,
            source_tail: src_tail,
            source_head_hook: src_hook,
            target_tail: dst_tail,
            target_tail_hook: dst_hook,
        }
    }

    /// # Safety
    /// The distinct source and target chains are reciprocal in one store; head and tail resolve their exact endpoints until commit.
    pub(in crate::depot) unsafe fn resolved(
        target: &'a links::Chain,
        source: &'a links::Chain,
        head: &'a links::Hook,
        tail: Option<&'a links::Hook>,
    ) -> Self {
        let (head_idx, tail_idx) = (
            (unsafe { source.parts() }).0.get(),
            (unsafe { source.parts() }).1.get(),
        );
        Self {
            target,
            source,
            source_head: head_idx,
            source_tail: tail_idx,
            source_head_hook: head,
            target_tail: (unsafe { target.parts() }).1.get(),
            target_tail_hook: tail,
        }
    }

    pub(in crate::depot) fn commit(self) {
        if let Some(tail) = self.target_tail_hook {
            tail.next.set(self.source_head);
            self.source_head_hook.previous.set(self.target_tail);
        } else {
            (unsafe { self.target.parts() }).0.set(self.source_head);
        }
        (unsafe { self.target.parts() }).1.set(self.source_tail);
        (unsafe { self.source.parts() }).0.set(links::Index::END);
        (unsafe { self.source.parts() }).1.set(links::Index::END);
    }
}
