use super::CapacityError;

/// Reports the logical byte prefix that an exclusive owner may consume.
pub trait PrefixLength {
    fn prefix_len(&self) -> usize;
}

/// Proof that `amount` fits one exclusively borrowed target prefix.
///
/// The monomorphized commit operation may remain a zero-sized function item.
#[must_use]
pub struct ValidatedPrefix<'a, T: ?Sized, F> {
    target: &'a mut T,
    amount: usize,
    commit: F,
}

impl<'a, T: PrefixLength + ?Sized, F> ValidatedPrefix<'a, T, F> {
    pub fn try_new(target: &'a mut T, amount: usize, commit: F) -> Result<Self, CapacityError> {
        let available = target.prefix_len();
        if amount > available {
            return Err(CapacityError::new(amount, available));
        }
        Ok(Self {
            target,
            amount,
            commit,
        })
    }

    /// Proves the largest prefix no longer than `requested`.
    pub fn up_to(target: &'a mut T, requested: usize, commit: F) -> Self {
        let amount = requested.min(target.prefix_len());
        Self {
            target,
            amount,
            commit,
        }
    }

    /// Proves the target's complete current prefix.
    pub fn all(target: &'a mut T, commit: F) -> Self {
        let amount = target.prefix_len();
        Self {
            target,
            amount,
            commit,
        }
    }

    pub const fn len(&self) -> usize {
        self.amount
    }

    pub const fn is_empty(&self) -> bool {
        self.amount == 0
    }
}

impl<T: ?Sized, F: FnOnce(&mut T, usize)> ValidatedPrefix<'_, T, F> {
    /// Applies the validated mutation exactly once.
    pub fn commit(self) {
        (self.commit)(self.target, self.amount);
    }
}

macro_rules! consume_prefix_api {
    ($commit:expr) => {
        pub fn try_consume_prefix(
            &mut self,
            amount: usize,
        ) -> Result<
            $crate::buffer::ValidatedPrefix<'_, Self, impl FnOnce(&mut Self, usize)>,
            $crate::buffer::CapacityError,
        > {
            $crate::buffer::ValidatedPrefix::try_new(self, amount, $commit)
        }

        pub fn consume_prefix_up_to(&mut self, requested: usize) -> usize {
            let prefix = $crate::buffer::ValidatedPrefix::up_to(self, requested, $commit);
            let amount = prefix.len();
            prefix.commit();
            amount
        }

        pub fn consume_all(&mut self) -> usize {
            let prefix = $crate::buffer::ValidatedPrefix::all(self, $commit);
            let amount = prefix.len();
            prefix.commit();
            amount
        }
    };
}

pub(crate) use consume_prefix_api;
