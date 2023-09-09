use core::cmp::min;
use core::mem::MaybeUninit;

pub struct RingBuf<const N: usize> {
    buf: MaybeUninit<[u8; N]>,
    start: usize,
    end: usize,
    empty: bool,
}

impl<const N: usize> RingBuf<N> {
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            buf: MaybeUninit::uninit(),
            start: 0,
            end: 0,
            empty: true,
        }
    }

    #[inline(always)]
    pub fn push(&mut self, data: &[u8]) -> usize {
        let mut offset = 0;

        while offset < data.len() {
            let buf: &mut [u8] = unsafe { self.buf.assume_init_mut() };

            let len = min(buf.len() - self.end, data.len() - offset);

            buf[self.end..self.end + len].copy_from_slice(&data[offset..offset + len]);

            offset += len;

            if !self.empty && self.start >= self.end && self.start < self.end + len {
                // Dropping oldest data
                self.start = self.end + len;
            }

            self.end += len;

            self.wrap();

            self.empty = false;
        }

        self.len()
    }

    #[inline(always)]
    pub fn push_byte(&mut self, data: u8) -> usize {
        let buf: &mut [u8] = unsafe { self.buf.assume_init_mut() };

        buf[self.end] = data;

        if !self.empty && self.start == self.end {
            // Dropping oldest data
            self.start = self.end + 1;
        }

        self.end += 1;

        self.wrap();

        self.empty = false;

        self.len()
    }

    #[inline(always)]
    pub fn pop(&mut self, out_buf: &mut [u8]) -> usize {
        let mut offset = 0;

        while offset < out_buf.len() && !self.empty {
            let buf: &mut [u8] = unsafe { self.buf.assume_init_mut() };

            let len = min(
                if self.start < self.end {
                    self.end
                } else {
                    buf.len()
                } - self.start,
                out_buf.len() - offset,
            );

            out_buf[offset..offset + len].copy_from_slice(&buf[self.start..self.start + len]);

            self.start += len;

            self.wrap();

            if self.start == self.end {
                self.empty = true
            }

            offset += len;
        }

        offset
    }

    #[inline(always)]
    pub fn is_full(&self) -> bool {
        self.start == self.end && !self.empty
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.empty
    }

    #[inline(always)]
    #[allow(unused)]
    pub fn len(&self) -> usize {
        if self.empty {
            0
        } else if self.start < self.end {
            self.end - self.start
        } else {
            let buf: &[u8] = unsafe { self.buf.assume_init_ref() };

            buf.len() + self.end - self.start
        }
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.start = 0;
        self.end = 0;
        self.empty = true;
    }

    #[inline(always)]
    fn wrap(&mut self) {
        let buf: &[u8] = unsafe { self.buf.assume_init_ref() };

        if self.start == buf.len() {
            self.start = 0;
        }

        if self.end == buf.len() {
            self.end = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_pop() {
        let mut rb: RingBuf<4> = RingBuf::new();
        assert!(rb.is_empty());

        rb.push(&[0, 1, 2]);
        assert_eq!(3, rb.len());
        assert!(!rb.is_empty());
        assert!(!rb.is_full());

        rb.push(&[3]);
        assert_eq!(4, rb.len());
        assert!(!rb.is_empty());
        assert!(rb.is_full());

        let mut buf = [0; 256];

        let len = rb.pop(&mut buf);
        assert_eq!(4, len);
        assert_eq!(&buf[0..4], &[0, 1, 2, 3]);
        assert!(rb.is_empty());

        rb.push(&[0, 1, 2, 3, 4, 5]);
        assert_eq!(4, rb.len());
        assert!(!rb.is_empty());
        assert!(rb.is_full());

        let len = rb.pop(&mut buf[..3]);
        assert_eq!(3, len);
        assert_eq!(&buf[0..len], &[2, 3, 4]);
        assert!(!rb.is_empty());
        assert!(!rb.is_full());

        let len = rb.pop(&mut buf);
        assert_eq!(1, len);
        assert_eq!(&buf[0..len], &[5]);
        assert!(rb.is_empty());
        assert!(!rb.is_full());

        let len = rb.pop(&mut buf);
        assert_eq!(0, len);
        assert_eq!(&buf[0..len], &[]);
        assert!(rb.is_empty());
        assert!(!rb.is_full());
    }
}
