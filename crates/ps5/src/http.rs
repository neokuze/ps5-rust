//! HTTP(S) client over libSceHttp2. Link with `-lSceSsl -lSceHttp2`
//! (libSceNet is linked by default).
//!
//! ```ignore
//! let http = Http::init("my-payload/1.0")?;
//! let mut res = http.get("http://192.168.1.10:8080/hello")?;
//! let mut buf = [0u8; 512];
//! let n = res.read(&mut buf)?;
//! ```
//!
//! Sequence and pool sizes follow the SDK's `http2_get` sample.

use crate::{buf::Buf, check, Error, Result};
use ps5_sys as sys;

const NET_POOL: i32 = 32 * 1024;
const SSL_POOL: usize = 256 * 1024;
const HTTP_POOL: usize = 256 * 1024;

/// Turns the SDK's "negative = error, else id" convention into a Result.
fn id(code: i32) -> Result<i32> {
    if code < 0 { Err(Error(code)) } else { Ok(code) }
}

/// Owns the net pool, SSL and HTTP2 contexts and one request template;
/// all of it is torn down in reverse order on drop.
pub struct Http {
    net: i32,
    ssl: i32,
    ctx: i32,
    tmpl: i32,
}

impl Http {
    pub fn init(user_agent: &str) -> Result<Self> {
        let agent = Buf::<128>::from_str(user_agent);
        // Fill in one handle at a time so Drop only frees what exists.
        let mut h = Http { net: -1, ssl: -1, ctx: -1, tmpl: -1 };
        unsafe {
            check(sys::sceNetInit())?;
            h.net = id(sys::sceNetPoolCreate(c"ps5-rs-http".as_ptr(), NET_POOL, 0))?;
            h.ssl = id(sys::sceSslInit(SSL_POOL))?;
            h.ctx = id(sys::sceHttp2Init(h.net, h.ssl, HTTP_POOL, 1))?;
            h.tmpl = id(sys::sceHttp2CreateTemplate(h.ctx, agent.as_ptr(), 3, 1))?;
        }
        Ok(h)
    }

    pub fn get(&self, url: &str) -> Result<Response<'_>> {
        self.request("GET", url, &[])
    }

    pub fn post(&self, url: &str, body: &[u8]) -> Result<Response<'_>> {
        self.request("POST", url, body)
    }

    /// Sends a request and waits for the status line.
    pub fn request(&self, method: &str, url: &str, body: &[u8]) -> Result<Response<'_>> {
        let method = Buf::<16>::from_str(method);
        let url = Buf::<2048>::from_str(url);
        let req = id(unsafe {
            sys::sceHttp2CreateRequestWithURL(self.tmpl, method.as_ptr(), url.as_ptr(), body.len() as u64)
        })?;
        let mut res = Response { req, status: -1, _http: core::marker::PhantomData };

        let body_ptr = if body.is_empty() { core::ptr::null() } else { body.as_ptr().cast() };
        unsafe {
            check(sys::sceHttp2SendRequest(req, body_ptr, body.len()))?;
            check(sys::sceHttp2GetStatusCode(req, &mut res.status))?;
        }
        Ok(res)
    }
}

impl Drop for Http {
    fn drop(&mut self) {
        unsafe {
            if self.tmpl >= 0 { sys::sceHttp2DeleteTemplate(self.tmpl); }
            if self.ctx >= 0 { sys::sceHttp2Term(self.ctx); }
            if self.ssl >= 0 { sys::sceSslTerm(self.ssl); }
            if self.net >= 0 { sys::sceNetPoolDestroy(self.net); }
        }
    }
}

/// An in-flight response; the request is deleted on drop.
pub struct Response<'a> {
    req: i32,
    status: i32,
    _http: core::marker::PhantomData<&'a Http>,
}

impl Response<'_> {
    pub fn status(&self) -> i32 {
        self.status
    }

    /// Reads the next chunk of the body; `Ok(0)` means the end.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let n = unsafe { sys::sceHttp2ReadData(self.req, buf.as_mut_ptr().cast(), buf.len()) };
        id(n).map(|n| n as usize)
    }

    /// Reads as much of the body as fits in `buf`; returns the bytes read.
    pub fn read_to_fill(&mut self, buf: &mut [u8]) -> Result<usize> {
        let mut total = 0;
        while total < buf.len() {
            match self.read(&mut buf[total..])? {
                0 => break,
                n => total += n,
            }
        }
        Ok(total)
    }
}

impl Drop for Response<'_> {
    fn drop(&mut self) {
        unsafe { sys::sceHttp2DeleteRequest(self.req) };
    }
}
