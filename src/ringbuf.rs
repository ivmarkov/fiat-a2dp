use core::cmp::min;

pub struct RingBuf<'a> {
    buf: &'a mut [u8],
    start: usize,
    end: usize,
    empty: bool,
}

impl<'a> RingBuf<'a> {
    #[inline(always)]
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self {
            buf,
            start: 0,
            end: 0,
            empty: true,
        }
    }

    #[inline(always)]
    pub fn push(&mut self, data: &[u8]) -> usize {
        let mut offset = 0;

        while offset < data.len() {
            let len = min(self.buf.len() - self.end, data.len() - offset);

            self.buf[self.end..self.end + len].copy_from_slice(&data[offset..offset + len]);

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
        self.buf[self.end] = data;

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
            let len = min(
                if self.start < self.end {
                    self.end
                } else {
                    self.buf.len()
                } - self.start,
                out_buf.len() - offset,
            );

            out_buf[offset..offset + len].copy_from_slice(&self.buf[self.start..self.start + len]);

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
            self.buf.len() + self.end - self.start
        }
    }

    pub fn buf_len(&self) -> usize {
        self.buf.len()
    }

    #[inline(always)]
    pub fn clear(&mut self) {
        self.start = 0;
        self.end = 0;
        self.empty = true;
    }

    #[inline(always)]
    fn wrap(&mut self) {
        if self.start == self.buf.len() {
            self.start = 0;
        }

        if self.end == self.buf.len() {
            self.end = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_pop() {
        let mut buf = [0; 4];
        let mut rb = RingBuf::new(&mut buf);
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
