use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};

static LOGS_ENABLED: AtomicBool = AtomicBool::new(false);
static LOGS_INIT: Once = Once::new();

/// Returns true when RPC logging is enabled.
///
/// The `RSRPC_LOGS_ENABLED` environment variable is read once as the fallback
/// default; an explicit [`set_logs_enabled`] call always wins.
pub fn enabled() -> bool {
    LOGS_INIT.call_once(|| {
        if matches!(std::env::var("RSRPC_LOGS_ENABLED"), Ok(value) if value == "1") {
            LOGS_ENABLED.store(true, Ordering::Relaxed);
        }
    });

    LOGS_ENABLED.load(Ordering::Relaxed)
}

/// Explicitly enable or disable RPC logging, overriding the env-var default.
///
/// Kept for backwards compatibility; prefer configuring a `tracing_subscriber` subscriber.
pub fn set_logs_enabled(enabled: bool) {
    LOGS_INIT.call_once(|| {});
    LOGS_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Print a log line through `tracing::info!`.
pub fn log(message: impl AsRef<str>) {
    tracing::info!("{}", message.as_ref());
}

/// Log a formatted message through `tracing::info!`.
#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {
        $crate::tracing::info!($($arg)*)
    };
}
