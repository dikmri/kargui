//! トレイ（通知領域のアイコン）。主の窓のビューの Ui::tray が中身（TrayDesc）を作り、窓の処理が OS に渡す。
//! Windows は Shell_NotifyIconW、Linux は StatusNotifierItem と dbusmenu（D-Bus）。メニューは OS が描くので、
//! kargui のメニュー（Menu）と同じ名前の関数を持つ別の型（TrayMenu）で中身だけを集める。
//!
//! 押した・選んだ知らせは入力（Event::Tray）として届き、入力を渡す回のビューでだけ clicked が true になる。
//! 項目と区切りの番号は、メニューを作った順（深さ優先、1 から）。並びが同じなら次のフレームでも同じ番号になる。

/// トレイの中身（そのフレームのビューが作った物）
#[derive(Clone, Debug, Default)]
pub(crate) struct TrayDesc {
    /// アプリが渡したアイコン（無ければ窓の処理が Options::icon か kargui の無地のアイコンを使う）
    pub icon: Option<crate::Image>,
    pub tooltip: String,
    pub menu: Vec<TrayNode>,
}

impl TrayDesc {
    /// アイコン（作った時の番号）・説明・メニューが同じか。窓の処理は、変わった物だけを OS に送る
    pub fn same_icon(&self, other: &TrayDesc) -> bool {
        self.icon.as_ref().map(|i| i.0.id) == other.icon.as_ref().map(|i| i.0.id)
    }
}

/// メニューの 1 行
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum TrayNode {
    /// 項目（children があれば入れ子のメニュー。checked が Some なら印の付く項目）
    Item { id: u32, text: String, enabled: bool, checked: Option<bool>, children: Vec<TrayNode> },
    /// 区切り（Linux の dbusmenu は区切りにも番号が要る）
    Separator { id: u32 },
}

/// OS から届いたトレイの知らせ
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum TrayInput {
    /// アイコンを押した（Windows は左のボタンか、キーボードで選んで Enter。Linux は Activate）
    Click,
    /// メニューの項目を選んだ（項目の番号）
    Choose(u32),
}

/// The icon in the notification area (the system tray), made in the main window's view with
/// [`Ui::tray`](crate::Ui::tray).
pub struct Tray<'t> {
    pub(crate) desc: &'t mut TrayDesc,
    /// このフレームに届いた知らせ（押した・選んだ。まとめて届くことがある）
    pub(crate) input: Vec<TrayInput>,
}

impl Tray<'_> {
    /// The icon's picture. Without one, the tray shows the window's icon
    /// ([`Options::icon`](crate::Options::icon)) or a plain kargui icon. Keep the [`Image`](crate::Image)
    /// in your state: the icon is sent to the system again only when it is a different image.
    ///
    /// ```
    /// use kargui::Image;
    /// # kargui::test::example(|ui| {
    /// let dot = Image::from_rgba(1, 1, vec![30, 120, 220, 255]).unwrap();
    /// ui.tray(|t| {
    ///     t.icon(&dot);
    /// });
    /// # });
    /// ```
    pub fn icon(&mut self, image: &crate::Image) -> &mut Self {
        self.desc.icon = Some(image.clone());
        self
    }

    /// The text shown when the pointer rests on the icon (and read by screen readers). Use the
    /// app's name, and add the state when it matters ("Notes, 3 unsaved").
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.tray(|t| {
    ///     t.tooltip("Notes");
    /// });
    /// # });
    /// ```
    pub fn tooltip(&mut self, text: impl Into<String>) -> &mut Self {
        self.desc.tooltip = text.into();
        self
    }

    /// Whether the user clicked the icon in this frame (on Windows the left button, or Enter
    /// after moving to the icon with the keyboard; on Linux the tray's "activate"). Apps usually
    /// show their window then. `ui` is in use inside [`Ui::tray`](crate::Ui::tray), so note it and
    /// act after it.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// let mut show = false;
    /// ui.tray(|t| {
    ///     show = t.clicked();
    /// });
    /// if show {
    ///     ui.show_window();
    /// }
    /// # });
    /// ```
    pub fn clicked(&self) -> bool {
        self.input.contains(&TrayInput::Click)
    }

    /// The menu that opens with a right click on the icon (on Linux, however the tray opens it).
    /// The system draws it, so it takes text items, check items, separators and submenus.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// # let mut muted = false;
    /// ui.tray(|t| {
    ///     t.menu(|m| {
    ///         m.item("Show window");
    ///         m.check(&mut muted, "Mute");
    ///         m.separator();
    ///         if m.item("Quit").clicked() {
    ///             // end the app here
    ///         }
    ///     });
    /// });
    /// # });
    /// ```
    pub fn menu(&mut self, f: impl FnOnce(&mut TrayMenu)) -> &mut Self {
        let mut next = 1;
        let mut nodes = Vec::new();
        let chosen: Vec<u32> = self.input.iter().filter_map(|i| if let TrayInput::Choose(id) = i { Some(*id) } else { None }).collect();
        let mut m = TrayMenu { nodes: &mut nodes, next: &mut next, chosen: &chosen };
        f(&mut m);
        self.desc.menu = nodes;
        self
    }
}

/// The tray icon's menu, made in [`Tray::menu`]. The functions have the same names as the menus
/// kargui draws ([`Menu`](crate::ui::Menu)).
pub struct TrayMenu<'m> {
    nodes: &'m mut Vec<TrayNode>,
    next: &'m mut u32,
    /// このフレームに選ばれた項目の番号
    chosen: &'m [u32],
}

impl TrayMenu<'_> {
    fn push(&mut self, text: String, checked: Option<bool>, children: Vec<TrayNode>) -> TrayItem<'_> {
        let id = *self.next;
        *self.next += 1;
        self.nodes.push(TrayNode::Item { id, text, enabled: true, checked, children });
        let chosen = self.chosen.contains(&id);
        TrayItem { node: self.nodes.last_mut().expect("just pushed"), chosen }
    }

    /// An item that does something when chosen. Ask with [`TrayItem::clicked`].
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.tray(|t| {
    ///     t.menu(|m| {
    ///         if m.item("Open notes").clicked() {
    ///             // show the window
    ///         }
    ///     });
    /// });
    /// # });
    /// ```
    pub fn item(&mut self, text: impl Into<String>) -> TrayItem<'_> {
        self.push(text.into(), None, Vec::new())
    }

    /// An item with a check mark that shows `value` and flips it when chosen.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// # let mut start_hidden = false;
    /// ui.tray(|t| {
    ///     t.menu(|m| {
    ///         m.check(&mut start_hidden, "Start in the tray");
    ///     });
    /// });
    /// # });
    /// ```
    pub fn check(&mut self, value: &mut bool, text: impl Into<String>) -> TrayItem<'_> {
        let id = *self.next;
        if self.chosen.contains(&id) {
            *value = !*value;
        }
        self.push(text.into(), Some(*value), Vec::new())
    }

    /// A line between groups of items.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.tray(|t| {
    ///     t.menu(|m| {
    ///         m.item("Show window");
    ///         m.separator();
    ///         m.item("Quit");
    ///     });
    /// });
    /// # });
    /// ```
    pub fn separator(&mut self) {
        let id = *self.next;
        *self.next += 1;
        self.nodes.push(TrayNode::Separator { id });
    }

    /// An item that opens another menu.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.tray(|t| {
    ///     t.menu(|m| {
    ///         m.submenu("Open recent", |m| {
    ///             m.item("notes.txt");
    ///             m.item("todo.txt");
    ///         });
    ///     });
    /// });
    /// # });
    /// ```
    pub fn submenu(&mut self, text: impl Into<String>, f: impl FnOnce(&mut TrayMenu)) {
        let id = *self.next;
        *self.next += 1;
        let mut children = Vec::new();
        let mut m = TrayMenu { nodes: &mut children, next: &mut *self.next, chosen: self.chosen };
        f(&mut m);
        self.nodes.push(TrayNode::Item { id, text: text.into(), enabled: true, checked: None, children });
    }
}

/// An item of the tray icon's menu, made with [`TrayMenu::item`] or [`TrayMenu::check`].
pub struct TrayItem<'m> {
    node: &'m mut TrayNode,
    chosen: bool,
}

impl TrayItem<'_> {
    /// Whether the item can be chosen (a gray item if not).
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// # let saved = true;
    /// ui.tray(|t| {
    ///     t.menu(|m| {
    ///         m.item("Save").enabled(!saved);
    ///     });
    /// });
    /// # });
    /// ```
    pub fn enabled(self, enabled: bool) -> Self {
        if let TrayNode::Item { enabled: e, .. } = &mut *self.node {
            *e = enabled;
        }
        self
    }

    /// Whether the user chose this item in this frame.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.tray(|t| {
    ///     t.menu(|m| {
    ///         if m.item("Quit").clicked() {
    ///             // end the app here
    ///         }
    ///     });
    /// });
    /// # });
    /// ```
    pub fn clicked(&self) -> bool {
        self.chosen && matches!(&*self.node, TrayNode::Item { enabled: true, .. })
    }
}

/// メニューを 1 行ずつの文字にする（試験台と記録）。印の付く項目は「[x] 」「[ ] 」、押せない項目は
/// 「 (disabled)」、入れ子のメニューは「 >」と、中の項目を 2 文字ずつ下げる。区切りは「-」
pub(crate) fn lines(nodes: &[TrayNode]) -> Vec<String> {
    fn walk(nodes: &[TrayNode], depth: usize, out: &mut Vec<String>) {
        for n in nodes {
            let pad = "  ".repeat(depth);
            match n {
                TrayNode::Separator { .. } => out.push(format!("{pad}-")),
                TrayNode::Item { text, enabled, checked, children, .. } => {
                    let mark = match checked {
                        Some(true) => "[x] ",
                        Some(false) => "[ ] ",
                        None => "",
                    };
                    let sub = if children.is_empty() { "" } else { " >" };
                    let off = if *enabled { "" } else { " (disabled)" };
                    out.push(format!("{pad}{mark}{text}{sub}{off}"));
                    walk(children, depth + 1, out);
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(nodes, 0, &mut out);
    out
}

/// 文字が text の最初の項目（深さ優先）の番号
pub(crate) fn find(nodes: &[TrayNode], want: &str) -> Option<u32> {
    nodes.iter().find_map(|n| match n {
        TrayNode::Item { id, text, children, .. } => (text == want).then_some(*id).or_else(|| find(children, want)),
        TrayNode::Separator { .. } => None,
    })
}

/// アプリもアイコンも渡さなかった時のアイコン: 角の丸い四角（kargui の赤）。size × size の画素。縁は 4 × 4 で
/// 数えてなめらかにする
pub(crate) fn default_icon(size: u32) -> crate::Image {
    let s = size.max(1) as f32;
    let (pad, radius) = (s * 0.08, s * 0.22);
    let inside = |x: f32, y: f32| {
        let (lo, hi) = (pad + radius, s - pad - radius);
        let dx = (lo - x).max(x - hi).max(0.0);
        let dy = (lo - y).max(y - hi).max(0.0);
        x >= pad && x <= s - pad && y >= pad && y <= s - pad && dx * dx + dy * dy <= radius * radius
    };
    let mut px = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let mut n = 0u32;
            for sy in 0..4 {
                for sx in 0..4 {
                    n += inside(x as f32 + (sx as f32 + 0.5) / 4.0, y as f32 + (sy as f32 + 0.5) / 4.0) as u32;
                }
            }
            let a = n * 255 / 16;
            // 乗算済み（0xc2 0x3b 0x22 に不透明度を掛ける）
            px.extend([(0xc2 * a / 255) as u8, (0x3b * a / 255) as u8, (0x22 * a / 255) as u8, a as u8]);
        }
    }
    crate::Image::premultiplied(size.max(1), size.max(1), px)
}
