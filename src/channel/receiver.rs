use super::*;

impl<T> Receiver<T> {
    pub fn poll_recv(&self, cx: &mut Context<'_>) -> Poll<Option<T>> {
        poll_recv_inner(&self.inner, cx)
    }

    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        try_recv_inner(&self.inner)
    }

    pub fn is_terminated(&self) -> bool {
        let i = self.inner.borrow();
        i.closed && i.buf.is_empty()
    }

    pub fn close(&self) {
        close_inner(&self.inner, false)
    }

    pub fn is_closed(&self) -> bool {
        self.inner.borrow().closed
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        close_inner(&self.inner, true)
    }
}
