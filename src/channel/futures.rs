use super::*;

impl<T> Unpin for Send<'_, T> {}

impl<T> Future for Send<'_, T> {
    type Output = Result<(), SendError<T>>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Some(value) = self.value.take() else {
            return Poll::Ready(Ok(()));
        };
        match self.sender.poll_ready(cx) {
            Poll::Ready(Ok(())) => match self.sender.start_send(value) {
                Ok(()) => Poll::Ready(Ok(())),
                Err(err) => Poll::Ready(Err(err)),
            },
            Poll::Ready(Err(_)) => Poll::Ready(Err(SendError(value))),
            Poll::Pending => {
                self.value = Some(value);
                Poll::Pending
            }
        }
    }
}
