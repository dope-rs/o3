use std::num;

use o3::{
    depot,
    depot::{
        arena::linked::{head, slots},
        capacity,
    },
};

pub enum Operation<T> {
    Push(T),
    Pop,
}

pub struct Script<'a, 'b, T, const N: usize> {
    nodes: &'a depot::raw::Arena<T>,
    head: &'b mut head::List<()>,
    steps: [Operation<T>; N],
    peak: capacity::Count,
}

impl<'a, 'b, T, const N: usize> Script<'a, 'b, T, N> {
    pub fn head(&self) -> &head::List<()> {
        self.head
    }

    /// # Safety
    /// The topology and every retained node must belong to this exact pool.
    pub unsafe fn try_new(
        nodes: &'a depot::raw::Arena<T>,
        head: &'b mut head::List<()>,
        steps: [Operation<T>; N],
    ) -> Result<Self, Error> {
        let original = unsafe { nodes.iter(head) }.count();
        let mut current = original;
        let mut peak = original;
        for (index, step) in steps.iter().enumerate() {
            match step {
                Operation::Push(_) => current = current.checked_add(1).ok_or(Error::Overflow)?,
                Operation::Pop => {
                    current = current.checked_sub(1).ok_or(Error::Empty { index })?;
                }
            }
            peak = peak.max(current);
        }
        let peak = capacity::Count::new(u32::try_from(peak - original).map_err(Error::Capacity)?);
        Ok(Self {
            nodes,
            head,
            steps,
            peak,
        })
    }
}

unsafe impl<'a, T: 'a, const N: usize> depot::raw::LinkedBatch<'a, T, ()> for Script<'a, '_, T, N> {
    type Output = Result<(), Error>;
    type Error = (Self, capacity::Shortage);

    fn nodes(&self) -> &'a depot::raw::Arena<T> {
        self.nodes
    }

    fn count(&self) -> capacity::Count {
        self.peak
    }

    fn reject(self, error: capacity::Shortage) -> Self::Error {
        (self, error)
    }

    fn run(self, reserve: &mut slots::Reserve<'a, T, ()>) -> Self::Output {
        for (index, step) in self.steps.into_iter().enumerate() {
            match step {
                Operation::Push(value) => unsafe {
                    let mut cursor = self.nodes.cursor(self.head);
                    let claim = slots::Reserve::claim(depot::raw::Proof::new(&mut *reserve), value);
                    slots::Cursor::insert(depot::raw::Proof::new((&mut cursor, claim)));
                },
                Operation::Pop => {
                    let entry = unsafe { self.nodes.front_entry(self.head) }
                        .ok_or(Error::Empty { index })?;
                    let (_, slot) = entry.take();
                    slots::Reserve::recycle(unsafe {
                        depot::raw::Proof::new((&mut *reserve, slot))
                    });
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum Error {
    Empty { index: usize },
    Overflow,
    Capacity(num::TryFromIntError),
}
