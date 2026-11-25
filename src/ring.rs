use crate::RX_CAPACITY;

pub(super) struct Ring {
    bytes: [u8; RX_CAPACITY],
    head: usize,
    len: usize,
}

impl Ring {
    pub(super) const fn new() -> Self {
        Self {
            bytes: [0; RX_CAPACITY],
            head: 0,
            len: 0,
        }
    }

    pub(super) fn len(&self) -> usize {
        self.len
    }

    pub(super) fn byte(&self, offset: usize) -> u8 {
        debug_assert!(offset < self.len);
        self.bytes[(self.head + offset) % RX_CAPACITY]
    }

    pub(super) fn extend(&mut self, bytes: &[u8]) {
        debug_assert!(bytes.len() <= RX_CAPACITY - self.len);
        let tail = (self.head + self.len) % RX_CAPACITY;
        let first = bytes.len().min(RX_CAPACITY - tail);
        let (end, start) = bytes.split_at(first);
        self.bytes[tail..tail + end.len()].copy_from_slice(end);
        self.bytes[..start.len()].copy_from_slice(start);
        self.len += bytes.len();
    }

    pub(super) fn discard(&mut self, count: usize) {
        debug_assert!(count <= self.len);
        self.head = (self.head + count) % RX_CAPACITY;
        self.len -= count;
    }
}
