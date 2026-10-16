//! Tiny formatting without `core::fmt` (which costs ~20-50 KB).
//!
//! ```ignore
//! ps5::log!("cpu ", temp, "C, fw ", Hex(ps5::fw_version()));
//! ps5::print!("status ", status);
//! ```

/// Anything a [`Line`] can append.
pub trait Arg {
    fn write_to<const N: usize>(&self, line: &mut Line<N>);
}

/// Formats an integer as `0x...` hex.
#[derive(Clone, Copy)]
pub struct Hex(pub u64);

/// A fixed-capacity text line; extra input is truncated, never panics.
pub struct Line<const N: usize> {
    buf: [u8; N],
    len: usize,
}

impl<const N: usize> Line<N> {
    pub const fn new() -> Self {
        Self { buf: [0; N], len: 0 }
    }

    pub fn push<T: Arg + ?Sized>(&mut self, v: &T) -> &mut Self {
        v.write_to(self);
        self
    }

    pub fn push_bytes(&mut self, s: &[u8]) {
        let n = s.len().min(N - self.len);
        self.buf[self.len..self.len + n].copy_from_slice(&s[..n]);
        self.len += n;
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    /// Valid UTF-8 unless truncation split a multi-byte char; then the
    /// string stops before it.
    pub fn as_str(&self) -> &str {
        match core::str::from_utf8(self.as_bytes()) {
            Ok(s) => s,
            Err(e) => unsafe { core::str::from_utf8_unchecked(&self.buf[..e.valid_up_to()]) },
        }
    }

    fn push_u64(&mut self, mut v: u64, radix: u64) {
        let mut tmp = [0u8; 20];
        let mut i = tmp.len();
        loop {
            i -= 1;
            tmp[i] = b"0123456789abcdef"[(v % radix) as usize];
            v /= radix;
            if v == 0 { break; }
        }
        self.push_bytes(&tmp[i..]);
    }
}

impl Arg for str {
    fn write_to<const N: usize>(&self, l: &mut Line<N>) { l.push_bytes(self.as_bytes()) }
}
impl Arg for &str {
    fn write_to<const N: usize>(&self, l: &mut Line<N>) { l.push_bytes(self.as_bytes()) }
}
impl Arg for bool {
    fn write_to<const N: usize>(&self, l: &mut Line<N>) { l.push_bytes(if *self { b"true" } else { b"false" }) }
}
impl Arg for Hex {
    fn write_to<const N: usize>(&self, l: &mut Line<N>) { l.push_bytes(b"0x"); l.push_u64(self.0, 16) }
}
impl<const M: usize> Arg for crate::CString<M> {
    fn write_to<const N: usize>(&self, l: &mut Line<N>) { l.push_bytes(self.as_bytes()) }
}

macro_rules! unsigned { ($($t:ty)*) => {$(
    impl Arg for $t {
        fn write_to<const N: usize>(&self, l: &mut Line<N>) { l.push_u64(*self as u64, 10) }
    }
)*} }
macro_rules! signed { ($($t:ty)*) => {$(
    impl Arg for $t {
        fn write_to<const N: usize>(&self, l: &mut Line<N>) {
            if *self < 0 { l.push_bytes(b"-"); }
            l.push_u64((*self as i64).unsigned_abs(), 10)
        }
    }
)*} }
unsigned!(u8 u16 u32 u64 usize);
signed!(i8 i16 i32 i64 isize);

/// Builds a [`Line`] (256 bytes) from comma-separated [`Arg`]s.
#[macro_export]
macro_rules! line {
    ($($arg:expr),* $(,)?) => {{
        let mut l = $crate::fmt::Line::<256>::new();
        $( l.push(&$arg); )*
        l
    }};
}

/// Writes a line to the kernel log.
#[macro_export]
macro_rules! log {
    ($($arg:expr),* $(,)?) => { $crate::klog::log($crate::line!($($arg),*).as_str()) };
}

/// Writes a line to stdout (shown by the loader / `prospero-deploy`).
#[macro_export]
macro_rules! print {
    ($($arg:expr),* $(,)?) => { $crate::fmt::stdout($crate::line!($($arg),*).as_bytes()) };
}

#[doc(hidden)]
pub fn stdout(s: &[u8]) {
    unsafe {
        ps5_sys::write(1, s.as_ptr().cast(), s.len() as _);
        ps5_sys::write(1, b"\n".as_ptr().cast(), 1);
    }
}
