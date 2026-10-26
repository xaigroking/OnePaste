use super::window::MessageWindow;
use crate::error::PlatformError;

const WM_POWERBROADCAST: u32 = 0x0218;
const PBT_APMRESUMEAUTOMATIC: usize = 0x0012;

pub(crate) struct Watcher {
    _win: MessageWindow,
}

impl Watcher {
    pub(crate) fn start(on_wake: Box<dyn Fn() + Send + Sync>) -> Result<Watcher, PlatformError> {
        // System broadcasts only reach top-level windows, so a message-only window is not used here.
        let win = MessageWindow::spawn(
            "OnePastePowerWatcher",
            false,
            |_| Ok(()),
            move |msg, wp, _| {
                if msg == WM_POWERBROADCAST && wp.0 == PBT_APMRESUMEAUTOMATIC {
                    on_wake();
                }
            },
        )?;
        Ok(Watcher { _win: win })
    }
}
