use core::ffi::c_char;

/// Fixed-size, NUL-terminated byte buffer: no heap, no core::fmt.
pub(crate) struct Buf<const N: usize> {
    data: [u8; N],
    len: usize,
}

impl<const N: usize> Buf<N> {
    pub const fn new() -> Self {
        Self { data: [0; N], len: 0 }
    }

    /// Copies `s` into a fresh buffer, truncating; for passing `&str` to C.
    pub fn from_str(s: &str) -> Self {
        let mut b = Self::new();
        b.push(s.as_bytes());
        b
    }

    /// Appends raw bytes, silently truncating (always keeps room for NUL).
    pub fn push(&mut self, s: &[u8]) {
        let n = s.len().min(N - 1 - self.len);
        self.data[self.len..self.len + n].copy_from_slice(&s[..n]);
        self.len += n;
    }

    /// Appends `s` escaped as a JSON string body.
    pub fn push_json_escaped(&mut self, s: &str) {
        for &b in s.as_bytes() {
            match b {
                b'"' => self.push(b"\\\""),
                b'\\' => self.push(b"\\\\"),
                b'\n' => self.push(b"\\n"),
                0..=0x1F => {} // drop other control chars
                _ => self.push(&[b]),
            }
        }
    }

    pub fn as_ptr(&self) -> *const c_char {
        self.data.as_ptr().cast() // data[len] is always 0
    }
}

/// A C string returned by the SDK, stored inline.
#[derive(Clone, Copy)]
pub struct CString<const N: usize>(pub(crate) [u8; N]);

impl<const N: usize> CString<N> {
    pub(crate) const fn zeroed() -> Self {
        Self([0; N])
    }

    pub(crate) fn as_mut_ptr(&mut self) -> *mut c_char {
        self.0.as_mut_ptr().cast()
    }

    /// Bytes up to the first NUL.
    pub fn as_bytes(&self) -> &[u8] {
        let end = self.0.iter().position(|&b| b == 0).unwrap_or(N);
        &self.0[..end]
    }

    /// The string, or `""` if the SDK returned invalid UTF-8.
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(self.as_bytes()).unwrap_or("")
    }
}
