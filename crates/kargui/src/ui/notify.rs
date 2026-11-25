//! 通知（Windows はトースト、Linux は org.freedesktop.Notifications）。ビューの Ui::notify が頼み、窓の処理がフレーム
//! の後で OS に渡す。押された答えは、窓の処理が頼んだ窓の Context の notification_results に入れ、
//! Ui::notification_clicked が 1 回だけ返す（ファイルのダイアログと同じ形）。

/// 1 つの通知に置けるボタンの数（Windows のトーストは 5 つまでだが、Linux の通知の係は 3 つほどしか並べない）
pub(crate) const MAX_BUTTONS: usize = 3;

/// A notification for [`Ui::notify`](crate::Ui::notify): a title, an optional body and up to
/// three buttons. On Windows it is a toast (it also stays in the notification center); on Linux
/// it goes to the desktop's notification service.
///
/// ```
/// use kargui::Notification;
///
/// let n = Notification::new("Meeting").body("In 5 minutes").button("snooze", "Snooze");
/// # let _ = n;
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct Notification {
    pub(crate) title: String,
    pub(crate) body: String,
    /// （ボタンの id, 文字）
    pub(crate) buttons: Vec<(String, String)>,
    /// 音を鳴らさない
    pub(crate) silent: bool,
}

impl Notification {
    /// A notification with this title.
    ///
    /// ```
    /// let n = kargui::Notification::new("Download finished");
    /// # let _ = n;
    /// ```
    pub fn new(title: impl Into<String>) -> Self {
        Self { title: title.into(), body: String::new(), buttons: Vec::new(), silent: false }
    }

    /// The text under the title.
    ///
    /// ```
    /// let n = kargui::Notification::new("Download finished").body("report.pdf, 2.4 MB");
    /// # let _ = n;
    /// ```
    pub fn body(mut self, text: impl Into<String>) -> Self {
        self.body = text.into();
        self
    }

    /// A button with this `label`; [`Ui::notification_clicked`](crate::Ui::notification_clicked)
    /// answers [`NotificationAction::Button`] with its `id`. Up to three buttons; more are left
    /// out.
    ///
    /// ```
    /// let n = kargui::Notification::new("Download finished").button("open", "Open").button("folder", "Show in folder");
    /// # let _ = n;
    /// ```
    pub fn button(mut self, id: impl Into<String>, label: impl Into<String>) -> Self {
        if self.buttons.len() < MAX_BUTTONS {
            self.buttons.push((id.into(), label.into()));
        }
        self
    }

    /// Shows it without the notification sound (for frequent, unimportant notifications).
    ///
    /// ```
    /// let n = kargui::Notification::new("Synced").silent();
    /// # let _ = n;
    /// ```
    pub fn silent(mut self) -> Self {
        self.silent = true;
        self
    }
}

/// What the user clicked on a notification, from
/// [`Ui::notification_clicked`](crate::Ui::notification_clicked).
///
/// ```
/// use kargui::NotificationAction;
///
/// # kargui::test::example(|ui| {
/// match ui.notification_clicked("download") {
///     Some(NotificationAction::Body) => { /* show the window */ }
///     Some(NotificationAction::Button(id)) if id == "open" => { /* open the file */ }
///     _ => {}
/// }
/// # });
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotificationAction {
    /// The notification itself (not a button).
    Body,
    /// The button with this id ([`Notification::button`]).
    Button(String),
}

/// 1 行の文字（試験台と記録）: 「キー: 題 | 本文 [id: 文字] ...」。本文が無ければ「| 本文」を書かない
pub(crate) fn line(key: &str, n: &Notification) -> String {
    let mut s = format!("{key}: {}", n.title);
    if !n.body.is_empty() {
        s.push_str(&format!(" | {}", n.body));
    }
    for (id, label) in &n.buttons {
        s.push_str(&format!(" [{id}: {label}]"));
    }
    if n.silent {
        s.push_str(" (silent)");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_most_three_buttons_are_kept() {
        let n = Notification::new("t").button("a", "A").button("b", "B").button("c", "C").button("d", "D");
        assert_eq!(n.buttons.len(), 3);
        assert_eq!(line("k", &n), "k: t [a: A] [b: B] [c: C]");
        assert_eq!(line("k", &Notification::new("t").body("b")), "k: t | b");
        assert_eq!(line("k", &Notification::new("t").silent()), "k: t (silent)");
    }
}
