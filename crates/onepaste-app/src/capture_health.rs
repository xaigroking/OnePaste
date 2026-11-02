//! Native listener health is separate from the user's recording preference.
use super::*;
use std::sync::atomic::Ordering;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureState {
    #[default]
    Stopped,
    Starting,
    Listening,
    ReadError,
    StartFailed,
}
impl CaptureState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Starting => "starting",
            Self::Listening => "listening",
            Self::ReadError => "read_error",
            Self::StartFailed => "start_failed",
        }
    }
}

/// Counts contain no content, application names, file paths or credentials.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureHealth {
    pub state: CaptureState,
    pub error: Option<String>,
    pub events: u64,
    pub read_failures: u64,
    pub recorded: u64,
    pub filtered: u64,
    pub retries: u32,
}

pub(super) fn safe_code(code: &str) -> String {
    if matches!(
        code,
        "clipboard_busy" | "bad_format" | "unsupported" | "monitor_stopped"
    ) {
        return code.into();
    }
    if let Some(number) = code.strip_prefix("os_") {
        if number.len() <= 20 && number.parse::<i64>().is_ok() {
            return code.into();
        }
    }
    "unavailable".into()
}

impl App {
    pub(super) fn start_monitor(&self) {
        self.0.capture_active.store(true, Ordering::Release);
        if !self.try_start_monitor() {
            self.schedule_capture_retry();
        }
    }

    fn try_start_monitor(&self) -> bool {
        let mut monitor = lock(&self.0.monitor);
        if !self.0.capture_active.load(Ordering::Acquire) || !self.is_joined() || self.expired() {
            return false;
        }
        let epoch = self.0.monitor_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        *monitor = None;
        {
            let mut shared = lock(&self.0.shared);
            shared.capture.state = CaptureState::Starting;
            shared.capture.error = None;
        }
        let app = self.clone();
        let handle = tokio::runtime::Handle::current();
        let started = self.0.clipboard.start_monitor(Box::new(move |event| {
            let mut tasks = lock(&app.0.tasks);
            if !app.0.capture_active.load(Ordering::Acquire)
                || app.0.monitor_epoch.load(Ordering::Acquire) != epoch
                || app.require_access().is_err()
            {
                return;
            }
            tasks.retain(|task| !task.is_finished());
            let captured = app.clone();
            tasks.push(handle.spawn(async move {
                if captured.0.monitor_epoch.load(Ordering::Acquire) == epoch
                    && captured.0.capture_active.load(Ordering::Acquire)
                {
                    captured.on_clip_event(event).await;
                }
            }));
        }));
        // shutdown can disable capture while the native worker is becoming ready.
        if !self.0.capture_active.load(Ordering::Acquire) {
            drop(started);
            return false;
        }
        let success = match started {
            Ok(guard) => {
                *monitor = Some(guard);
                let mut shared = lock(&self.0.shared);
                if shared.capture.state == CaptureState::Starting {
                    shared.capture.state = CaptureState::Listening;
                }
                true
            }
            Err(error) => {
                let code = match error {
                    AppError::Clipboard(code) => safe_code(&code),
                    _ => "unavailable".into(),
                };
                {
                    let mut shared = lock(&self.0.shared);
                    shared.capture.state = CaptureState::StartFailed;
                    shared.capture.error = Some(code.clone());
                }
                self.log("clipboard_monitor_failed", Some(code));
                false
            }
        };
        drop(monitor);
        self.notify_status();
        success
    }

    pub(super) fn stop_capture_monitor(&self) {
        self.0.capture_active.store(false, Ordering::Release);
        if let Some(task) = lock(&self.0.capture_retry).take() {
            task.abort();
        }
        let mut monitor = lock(&self.0.monitor);
        self.0.monitor_epoch.fetch_add(1, Ordering::AcqRel);
        *monitor = None;
        let mut shared = lock(&self.0.shared);
        shared.capture.state = CaptureState::Stopped;
        shared.capture.error = None;
    }

    /// An explicit retry never unlocks a session or changes the recording preference.
    pub async fn retry_clipboard_monitor(&self) -> Result<(), AppError> {
        let _connection = self.session_write_access().await?;
        if let Some(task) = lock(&self.0.capture_retry).take() {
            task.abort();
        }
        self.start_monitor();
        Ok(())
    }

    pub(super) fn schedule_capture_retry(&self) {
        if !self.0.capture_active.load(Ordering::Acquire) {
            return;
        }
        let mut retry = lock(&self.0.capture_retry);
        if retry.as_ref().is_some_and(|task| !task.is_finished()) {
            return;
        }
        let epoch = self.session_epoch();
        let weak = Arc::downgrade(&self.0);
        *retry = Some(tokio::spawn(async move {
            for seconds in [2, 5, 15] {
                tokio::time::sleep(Duration::from_secs(seconds)).await;
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let app = App(inner);
                let _connection = app.0.connection.write().await;
                if epoch != app.session_epoch()
                    || !app.0.capture_active.load(Ordering::Acquire)
                    || app.require_access().is_err()
                {
                    return;
                }
                lock(&app.0.shared).capture.retries += 1;
                if app.try_start_monitor() {
                    return;
                }
            }
        }));
    }

    pub(super) fn record_capture_event(&self, event: &ClipEvent) {
        let mut shared = lock(&self.0.shared);
        if !shared.joined || !self.0.capture_active.load(Ordering::Acquire) {
            return;
        }
        let health = &mut shared.capture;
        health.events += 1;
        match event {
            ClipEvent::ReadFailed(code) => {
                health.read_failures += 1;
                health.state = CaptureState::ReadError;
                health.error = Some(safe_code(code));
            }
            ClipEvent::MonitorStopped => {
                health.state = CaptureState::StartFailed;
                health.error = Some("monitor_stopped".into());
            }
            _ => {
                health.state = CaptureState::Listening;
                health.error = None;
                if !matches!(event, ClipEvent::Captured(_)) {
                    health.filtered += 1;
                }
            }
        }
    }
}
