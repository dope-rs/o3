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
    /// The distinct source and target chains are reciprocal in one store; head and tail resolve their exact endpoints until commit.
    pub(in crate::depot) unsafe fn new(
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
