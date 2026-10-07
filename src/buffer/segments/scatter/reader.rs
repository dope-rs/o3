use std::{io, marker};

/// A standard reader over borrowed byte-bearing parts.
pub struct Reader<'data, I, T: ?Sized> {
    parts: I,
    current: &'data [u8],
    item: marker::PhantomData<fn() -> T>,
}

impl<'data, I, T> Reader<'data, I, T>
where
    I: Iterator<Item = &'data T>,
    T: AsRef<[u8]> + ?Sized + 'data,
{
    pub fn new(parts: I) -> Self {
        Self {
            parts,
            current: &[],
            item: marker::PhantomData,
        }
    }

    fn fill(&mut self) {
        while self.current.is_empty() {
            let Some(part) = self.parts.next() else {
                break;
            };
            self.current = part.as_ref();
        }
    }
}

impl<'data, I, T> io::Read for Reader<'data, I, T>
where
    I: Iterator<Item = &'data T>,
    T: AsRef<[u8]> + ?Sized + 'data,
{
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        self.fill();
        let len = output.len().min(self.current.len());
        output[..len].copy_from_slice(&self.current[..len]);
        self.current = &self.current[len..];
        Ok(len)
    }
}

impl<'data, I, T> io::BufRead for Reader<'data, I, T>
where
    I: Iterator<Item = &'data T>,
    T: AsRef<[u8]> + ?Sized + 'data,
{
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        self.fill();
        Ok(self.current)
    }

    fn consume(&mut self, amount: usize) {
        let consumed = amount.min(self.current.len());
        self.current = &self.current[consumed..];
    }
}
