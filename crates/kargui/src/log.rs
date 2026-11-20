//! Diagnostic events.
//!
//! kargui reports what it does (window creation, rendering, input method, accessibility)
//! as one-line events. It does not write files itself: the application decides where
//! the events go by calling [`set_logger`].
//!
//! フレームワークは記録の置き場所を決めない。アプリが set_logger で渡した関数に、
//! 「出来事・引数・結果」を渡すだけにする。

use std::sync::OnceLock;

/// A function that receives `(event, args, result)`.
pub type Logger = fn(event: &str, args: &str, result: &str);

static LOGGER: OnceLock<Logger> = OnceLock::new();

/// Sets where kargui sends its diagnostic events. Only the first call has an effect.
///
/// ```
/// fn print(event: &str, args: &str, result: &str) {
///     println!("{event} {args} -> {result}");
/// }
/// kargui::set_logger(print);
/// ```
pub fn set_logger(logger: Logger) {
    let _ = LOGGER.set(logger);
}

/// 記録を 1 つ渡す。set_logger が呼ばれていなければ何もしない
pub(crate) fn event(event: &str, args: &str, result: &str) {
    if let Some(f) = LOGGER.get() {
        f(event, args, result);
    }
}

/// 記録が要るか（記録の文字列を作る手間を省くため）
pub(crate) fn enabled() -> bool {
    LOGGER.get().is_some()
}
