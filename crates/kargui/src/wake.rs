//! ほかのスレッドから窓を起こす係。
//!
//! 起こすと印を立て、OS ごとの「窓を起こす」処理を呼ぶ（Windows は窓へメッセージを送る。試験台は印だけ）。
//! 窓は起こされたらビューを回すので、アプリはビューの中でチャンネルなどから届いた物を読む。

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

struct Inner {
    woken: AtomicBool,
    notify: Box<dyn Fn() + Send + Sync>,
}

/// Wakes the window from any thread so that [`App::view`](crate::App::view) runs again. Get one
/// with [`Ui::waker`](crate::Ui::waker) and move a clone to the thread that does the work. kargui
/// sleeps while nothing changes, so a background thread that has new data calls [`Waker::wake`]
/// after sending it (for example through a `std::sync::mpsc` channel that `view` reads).
///
/// ```
/// use std::sync::mpsc::{Receiver, channel};
/// use kargui::{App, Ui, Waker};
///
/// struct Downloads {
///     rx: Receiver<String>,
///     waker: Option<Waker>,
///     done: Vec<String>,
/// }
///
/// impl App for Downloads {
///     fn view(&mut self, ui: &mut Ui) {
///         self.waker.get_or_insert_with(|| ui.waker());
///         self.done.extend(self.rx.try_iter());
///         for name in &self.done {
///             ui.label(name);
///         }
///     }
/// }
///
/// let (tx, rx) = channel();
/// let mut h = kargui::test::Harness::new(Downloads { rx, waker: None, done: Vec::new() });
/// let waker = h.app().waker.clone().unwrap();
/// std::thread::spawn(move || {
///     tx.send("report.pdf".to_string()).unwrap();
///     waker.wake();
/// })
/// .join()
/// .unwrap();
/// h.frame();
/// assert!(h.find("report.pdf").is_some());
/// ```
#[derive(Clone)]
pub struct Waker(Arc<Inner>);

impl Waker {
    /// 起こされた時に呼ぶ処理を持つ係を作る（窓は「窓へメッセージを送る」、試験台は何もしない）
    pub(crate) fn new(notify: impl Fn() + Send + Sync + 'static) -> Self {
        Waker(Arc::new(Inner { woken: AtomicBool::new(false), notify: Box::new(notify) }))
    }

    /// Asks the window to run [`App::view`](crate::App::view) again soon. Cheap, and safe to call
    /// from any thread and many times: wakes that arrive before the next frame are merged.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// let waker = ui.waker();
    /// std::thread::spawn(move || {
    ///     // ... finish some work, send the result ...
    ///     waker.wake();
    /// });
    /// # });
    /// ```
    pub fn wake(&self) {
        // 前の起こしがまだ窓に届いていなければ、もう一度は送らない
        if !self.0.woken.swap(true, Ordering::SeqCst) {
            (self.0.notify)();
        }
    }

    /// 起こされた印を読んで消す（窓がフレームを回す前に呼ぶ）
    pub(crate) fn take_woken(&self) -> bool {
        self.0.woken.swap(false, Ordering::SeqCst)
    }
}

impl fmt::Debug for Waker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Waker").field("woken", &self.0.woken.load(Ordering::SeqCst)).finish()
    }
}
