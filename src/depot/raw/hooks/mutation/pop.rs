use crate::depot::queue::links;

pub(in crate::depot) struct Pop<'a, C: crate::depot::raw::ChainCount> {
    pub(in crate::depot::raw) links: &'a links::Chain<C>,
    index: links::Index,
    next_index: u32,
    hook: &'a links::Hook,
    next: Option<&'a links::Hook>,
}

impl<'a, C: crate::depot::raw::ChainCount> Pop<'a, C> {
    /// # Safety
    /// `index` is the reciprocal front of these links in this exact store.
    pub(in crate::depot) unsafe fn at<H: crate::depot::raw::Hooks + ?Sized>(
        links: &'a links::Chain<C>,
        hooks: &'a H,
        index: links::Index,
    ) -> Self {
        // SAFETY: caller identifies the live front in this exact store.
        let hook = unsafe { hooks.hook_raw(index) };
        let next = hook.next.get();
        let tail = links::Index::decode(next).map(|index| unsafe { hooks.hook_raw(index) });
        Self {
            links,
            index,
            next_index: next,
            hook,
            next: tail,
        }
    }

    pub(in crate::depot) fn is_last(&self) -> bool {
        self.next.is_none()
    }
}

const _: () = assert!(std::mem::size_of::<Pop<'static, ()>>() == 4 * std::mem::size_of::<usize>());

// SAFETY: construction retains this exact chain's live hook until consumption.
unsafe impl<'a, C: crate::depot::raw::ChainCount> crate::depot::raw::Removal<'a, C> for Pop<'a, C> {
    fn links(&self) -> &'a links::Chain<C> {
        self.links
    }

    unsafe fn apply(self) -> links::Detached<'a> {
        (unsafe { self.links.parts() }).0.set(self.next_index);
        if let Some(next) = self.next {
            next.previous.set(links::Index::END);
        } else {
            (unsafe { self.links.parts() }).1.set(links::Index::END);
        }
        // SAFETY: committing these reciprocal links detached this exact retained hook.
        unsafe { links::Detached::new(self.hook, self.index) }
    }
}
