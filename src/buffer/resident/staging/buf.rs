use std::{num, ops};

use storage::range;

use crate::{
    buffer,
    buffer::{
        Complete as _,
        bytes::window,
        hold, pool,
        pool::owners,
        resident,
        resident::staging::{Backing, Plan, Pool, Slot, reserve},
        storage,
        storage::regions,
        write::{self, consume, consume::Consume},
    },
    mem::rc,
};

/// One connection's bounded incremental receive owner.
#[repr(C)]
pub struct Buf<'d> {
    backing: State<'d>,
    head: u32,
    pub(in crate::buffer::resident::staging) pool: Pool<'d>,
}

impl<'d> TryFrom<&Pool<'d>> for Buf<'d> {
    type Error = rc::Error;

    fn try_from(pool: &Pool<'d>) -> Result<Self, Self::Error> {
        Ok(Self {
            pool: pool.try_clone()?,
            backing: State::Empty,
            head: 0,
        })
    }
}

impl<'d> Buf<'d> {
    pub const fn plan(&self) -> Plan {
        self.pool.plan()
    }

    pub fn len(&self) -> usize {
        self.backing.len() - self.head as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Writable bytes remaining in the acquired backing.
    pub fn spare(&self) -> usize {
        self.backing.capacity() - self.len()
    }

    pub fn as_slice(&self) -> &[u8] {
        let bytes = match &self.backing {
            State::Empty | State::Cold(_) => return &[],
            State::Live(Backing::Warm(cursor) | Backing::Accounted(cursor)) => cursor.as_slice(),
            State::Live(Backing::Local(state)) => state.as_slice(),
        };
        // SAFETY: each live backing retains its initialized prefix and bounded suffix cursor.
        unsafe { bytes.get_unchecked(self.head as usize..) }
    }

    pub fn clear(&mut self) {
        self.backing = State::Empty;
        self.head = 0;
    }

    pub fn try_reserve(&mut self, target: usize) -> Result<(), reserve::Error> {
        if target <= self.backing.capacity() {
            return Ok(());
        }
        consume::Error::check(target, self.plan().ceiling().get() as usize)
            .map_err(reserve::Error::Capacity)?;
        let target = num::NonZero::<u32>::MIN.saturating_add(target as u32 - 1);
        Reserve {
            buf: self,
            target,
            source: owners::Measured::slice(&[][..]),
        }
        .try_commit::<false>()
    }

    pub fn try_grow(&mut self) -> Result<(), reserve::Error> {
        let len = self.len();
        if len < self.backing.capacity() {
            return Ok(());
        }
        consume::Error::check(1, self.plan().ceiling().get() as usize - len)
            .map_err(reserve::Error::Capacity)?;
        let target = self.growth(
            num::NonZero::<u32>::MIN.saturating_add(len as u32),
            len as u32,
        );
        Reserve {
            buf: self,
            target,
            source: owners::Measured::slice(&[][..]),
        }
        .try_commit::<false>()
    }

    fn growth(&self, target: num::NonZero<u32>, current: u32) -> num::NonZero<u32> {
        let (warm, limit) = self.pool.plan().bounds();
        let current = num::NonZero::new(current).map_or(warm, |current| warm.max(current));
        window::Capacity::from(current)
            .grow_to(target.into(), limit.into())
            .nonzero()
    }
}

pub(in crate::buffer) struct Reserve<T, S> {
    pub(in crate::buffer::resident::staging) buf: T,
    pub(in crate::buffer::resident::staging) target: num::NonZero<u32>,
    pub(in crate::buffer::resident::staging) source: S,
}

impl<T, S> Reserve<T, S> {
    pub(in crate::buffer) fn into_parts(self) -> (T, num::NonZero<u32>, S) {
        (self.buf, self.target, self.source)
    }
}

impl<I: buffer::raw::SliceExt> Reserve<&mut Buf<'_>, owners::Measured<I>> {
    fn try_commit<const APPEND: bool>(self) -> Result<(), reserve::Error> {
        let Self {
            buf,
            target,
            source: src,
        } = self;
        let warm = matches!(&buf.backing, State::Live(Backing::Warm(_)));
        match &mut buf.backing {
            State::Live(Backing::Warm(cursor) | Backing::Accounted(cursor)) => {
                let source = if APPEND {
                    match pool::Span::try_extend(
                        Slot {
                            cursor: &mut *cursor,
                            head: &mut buf.head,
                        },
                        warm.then_some(buf.pool.warm()),
                        &src,
                    ) {
                        Ok(()) => return Ok(()),
                        Err(source) => Some(source),
                    }
                } else {
                    None
                };
                let next = Backing::try_fresh(Reserve {
                    buf: &buf.pool,
                    target,
                    source: (buf.as_slice(), src),
                })
                .map_err(|error| match source {
                    Some(source) => reserve::Error::Spill { source, error },
                    None => reserve::Error::Resident(error),
                })?;
                buf.backing = State::Live(next);
                buf.head = 0;
                Ok(())
            }
            State::Live(Backing::Local(snapshot)) => {
                let result = if APPEND {
                    window::Edit::try_staged(Reserve {
                        buf: (snapshot, &mut buf.head),
                        target,
                        source: src,
                    })
                } else {
                    // The strict target remains bound to this unchanged exclusive owner.
                    window::Edit::try_reserve(Reserve {
                        buf: (snapshot, &mut buf.head),
                        target,
                        source: (),
                    })
                };
                result
                    .map_err(resident::AllocError::from)
                    .map_err(reserve::Error::Resident)?;
                Ok(())
            }
            State::Cold(seed) => {
                let next = window::Fresh::try_staged(Reserve {
                    buf: *seed,
                    target,
                    source: (&[][..], src),
                })
                .map_err(resident::AllocError::from)
                .map_err(reserve::Error::Resident)?;
                buf.backing = State::Live(Backing::Local(next));
                Ok(())
            }
            State::Empty => {
                let next = if target <= buf.pool.plan().warm()
                    && let Some(cursor) = buf.pool.warm().acquire()
                {
                    Backing::Warm(pool::Span::staged(Reserve {
                        buf: cursor,
                        target,
                        source: src,
                    }))
                } else {
                    Backing::try_fresh(Reserve {
                        buf: &buf.pool,
                        target,
                        source: (&[][..], src),
                    })
                    .map_err(reserve::Error::Resident)?
                };
                buf.backing = State::Live(next);
                Ok(())
            }
        }
    }
}

impl write::Sink for Buf<'_> {
    type Error = reserve::Error;

    fn write_slice(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        let len = self.len();
        consume::Error::check(bytes.len(), self.plan().ceiling().get() as usize - len)
            .map_err(reserve::Error::Capacity)?;
        self.append(owners::Measured::slice(bytes), len)
    }

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error> {
        let len = self.len();
        let source =
            owners::Measured::try_slices(slices, self.plan().ceiling().get() as usize - len)
                .map_err(reserve::Error::Capacity)?;
        self.append(source, len)
    }
}

impl Buf<'_> {
    fn append<I: buffer::raw::SliceExt>(
        &mut self,
        source: owners::Measured<I>,
        len: usize,
    ) -> Result<(), reserve::Error> {
        let Some(add) = num::NonZero::new(source.len()) else {
            return Ok(());
        };
        let target = self.growth(
            num::NonZero::<u32>::MIN.saturating_add((add.get() + len - 1) as u32),
            self.backing.capacity() as u32,
        );
        Reserve {
            buf: self,
            target,
            source,
        }
        .try_commit::<true>()
    }
}

impl buffer::Residency for Buf<'_> {
    fn resident(&self) -> usize {
        self.backing.capacity()
    }
}

impl AsRef<[u8]> for Buf<'_> {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl Consume for Buf<'_> {
    fn remaining(&self) -> usize {
        self.len()
    }

    fn try_consume(&mut self, amount: usize) -> Result<(), consume::Error> {
        buffer::raw::Selection::try_new(self, amount).map(Self::advance)
    }

    fn consume_max(&mut self, max: usize) -> usize {
        let selected = buffer::raw::Selection::new(self, max);
        let amount = selected.len();
        Self::advance(selected);
        amount
    }
}

impl Buf<'_> {
    fn advance(prefix: buffer::raw::Selection<'_, Self>) {
        let (buf, amount) = prefix.into_parts();
        if amount == 0 {
            return;
        }
        buf.head += amount as u32;
        if let State::Live(Backing::Local(state)) = &buf.backing
            && buf.head == state.len()
        {
            buf.backing = State::Cold(state.seed());
            buf.head = 0;
        }
    }
}

// SAFETY: Buf retains the source and cursor validated by this exclusive branded prefix.
unsafe impl buffer::raw::Consume for Buf<'_> {
    fn consume<const MAX: u32>(prefix: buffer::raw::Consumed<'_, '_, Self, MAX>) {
        Self::advance(buffer::raw::Selection::from(prefix));
    }
}

impl<'d> buffer::Source for Buf<'d> {
    type View<'a>
        = buffer::View<'a, Self>
    where
        Self: 'a;

    fn remaining(&self) -> usize {
        self.len()
    }
}

impl buffer::Cursor for Buf<'_> {
    fn try_get<'a>(&'a self, range: ops::Range<usize>) -> Result<Self::View<'a>, range::Error> {
        self.view().try_get(range)
    }
}

impl buffer::Complete for Buf<'_> {
    fn view(&self) -> Self::View<'_> {
        buffer::View::new(self)
    }
}

// SAFETY: The immutable view retains the same backing range or copies those exact bytes.
unsafe impl<'d> hold::raw::Retain<'d> for Buf<'d> {
    fn charge<'a>(plan: &Self::View<'a>) -> hold::Charge
    where
        Self: 'a,
    {
        hold::Charge::from_view(plan)
    }

    fn try_retain<'a>(
        plan: &Self::View<'a>,
        budget: &resident::Budget<'d>,
    ) -> Result<resident::Shared<'d>, hold::Error>
    where
        Self: 'a,
    {
        use buffer::raw::Retain as _;
        use storage::regions::NonEmpty;

        let source = match &plan.source().backing {
            State::Empty | State::Cold(_) => return Ok(resident::Shared::new()),
            State::Live(source) => source,
        };
        let plan = buffer::View {
            source,
            bytes: plan.as_slice(),
        };
        let Some(view) = NonEmpty::new(&plan) else {
            return Ok(resident::Shared::new());
        };
        let owner = match view.source() {
            Backing::Warm(cursor) | Backing::Accounted(cursor) => {
                let Some(owner) = owners::Account::try_bind(cursor, budget)? else {
                    return resident::Exact::try_copy(budget, view.as_slice())
                        .map(resident::Exact::into_shared)
                        .map_err(hold::Error::Resident);
                };
                resident::State::Accounted(owner)
            }
            Backing::Local(state) => {
                let owner = state
                    .buf
                    .try_retain()
                    .map_err(resident::AllocError::Retain)?;
                resident::State::Resident(owner)
            }
        };
        Ok(resident::Shared {
            // SAFETY: the selected backing retains this exact initialized view.
            region: unsafe {
                regions::Value::new(owner, regions::Extent::from_bytes(view.as_slice()))
            },
        })
    }
}

const _: () = {
    assert!(size_of::<Reserve<&'static mut Buf<'static>, ()>>() == size_of::<usize>() * 2);
    assert!(size_of::<Option<Reserve<&'static mut Buf<'static>, ()>>>() == size_of::<usize>() * 2);
    assert!(
        size_of::<Reserve<&'static mut Buf<'static>, &'static [u8]>>() == size_of::<usize>() * 4
    );
    assert!(size_of::<Buf<'static>>() == size_of::<usize>() * 5);
    assert!(size_of::<Option<Buf<'static>>>() == size_of::<Buf<'static>>());
    assert!(std::mem::offset_of!(Buf<'static>, backing) == 0);
    assert!(std::mem::offset_of!(Buf<'static>, pool) == 4 * size_of::<usize>());
};

enum State<'d> {
    Empty,
    Cold(resident::Budget<'d>),
    Live(Backing<'d>),
}

impl State<'_> {
    fn len(&self) -> usize {
        match self {
            Self::Empty | Self::Cold(_) => 0,
            Self::Live(Backing::Warm(cursor) | Backing::Accounted(cursor)) => cursor.len(),
            Self::Live(Backing::Local(state)) => state.len() as usize,
        }
    }

    fn capacity(&self) -> usize {
        match self {
            Self::Empty | Self::Cold(_) => 0,
            Self::Live(backing) => backing.capacity().get() as usize,
        }
    }
}

const _: () = {
    assert!(size_of::<Backing<'static>>() == 3 * size_of::<usize>());
    assert!(size_of::<State<'static>>() == size_of::<Backing<'static>>());
};
