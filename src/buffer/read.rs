use super::{Owned, Shared};

pub trait Read {
    fn remaining(&self) -> usize;
    fn chunk(&self) -> &[u8];
    fn advance(&mut self, cnt: usize);

    fn has_remaining(&self) -> bool {
        self.remaining() > 0
    }

    fn get_uint(&mut self, nbytes: usize) -> u64 {
        assert!((1..=8).contains(&nbytes), "get_uint: nbytes must be 1..=8");
        let mut out = [0u8; 8];
        self.copy_to_slice(&mut out[8 - nbytes..]);
        u64::from_be_bytes(out)
    }

    fn copy_to_slice(&mut self, dst: &mut [u8]) {
        assert!(
            self.remaining() >= dst.len(),
            "buffer::Read::copy_to_slice: source exhausted (need {}, have {})",
            dst.len(),
            self.remaining()
        );
        let mut filled = 0usize;
        while filled < dst.len() {
            let chunk = self.chunk();
            debug_assert!(
                !chunk.is_empty(),
                "buffer::Read impl violated contract: chunk() empty while remaining() > 0"
            );
            let take = (dst.len() - filled).min(chunk.len());
            dst[filled..filled + take].copy_from_slice(&chunk[..take]);
            self.advance(take);
            filled += take;
        }
    }

    fn get_u8(&mut self) -> u8 {
        let mut buf = [0u8; 1];
        self.copy_to_slice(&mut buf);
        buf[0]
    }

    fn get_u16(&mut self) -> u16 {
        let mut buf = [0u8; 2];
        self.copy_to_slice(&mut buf);
        u16::from_be_bytes(buf)
    }

    fn get_i16(&mut self) -> i16 {
        let mut buf = [0u8; 2];
        self.copy_to_slice(&mut buf);
        i16::from_be_bytes(buf)
    }

    fn get_u32(&mut self) -> u32 {
        let mut buf = [0u8; 4];
        self.copy_to_slice(&mut buf);
        u32::from_be_bytes(buf)
    }

    fn get_i32(&mut self) -> i32 {
        let mut buf = [0u8; 4];
        self.copy_to_slice(&mut buf);
        i32::from_be_bytes(buf)
    }

    fn get_u64(&mut self) -> u64 {
        let mut buf = [0u8; 8];
        self.copy_to_slice(&mut buf);
        u64::from_be_bytes(buf)
    }
}

pub trait Write {
    fn remaining_mut(&self) -> usize;
    fn put_slice(&mut self, src: &[u8]);

    fn put_u8(&mut self, n: u8) {
        self.put_slice(&[n]);
    }

    fn put_u16(&mut self, n: u16) {
        self.put_slice(&n.to_be_bytes());
    }

    fn put_i16(&mut self, n: i16) {
        self.put_slice(&n.to_be_bytes());
    }

    fn put_u32(&mut self, n: u32) {
        self.put_slice(&n.to_be_bytes());
    }

    fn put_i32(&mut self, n: i32) {
        self.put_slice(&n.to_be_bytes());
    }

    fn put_u64(&mut self, n: u64) {
        self.put_slice(&n.to_be_bytes());
    }

    fn put_uint(&mut self, n: u64, nbytes: usize) {
        assert!((1..=8).contains(&nbytes), "put_uint: nbytes must be 1..=8");
        let bytes = n.to_be_bytes();
        self.put_slice(&bytes[8 - nbytes..]);
    }

    fn put<T: Read>(&mut self, mut src: T) {
        while src.remaining() > 0 {
            let chunk = src.chunk();
            let len = chunk.len();
            self.put_slice(chunk);
            src.advance(len);
        }
    }
}

impl Read for &[u8] {
    fn remaining(&self) -> usize {
        self.len()
    }

    fn chunk(&self) -> &[u8] {
        self
    }

    fn advance(&mut self, cnt: usize) {
        *self = &self[cnt..];
    }

    fn copy_to_slice(&mut self, dst: &mut [u8]) {
        let n = dst.len();
        dst.copy_from_slice(&self[..n]);
        *self = &self[n..];
    }

    fn get_u8(&mut self) -> u8 {
        let v = self[0];
        *self = &self[1..];
        v
    }

    fn get_u32(&mut self) -> u32 {
        let v = u32::from_be_bytes(self[..4].try_into().unwrap());
        *self = &self[4..];
        v
    }
}

impl<T: AsRef<[u8]>> Read for std::io::Cursor<T> {
    fn remaining(&self) -> usize {
        let pos = self.position() as usize;
        let total = self.get_ref().as_ref().len();
        total.saturating_sub(pos)
    }

    fn chunk(&self) -> &[u8] {
        let pos = self.position() as usize;
        let buf = self.get_ref().as_ref();
        let pos = pos.min(buf.len());
        &buf[pos..]
    }

    fn advance(&mut self, cnt: usize) {
        let new = self.position().saturating_add(cnt as u64);
        self.set_position(new);
    }
}

impl Read for Shared {
    fn remaining(&self) -> usize {
        self.len()
    }

    fn chunk(&self) -> &[u8] {
        self.as_slice()
    }

    fn advance(&mut self, cnt: usize) {
        Self::advance(self, cnt);
    }
}

impl Write for Vec<u8> {
    fn remaining_mut(&self) -> usize {
        usize::MAX - self.len()
    }

    fn put_slice(&mut self, src: &[u8]) {
        self.extend_from_slice(src);
    }
}

impl Write for Owned {
    fn remaining_mut(&self) -> usize {
        usize::MAX - self.len()
    }

    fn put_slice(&mut self, src: &[u8]) {
        self.extend_from_slice(src);
    }
}

impl Write for &mut [u8] {
    fn remaining_mut(&self) -> usize {
        self.len()
    }

    fn put_slice(&mut self, src: &[u8]) {
        let n = src.len();
        let (head, tail) = std::mem::take(self).split_at_mut(n);
        head.copy_from_slice(src);
        *self = tail;
    }
}

impl<T: Write + ?Sized> Write for &mut T {
    fn remaining_mut(&self) -> usize {
        (**self).remaining_mut()
    }

    fn put_slice(&mut self, src: &[u8]) {
        (**self).put_slice(src)
    }
}

impl<T: Read + ?Sized> Read for &mut T {
    fn remaining(&self) -> usize {
        (**self).remaining()
    }

    fn chunk(&self) -> &[u8] {
        (**self).chunk()
    }

    fn advance(&mut self, cnt: usize) {
        (**self).advance(cnt)
    }
}
