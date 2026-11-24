//! Platform seam unifying native OS and `wasm32-unknown-unknown` primitives:
//! clocks, timers, timeouts, timezone resolution, async Send bounds, and blocking work.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use anyhow::Result;

#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send + Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + Sync + ?Sized> MaybeSend for T {}

#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}
#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> MaybeSend for T {}

#[cfg(not(target_arch = "wasm32"))]
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[cfg(target_arch = "wasm32")]
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use std::time::Instant;

/// Monotonic instant on `wasm32`, backed by `performance.now()` (or `Date.now()` fallback).
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Instant(u64);

#[cfg(target_arch = "wasm32")]
impl Instant {
    #[must_use]
    pub(crate) fn now() -> Self {
        Self(monotonic_us().max(0) as u64)
    }

    #[must_use]
    pub(crate) fn elapsed(self) -> Duration {
        let now = Self::now().0;
        Duration::from_micros(now.saturating_sub(self.0))
    }

    #[must_use]
    pub(crate) fn checked_add(self, duration: Duration) -> Option<Self> {
        let micros = u64::try_from(duration.as_micros()).ok()?;
        self.0.checked_add(micros).map(Self)
    }
}

/// Wall clock in microseconds since the Unix epoch.
#[must_use]
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn epoch_us() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    let since = SystemTime::now().duration_since(UNIX_EPOCH);
    since
        .expect("the wall clock reads after the epoch")
        .as_micros() as i64
}

/// Wall clock in microseconds since the Unix epoch via JavaScript `Date.now()`.
#[must_use]
#[cfg(target_arch = "wasm32")]
pub(crate) fn epoch_us() -> i64 {
    (js_sys::Date::now() * 1000.0) as i64
}

/// Monotonic clock in microseconds, preserving absolute magnitude where available.
#[must_use]
#[cfg(unix)]
pub(crate) fn monotonic_us() -> i64 {
    let mut time = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `time` is a live, writable, aligned `timespec`, and `clock_gettime` writes only
    // through that pointer. `CLOCK_MONOTONIC` is valid on Unix.
    let status = unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) };
    assert_eq!(status, 0, "CLOCK_MONOTONIC is unavailable");
    time.tv_sec * 1_000_000 + time.tv_nsec / 1000
}

#[must_use]
#[cfg(windows)]
pub(crate) fn monotonic_us() -> i64 {
    use windows_sys::Win32::System::Performance::{
        QueryPerformanceCounter, QueryPerformanceFrequency,
    };

    let (mut count, mut frequency) = (0, 0);
    // SAFETY: both pointers are to live, writable, aligned `i64`s, which the calls only write
    // through. Neither call fails on Windows XP or later.
    unsafe {
        QueryPerformanceCounter(&mut count);
        QueryPerformanceFrequency(&mut frequency);
    }
    (i128::from(count) * 1_000_000 / i128::from(frequency)) as i64
}

#[must_use]
#[cfg(target_arch = "wasm32")]
pub(crate) fn monotonic_us() -> i64 {
    use std::cell::Cell;

    thread_local! {
        static LAST_US: Cell<i64> = const { Cell::new(0) };
    }
    let raw = js_performance_now_ms()
        .map(|ms| (ms * 1000.0) as i64)
        .unwrap_or_else(epoch_us);
    LAST_US.with(|last| {
        let clamped = raw.max(last.get());
        last.set(clamped);
        clamped
    })
}

#[cfg(target_arch = "wasm32")]
fn js_performance_now_ms() -> Option<f64> {
    use wasm_bindgen::{JsCast, JsValue};

    let global = js_sys::global();
    let perf = js_sys::Reflect::get(&global, &JsValue::from_str("performance")).ok()?;
    if perf.is_undefined() || perf.is_null() {
        return None;
    }
    let now_fn = js_sys::Reflect::get(&perf, &JsValue::from_str("now"))
        .ok()?
        .dyn_into::<js_sys::Function>()
        .ok()?;
    let value = now_fn.call0(&perf).ok()?.as_f64()?;
    value.is_finite().then_some(value)
}

/// Sleeps asynchronously for `duration`.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn sleep(duration: Duration) {
    tokio::time::sleep(duration).await;
}

/// Cancellable `setTimeout` guard so early-dropped timers (e.g. inside `timeout()`) never leak
/// callbacks or keep Node/Bun event loops alive.
#[cfg(target_arch = "wasm32")]
struct TimerGuard {
    timer_id: Option<wasm_bindgen::JsValue>,
    _closure: Option<wasm_bindgen::closure::Closure<dyn FnMut()>>,
}

#[cfg(target_arch = "wasm32")]
impl Drop for TimerGuard {
    fn drop(&mut self) {
        use wasm_bindgen::{JsCast, JsValue};

        if let Some(id) = self.timer_id.take() {
            let global = js_sys::global();
            if let Ok(clear_fn) = js_sys::Reflect::get(&global, &JsValue::from_str("clearTimeout"))
                .and_then(|value| value.dyn_into::<js_sys::Function>())
            {
                let _ = clear_fn.call1(&global, &id);
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn sleep(duration: Duration) {
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::{JsCast, JsValue};

    if duration.is_zero() {
        return;
    }
    let ms = duration.as_millis().min(i32::MAX as u128) as i32;
    let mut guard = TimerGuard {
        timer_id: None,
        _closure: None,
    };
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        let global = js_sys::global();
        let set_timeout = js_sys::Reflect::get(&global, &JsValue::from_str("setTimeout"))
            .ok()
            .and_then(|value| value.dyn_into::<js_sys::Function>().ok());
        let Some(set_timeout) = set_timeout else {
            let _ = resolve.call0(&JsValue::UNDEFINED);
            return;
        };
        let resolve_cb = resolve.clone();
        let closure = Closure::once(move || {
            let _ = resolve_cb.call0(&JsValue::UNDEFINED);
        });
        match set_timeout.call2(&global, closure.as_ref(), &JsValue::from_f64(f64::from(ms))) {
            Ok(id) => {
                guard.timer_id = Some(id);
                guard._closure = Some(closure);
            }
            Err(_) => {
                let _ = resolve.call0(&JsValue::UNDEFINED);
            }
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
    // Timer already fired; avoid redundant `clearTimeout` on drop.
    guard.timer_id = None;
}

/// Runs `future` with a timeout, returning `None` if `duration` elapses first.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn timeout<F: Future>(duration: Duration, future: F) -> Option<F::Output> {
    tokio::time::timeout(duration, future).await.ok()
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn timeout<F: Future>(duration: Duration, future: F) -> Option<F::Output> {
    use futures_util::future::{Either, select};

    let timer = sleep(duration);
    futures_util::pin_mut!(future, timer);
    match select(future, timer).await {
        Either::Left((output, _)) => Some(output),
        Either::Right(((), _)) => None,
    }
}

/// Runs blocking CPU work off the async runtime on native, or inline on `wasm32`.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) async fn run_blocking<T: Send + 'static>(
    name: &str,
    work: impl FnOnce() -> T + Send + 'static,
) -> Result<T> {
    use anyhow::Context;

    tokio::task::spawn_blocking(work)
        .await
        .with_context(|| format!("the {name} thread failed"))
}

#[cfg(target_arch = "wasm32")]
pub(crate) async fn run_blocking<T: 'static>(
    _name: &str,
    work: impl FnOnce() -> T + 'static,
) -> Result<T> {
    Ok(work())
}

/// Returns the host's IANA time zone name (e.g. `America/New_York`).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn local_timezone_name() -> Result<String> {
    use anyhow::Context;

    iana_time_zone::get_timezone().context("reading the host IANA time zone")
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn local_timezone_name() -> Result<String> {
    use wasm_bindgen::{JsCast, JsValue};

    let global = js_sys::global();
    let tz = (|| -> Option<String> {
        let intl = js_sys::Reflect::get(&global, &JsValue::from_str("Intl")).ok()?;
        let dtf_ctor = js_sys::Reflect::get(&intl, &JsValue::from_str("DateTimeFormat"))
            .ok()?
            .dyn_into::<js_sys::Function>()
            .ok()?;
        let dtf = js_sys::Reflect::construct(&dtf_ctor, &js_sys::Array::new()).ok()?;
        let resolved_fn = js_sys::Reflect::get(&dtf, &JsValue::from_str("resolvedOptions"))
            .ok()?
            .dyn_into::<js_sys::Function>()
            .ok()?;
        let options = resolved_fn.call0(&dtf).ok()?;
        js_sys::Reflect::get(&options, &JsValue::from_str("timeZone"))
            .ok()?
            .as_string()
            .filter(|name| !name.is_empty())
    })()
    .unwrap_or_else(|| "UTC".to_owned());
    Ok(tz)
}

/// Formats `document.lastModified` (`MM/DD/YYYY HH:MM:SS`) in `time_zone` or the host zone.
#[must_use]
pub(crate) fn local_modified_string(time_zone: Option<&str>) -> String {
    use chrono::Utc;

    let now = Utc::now();
    if let Some(name) = time_zone
        && let Ok(tz) = name.parse::<chrono_tz::Tz>()
    {
        return now.with_timezone(&tz).format("%m/%d/%Y %H:%M:%S").to_string();
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        chrono::Local::now().format("%m/%d/%Y %H:%M:%S").to_string()
    }
    #[cfg(target_arch = "wasm32")]
    {
        if let Ok(name) = local_timezone_name()
            && let Ok(tz) = name.parse::<chrono_tz::Tz>()
        {
            return now.with_timezone(&tz).format("%m/%d/%Y %H:%M:%S").to_string();
        }
        let date = js_sys::Date::new_0();
        format!(
            "{:02}/{:02}/{:04} {:02}:{:02}:{:02}",
            date.get_month() + 1,
            date.get_date(),
            date.get_full_year(),
            date.get_hours(),
            date.get_minutes(),
            date.get_seconds()
        )
    }
}
