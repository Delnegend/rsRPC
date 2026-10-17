use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;

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
pub fn set_logs_enabled(enabled: bool) {
  LOGS_INIT.call_once(|| {});
  LOGS_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Print a log line when logging is [`enabled`].
pub fn log(message: impl AsRef<str>) {
  if enabled() {
    println!(
      "[{}] {}",
      chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
      message.as_ref()
    );
  }
}

/// Log a formatted message. The arguments are only formatted when logging is
/// [`enabled`], so disabled logging costs a single atomic load.
#[macro_export]
macro_rules! log {
  ($($arg:tt)*) => {
    if $crate::logger::enabled() {
      $crate::logger::log(format!($($arg)*))
    }
  };
}
