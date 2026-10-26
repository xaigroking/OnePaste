use super::window::MessageWindow;
use crate::error::PlatformError;

const WM_POWERBROADCAST: u32 = 0x0218;
const PBT_APMRESUMEAUTOMATIC: usize = 0x0012;

pub(crate) struct Watcher {
    _win: MessageWindow,
}

impl Watcher {
    pub(crate) fn start(on_wake: Box<dyn Fn() + Send + Sync>) -> Result<Watcher, PlatformError> {
        // 系统广播只发给顶层窗口，所以这里不用仅消息窗口。
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
