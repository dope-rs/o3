use std::{collections, ops};

use crate::buffer::{self, bytes};

pub(super) mod ring;

pub use ring::Ring;

/// A byte-length-aware queue of independently owned segments.
/// It tracks ordering and aggregate length while callers retain
/// protocol-specific byte and segment limits.
#[derive(Debug)]
pub struct Segments<T> {
    segments: collections::VecDeque<T>,
    len: usize,
}

#[derive(Debug)]
/// A logical read cursor over a queue of independently owned segments.
pub struct Cursor<T> {
    queue: Segments<T>,
    front_offset: usize,
}

impl<T> Segments<T> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            segments: collections::VecDeque::new(),
            len: 0,
        }
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    fn back(&self) -> Option<&T> {
        self.segments.back()
    }

    fn clear(&mut self) {
        self.segments.clear();
        self.len = 0;
    }
}

impl<T: AsRef<[u8]>> Segments<T> {
    /// Appends without copying. Empty segments are discarded.
    /// On length overflow, the segment is returned and the queue is unchanged.
    pub fn try_push_back(&mut self, segment: T) -> Result<(), T> {
        let segment_len = segment.as_ref().len();
        if segment_len == 0 {
            return Ok(());
        }
        let Some(len) = self.len.checked_add(segment_len) else {
            return Err(segment);
        };
        self.segments.push_back(segment);
        self.len = len;
        Ok(())
    }

    /// Mutates the last segment after reserving its maximum permitted growth.
    ///
    /// Length tracking is restored from the actual delta during unwinding.
    fn try_mutate_back<R>(
        &mut self,
        additional: usize,
        mutate: impl FnOnce(&mut T) -> R,
    ) -> Option<R> {
        let total_ceiling = self.len.checked_add(additional)?;
        let segment = self.segments.back_mut()?;
        let before = segment.as_ref().len();
        let segment_ceiling = before.checked_add(additional)?;
        let mutation = SegmentMutation {
            segment,
            total: &mut self.len,
            total_ceiling,
            segment_ceiling,
            before,
        };
        Some(mutate(mutation.segment))
    }

    pub fn range_available(&self, front_offset: usize, offset: usize, len: usize) -> bool {
        if !self.front_offset_valid(front_offset) {
            return false;
        }
        offset.checked_add(len).is_some_and(|end| end <= self.len)
    }

    /// Visits a logical range without materializing it.
    /// `front_offset` is consumed physical prefix; `offset` starts from the
    /// remaining logical bytes.
    pub fn for_each_range(
        &self,
        front_offset: usize,
        offset: usize,
        len: usize,
        mut visit: impl FnMut(&[u8]),
    ) -> bool {
        if !self.range_available(front_offset, offset, len) {
            return false;
        }
        if len == 0 {
            return true;
        }
        let Some(mut skip) = front_offset.checked_add(offset) else {
            return false;
        };
        let mut remaining = len;
        for segment in &self.segments {
            let bytes = segment.as_ref();
            if skip >= bytes.len() {
                skip -= bytes.len();
                continue;
            }
            let bytes = &bytes[skip..];
            let take = remaining.min(bytes.len());
            visit(&bytes[..take]);
            remaining -= take;
            skip = 0;
            if remaining == 0 {
                return true;
            }
        }
        false
    }

    pub fn copy_range_into(&self, front_offset: usize, offset: usize, output: &mut [u8]) -> bool {
        let mut written = 0;
        let copied = self.for_each_range(front_offset, offset, output.len(), |bytes| {
            let end = written + bytes.len();
            output[written..end].copy_from_slice(bytes);
            written = end;
        });
        debug_assert!(!copied || written == output.len());
        copied
    }

    fn extend_range(
        &self,
        front_offset: usize,
        offset: usize,
        len: usize,
        output: &mut Vec<u8>,
    ) -> bool {
        self.for_each_range(front_offset, offset, len, |bytes| {
            output.extend_from_slice(bytes);
        })
    }

    /// Returns the owner and physical range when a logical range is contiguous.
    pub fn contiguous_segment(
        &self,
        front_offset: usize,
        offset: usize,
        len: usize,
    ) -> Option<(&T, ops::Range<usize>)> {
        if len == 0 || !self.range_available(front_offset, offset, len) {
            return None;
        }
        let mut skip = front_offset.checked_add(offset)?;
        for segment in &self.segments {
            let segment_len = segment.as_ref().len();
            if skip >= segment_len {
                skip -= segment_len;
                continue;
            }
            return (len <= segment_len - skip).then_some((segment, skip..skip + len));
        }
        None
    }

    fn consume_front_up_to(
        &mut self,
        front_offset: &mut usize,
        requested: usize,
        mut removed: impl FnMut(T),
    ) -> usize {
        if !self.front_offset_valid(*front_offset) {
            return 0;
        }
        let mut amount = requested.min(self.len);
        let target = amount;
        while amount != 0 {
            let Some(segment) = self.segments.front() else {
                break;
            };
            let available = segment.as_ref().len() - *front_offset;
            if amount < available {
                *front_offset += amount;
                self.len -= amount;
                return target;
            }
            amount -= available;
            self.len -= available;
            let Some(segment) = self.segments.pop_front() else {
                break;
            };
            *front_offset = 0;
            removed(segment);
        }
        target - amount
    }

    fn front_offset_valid(&self, front_offset: usize) -> bool {
        match self.segments.front() {
            Some(segment) => front_offset < segment.as_ref().len(),
            None => front_offset == 0 && self.is_empty(),
        }
    }
}

impl Segments<bytes::Bytes<bytes::Retained>> {
    /// Consumes a prefix by advancing a partial front segment in place.
    pub fn try_consume_front(&mut self, mut amount: usize) -> bool {
        if amount > self.len {
            return false;
        }
        while amount != 0 {
            let Some(segment) = self.segments.front() else {
                return false;
            };
            let available = segment.as_ref().len();
            if amount < available {
                let Some(segment) = self.segments.front_mut() else {
                    return false;
                };
                if !segment.try_advance(amount) {
                    return false;
                }
                self.len -= amount;
                return true;
            }
            amount -= available;
            self.len -= available;
            self.segments.pop_front();
        }
        true
    }
}

impl<T> Default for Segments<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> buffer::PrefixLength for Segments<T> {
    fn prefix_len(&self) -> usize {
        self.len()
    }
}

impl<T> Cursor<T> {
    #[must_use]
    const fn new() -> Self {
        Self {
            queue: Segments::new(),
            front_offset: 0,
        }
    }

    pub const fn len(&self) -> usize {
        self.queue.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn segment_count(&self) -> usize {
        self.queue.segment_count()
    }

    pub fn back(&self) -> Option<&T> {
        self.queue.back()
    }

    pub fn clear(&mut self) {
        self.queue.clear();
        self.front_offset = 0;
    }
}

impl<T: AsRef<[u8]>> Cursor<T> {
    pub fn try_push_back(&mut self, segment: T) -> Result<(), T> {
        self.queue.try_push_back(segment)
    }

    pub fn try_mutate_back<R>(
        &mut self,
        additional: usize,
        mutate: impl FnOnce(&mut T) -> R,
    ) -> Option<R> {
        self.queue.try_mutate_back(additional, mutate)
    }

    pub fn range_available(&self, offset: usize, len: usize) -> bool {
        self.queue.range_available(self.front_offset, offset, len)
    }

    pub fn for_each_range(&self, offset: usize, len: usize, visit: impl FnMut(&[u8])) -> bool {
        self.queue
            .for_each_range(self.front_offset, offset, len, visit)
    }

    pub fn copy_range_into(&self, offset: usize, output: &mut [u8]) -> bool {
        self.queue
            .copy_range_into(self.front_offset, offset, output)
    }

    pub fn contiguous_segment(&self, offset: usize, len: usize) -> Option<(&T, ops::Range<usize>)> {
        self.queue
            .contiguous_segment(self.front_offset, offset, len)
    }

    pub fn extend_range(&self, offset: usize, len: usize, output: &mut Vec<u8>) -> bool {
        self.queue
            .extend_range(self.front_offset, offset, len, output)
    }

    pub fn consume_prefix_up_to(&mut self, requested: usize, removed: impl FnMut(T)) -> usize {
        self.queue
            .consume_front_up_to(&mut self.front_offset, requested, removed)
    }
}

impl<T> Default for Cursor<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> buffer::PrefixLength for Cursor<T> {
    fn prefix_len(&self) -> usize {
        self.len()
    }
}

struct SegmentMutation<'a, T: AsRef<[u8]>> {
    segment: &'a mut T,
    total: &'a mut usize,
    total_ceiling: usize,
    segment_ceiling: usize,
    before: usize,
}

impl<T: AsRef<[u8]>> Drop for SegmentMutation<'_, T> {
    fn drop(&mut self) {
        use std::process::abort;

        let after = self.segment.as_ref().len();
        if after > self.segment_ceiling {
            abort();
        }
        let next = if after >= self.before {
            self.total.checked_add(after - self.before)
        } else {
            self.total.checked_sub(self.before - after)
        };
        let Some(next) = next else {
            abort();
        };
        if next > self.total_ceiling {
            abort();
        }
        *self.total = next;
    }
}
