use super::*;

impl<T> Sender<T> {
    pub fn try_send(&self, value: T) -> Result<(), SendError<T>> {
        try_send_inner(&self.inner, value)
    }

    pub fn unbounded_send(&self, value: T) -> Result<(), SendError<T>> {
        self.try_send(value)
    }

    pub fn send(&self, value: T) -> Send<'_, T> {
        Send {
            sender: self,
            value: Some(value),
        }
    }

    pub fn poll_ready(&self, cx: &mut Context<'_>) -> Poll<Result<(), ClosedError>> {
        poll_ready_inner(&self.inner, cx)
    }

    pub fn start_send(&self, value: T) -> Result<(), SendError<T>> {
        self.try_send(value)
    }

    pub fn poll_flush(&self, _cx: &mut Context<'_>) -> Poll<Result<(), ClosedError>> {
        Poll::Ready(Ok(()))
    }

    pub fn poll_close(&self, _cx: &mut Context<'_>) -> Poll<Result<(), ClosedError>> {
        close_inner(&self.inner, false);
        Poll::Ready(Ok(()))
    }

    pub fn close(&self) {
        close_inner(&self.inner, false)
    }

    pub fn close_channel(&self) {
        self.close()
    }

    pub fn is_closed(&self) -> bool {
        self.inner.borrow().closed
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        close_inner(&self.inner, false)
    }
}
