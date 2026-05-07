use std::{
    cell::RefCell,
    collections::VecDeque,
    fmt,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

mod futures;
mod receiver;
mod sender;

#[derive(Debug)]
pub struct SendError<T>(pub T);

impl<T> fmt::Display for SendError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("channel receiver dropped before send")
    }
}

impl<T: fmt::Debug> std::error::Error for SendError<T> {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClosedError;

impl fmt::Display for ClosedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("channel is closed")
    }
}

impl std::error::Error for ClosedError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TryRecvError {
    Empty,
    Closed,
}

impl fmt::Display for TryRecvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("channel is empty"),
            Self::Closed => f.write_str("channel is closed"),
        }
    }
}

impl std::error::Error for TryRecvError {}

pub struct Sender<T> {
    pub(super) inner: Rc<RefCell<Inner<T>>>,
}

pub struct Receiver<T> {
    pub(super) inner: Rc<RefCell<Inner<T>>>,
}

pub struct Send<'a, T> {
    pub(super) sender: &'a Sender<T>,
    pub(super) value: Option<T>,
}

pub(super) struct Inner<T> {
    pub(super) buf: VecDeque<T>,
    pub(super) cap: Option<usize>,
    pub(super) rx_waker: Option<Waker>,
    pub(super) tx_waker: Option<Waker>,
    pub(super) closed: bool,
}

pub(super) fn wake(w: Option<Waker>) {
    if let Some(w) = w {
        w.wake();
    }
}

pub(super) fn close_inner<T>(inner: &Rc<RefCell<Inner<T>>>, clear: bool) {
    let (rx, tx) = {
        let mut i = inner.borrow_mut();
        i.closed = true;
        if clear {
            i.buf.clear();
        }
        (i.rx_waker.take(), i.tx_waker.take())
    };
    wake(rx);
    wake(tx);
}

pub(super) fn poll_ready_inner<T>(
    inner: &Rc<RefCell<Inner<T>>>,
    cx: &mut Context<'_>,
) -> Poll<Result<(), ClosedError>> {
    let mut i = inner.borrow_mut();
    if i.closed {
        return Poll::Ready(Err(ClosedError));
    }
    let Some(cap) = i.cap else {
        return Poll::Ready(Ok(()));
    };
    if i.buf.len() < cap {
        Poll::Ready(Ok(()))
    } else {
        i.tx_waker = Some(cx.waker().clone());
        Poll::Pending
    }
}

pub(super) fn try_send_inner<T>(
    inner: &Rc<RefCell<Inner<T>>>,
    value: T,
) -> Result<(), SendError<T>> {
    let rx = {
        let mut i = inner.borrow_mut();
        if i.closed {
            return Err(SendError(value));
        }
        if let Some(cap) = i.cap
            && i.buf.len() >= cap
        {
            return Err(SendError(value));
        }
        i.buf.push_back(value);
        i.rx_waker.take()
    };
    wake(rx);
    Ok(())
}

pub(super) fn poll_recv_inner<T>(
    inner: &Rc<RefCell<Inner<T>>>,
    cx: &mut Context<'_>,
) -> Poll<Option<T>> {
    let (value, tx, closed) = {
        let mut i = inner.borrow_mut();
        match i.buf.pop_front() {
            Some(v) => (Some(v), i.tx_waker.take(), i.closed),
            None => {
                if !i.closed {
                    i.rx_waker = Some(cx.waker().clone());
                }
                (None, None, i.closed)
            }
        }
    };
    wake(tx);
    match value {
        Some(v) => Poll::Ready(Some(v)),
        None if closed => Poll::Ready(None),
        None => Poll::Pending,
    }
}

pub(super) fn try_recv_inner<T>(inner: &Rc<RefCell<Inner<T>>>) -> Result<T, TryRecvError> {
    let (res, tx) = {
        let mut i = inner.borrow_mut();
        match i.buf.pop_front() {
            Some(v) => (Ok(v), i.tx_waker.take()),
            None if i.closed => (Err(TryRecvError::Closed), None),
            None => (Err(TryRecvError::Empty), None),
        }
    };
    wake(tx);
    res
}

pub fn unbounded<T>() -> (Sender<T>, Receiver<T>) {
    let inner = Rc::new(RefCell::new(Inner {
        buf: VecDeque::new(),
        cap: None,
        rx_waker: None,
        tx_waker: None,
        closed: false,
    }));
    (
        Sender {
            inner: inner.clone(),
        },
        Receiver { inner },
    )
}

pub fn channel<T>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    assert!(capacity > 0, "capacity must be > 0");
    let inner = Rc::new(RefCell::new(Inner {
        buf: VecDeque::with_capacity(capacity),
        cap: Some(capacity),
        rx_waker: None,
        tx_waker: None,
        closed: false,
    }));
    (
        Sender {
            inner: inner.clone(),
        },
        Receiver { inner },
    )
}
