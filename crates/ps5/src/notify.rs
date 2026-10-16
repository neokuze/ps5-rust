//! System toasts via `sceNotificationSend`. Feature `notify`.

use crate::{buf::Buf, check, Result};
use ps5_sys::{sceNotificationSend, SCE_NOTIFICATION_LOCAL_USER_ID_SYSTEM};

/// A toast with a title line and an optional second line.
///
/// `InteractiveToastTemplateB` puts `message` on the top line, so a lone
/// message sits at the top; `sub` fills the line under it.
#[derive(Clone, Copy)]
pub struct Toast<'a> {
    pub message: &'a str,
    pub sub: Option<&'a str>,
}

impl<'a> Toast<'a> {
    pub const fn new(message: &'a str) -> Self {
        Self { message, sub: None }
    }

    pub const fn sub(mut self, sub: &'a str) -> Self {
        self.sub = Some(sub);
        self
    }

    pub fn send(&self) -> Result<()> {
        let mut json = Buf::<1024>::new();
        json.push(br#"{"rawData":{"viewTemplateType":"InteractiveToastTemplateB","channelType":"Downloads","useCaseId":"IDC","toastOverwriteType":"No","isImmediate":true,"priority":100,"viewData":{"icon":{"type":"Predefined","parameters":{"icon":"download"}},"message":{"body":""#);
        json.push_json_escaped(self.message);
        json.push(br#""}"#);
        if let Some(sub) = self.sub {
            json.push(br#","subMessage":{"body":""#);
            json.push_json_escaped(sub);
            json.push(br#""}"#);
        }
        json.push(br#"}},"createdDateTime":"2025-12-14T03:14:51.473Z","localNotificationId":"588193127"}"#);

        check(unsafe {
            sceNotificationSend(SCE_NOTIFICATION_LOCAL_USER_ID_SYSTEM as i32, true, json.as_ptr())
        })
    }
}

/// Classic plain-text popup via the kernel (no JSON, no extra lib).
pub fn simple(message: &str) -> Result<()> {
    let mut req: ps5_sys::notify_request_t = unsafe { core::mem::zeroed() };
    let n = message.len().min(req.message.len() - 1);
    for (d, &s) in req.message.iter_mut().zip(&message.as_bytes()[..n]) {
        *d = s as _;
    }
    check(unsafe {
        ps5_sys::sceKernelSendNotificationRequest(0, &mut req, core::mem::size_of_val(&req), 0)
    })
}

/// Shorthand for a single-line toast.
pub fn notify(message: &str) -> Result<()> {
    Toast::new(message).send()
}
