//! Linux のトレイ。StatusNotifierItem（アイコン・説明・押した）と com.canonical.dbusmenu（メニュー）を D-Bus で出し、
//! org.kde.StatusNotifierWatcher に届け出る。KDE、タイル型の合成の帯（Waybar・Quickshell など）、拡張機能を入れた
//! GNOME が表示する。Wayland と X11 で同じ。
//!
//! 窓の処理は止めない: 中身の変更は 1 本のスレッドが順に受けて知らせを送り、D-Bus からの呼び出し（押した・項目を
//! 選んだ）は zbus のスレッドが受けて列に積み、eventfd に書いて窓の処理を起こす（ime_dbus と同じ形）。

use std::collections::HashMap;
use std::os::fd::OwnedFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use zbus::blocking::Connection;
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Structure, Value};

use crate::log;
use crate::ui::tray::{TrayDesc, TrayInput, TrayNode};

const ITEM_PATH: &str = "/StatusNotifierItem";
const ITEM_IFACE: &str = "org.kde.StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";
const MENU_IFACE: &str = "com.canonical.dbusmenu";
const WATCHER: &str = "org.kde.StatusNotifierWatcher";
/// 窓の処理からトレイのスレッドへの頼み
enum Cmd {
    Set(TrayDesc),
    Remove,
}

/// 窓の処理が持つトレイ。落とすとアイコンを消す
pub(crate) struct Tray {
    tx: Sender<Cmd>,
    /// 押した・項目を選んだ知らせ（eventfd で起こされた窓の処理が読む）
    pub rx: Receiver<TrayInput>,
    /// 最後に送った中身（変わった時だけ送る）
    sent: Option<TrayDesc>,
}

impl Tray {
    /// このフレームの中身を送る（前に送った物と同じなら送らない）
    pub fn set(&mut self, d: &TrayDesc) {
        let same = self.sent.as_ref().is_some_and(|s| s.same_icon(d) && s.tooltip == d.tooltip && s.menu == d.menu);
        if !same {
            self.sent = Some(d.clone());
            let _ = self.tx.send(Cmd::Set(d.clone()));
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Remove);
    }
}

/// トレイのスレッドを始める。fallback はアプリがアイコンを渡さなかった時の画像（Options::icon）
pub(crate) fn spawn(fallback: Option<crate::Image>, wake: Arc<OwnedFd>) -> Tray {
    let (tx, cmds) = channel();
    let (input_tx, rx) = channel();
    let poke = Poke { tx: input_tx, wake };
    let spawned = std::thread::Builder::new().name("kargui-tray".into()).spawn(move || run(cmds, poke, fallback));
    if let Err(e) = spawned {
        log::event("tray.error", "spawn", &e.to_string());
    }
    Tray { tx, rx, sent: None }
}

/// D-Bus のスレッドから窓の処理へ知らせる
#[derive(Clone)]
struct Poke {
    tx: Sender<TrayInput>,
    wake: Arc<OwnedFd>,
}

impl Poke {
    fn send(&self, t: TrayInput) {
        let _ = self.tx.send(t);
        let _ = rustix::io::write(&*self.wake, &1u64.to_ne_bytes());
    }
}

/// IconPixmap の形（大きさごとの幅・高さ・ARGB）
type Pixmaps = Vec<(i32, i32, Vec<u8>)>;
/// ToolTip の形（アイコンの名前・アイコン・題・説明）
type ToolTip = (String, Pixmaps, String, String);

/// D-Bus の呼び出しに答える中身（中身の変更のスレッドと、zbus のスレッドが分け合う）
struct Shared {
    desc: TrayDesc,
    /// IconPixmap（大きさごとの ARGB。上位から A・R・G・B の順）
    pixmaps: Pixmaps,
    /// メニューの版（変わるたびに 1 増やし、LayoutUpdated で知らせる）
    revision: u32,
}

fn run(cmds: Receiver<Cmd>, poke: Poke, fallback: Option<crate::Image>) {
    let first = match cmds.recv() {
        Ok(Cmd::Set(d)) => d,
        _ => return,
    };
    let shared = Arc::new(Mutex::new(Shared { pixmaps: pixmaps(&first, &fallback), desc: first, revision: 1 }));
    let name = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
    let item = Item { shared: shared.clone(), poke: poke.clone() };
    let menu = Menu { shared: shared.clone(), poke };
    let conn = zbus::blocking::connection::Builder::session()
        .and_then(|b| b.name(name.as_str()))
        .and_then(|b| b.serve_at(ITEM_PATH, item))
        .and_then(|b| b.serve_at(MENU_PATH, menu))
        .and_then(|b| b.build());
    let conn = match conn {
        Ok(c) => c,
        Err(e) => {
            log::event("tray.error", "connect", &e.to_string());
            return;
        }
    };
    log::event("tray.add", &format!("name={name}"), "served");
    let removed = Arc::new(AtomicBool::new(false));
    register(&conn, &name);
    // 受け手（帯）が後から起動した・起動し直した時に、届け出直す
    {
        let (conn, name, removed) = (conn.clone(), name.clone(), removed.clone());
        let _ = std::thread::Builder::new().name("kargui-tray-watch".into()).spawn(move || watch(conn, name, removed));
    }
    for cmd in cmds {
        match cmd {
            Cmd::Set(d) => update(&conn, &shared, d, &fallback),
            Cmd::Remove => break,
        }
    }
    // 名前を手放すと、受け手がアイコンを消す
    removed.store(true, Ordering::Relaxed);
    let released = conn.release_name(name.as_str());
    log::event("tray.remove", &format!("name={name}"), &format!("{released:?}"));
}

/// 受け手に届け出る。受け手がいなければ記録だけ（GNOME の拡張機能なしなど）
fn register(conn: &Connection, name: &str) {
    let r = conn.call_method(Some(WATCHER), "/StatusNotifierWatcher", Some(WATCHER), "RegisterStatusNotifierItem", &(name,));
    match r {
        Ok(_) => log::event("tray.register", &format!("name={name}"), "ok"),
        Err(e) => log::event("tray.register", &format!("name={name}"), &format!("no watcher: {e}")),
    }
}

/// 受け手の名前の持ち主が変わる知らせを待ち、新しい持ち主が現れたら届け出直す
fn watch(conn: Connection, name: String, removed: Arc<AtomicBool>) {
    let rule = format!("type='signal',sender='org.freedesktop.DBus',interface='org.freedesktop.DBus',member='NameOwnerChanged',arg0='{WATCHER}'");
    let messages = match zbus::blocking::MessageIterator::for_match_rule(rule.as_str(), &conn, Some(8)) {
        Ok(m) => m,
        Err(e) => {
            log::event("tray.error", "watch", &e.to_string());
            return;
        }
    };
    for m in messages.flatten() {
        if removed.load(Ordering::Relaxed) {
            return;
        }
        if let Ok((_, _, new_owner)) = m.body().deserialize::<(String, String, String)>()
            && !new_owner.is_empty()
        {
            log::event("tray.watcher", "", "appeared");
            register(&conn, &name);
        }
    }
}

/// 中身を入れ替え、変わった物を知らせる
fn update(conn: &Connection, shared: &Mutex<Shared>, d: TrayDesc, fallback: &Option<crate::Image>) {
    let (icon, tip, menu, revision) = {
        let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
        let icon = !s.desc.same_icon(&d);
        let tip = s.desc.tooltip != d.tooltip;
        let menu = s.desc.menu != d.menu;
        if icon {
            s.pixmaps = pixmaps(&d, fallback);
        }
        if menu {
            s.revision += 1;
        }
        s.desc = d;
        (icon, tip, menu, s.revision)
    };
    let emit = |iface: &str, path: &str, signal: &str| {
        if let Err(e) = conn.emit_signal(None::<&str>, path, iface, signal, &()) {
            log::event("tray.error", signal, &e.to_string());
        }
    };
    if icon {
        emit(ITEM_IFACE, ITEM_PATH, "NewIcon");
    }
    if tip {
        emit(ITEM_IFACE, ITEM_PATH, "NewToolTip");
        emit(ITEM_IFACE, ITEM_PATH, "NewTitle");
    }
    if menu && let Err(e) = conn.emit_signal(None::<&str>, MENU_PATH, MENU_IFACE, "LayoutUpdated", &(revision, 0i32)) {
        log::event("tray.error", "LayoutUpdated", &e.to_string());
    }
    log::event("tray.update", &format!("icon={icon} tip={tip} menu={menu}"), &format!("revision={revision}"));
}

/// IconPixmap: アプリの画像（無ければ fallback、それも無ければ kargui の無地のアイコン）を 32 と 64 の大きさで。
/// 帯は近い大きさを選んで縮める
fn pixmaps(d: &TrayDesc, fallback: &Option<crate::Image>) -> Pixmaps {
    [32u32, 64]
        .into_iter()
        .map(|size| {
            let image = match d.icon.as_ref().or(fallback.as_ref()) {
                Some(i) => i.resized(size, size),
                None => crate::ui::tray::default_icon(size),
            };
            let mut argb = Vec::with_capacity(image.0.pixels.len());
            for &[r, g, b, a] in image.0.pixels.as_chunks::<4>().0 {
                let al = a as u32;
                let un = |c: u8| (c as u32 * 255 + al / 2).checked_div(al).map_or(0, |v| v.min(255) as u8);
                argb.extend([a, un(r), un(g), un(b)]);
            }
            (size as i32, size as i32, argb)
        })
        .collect()
}

/// org.kde.StatusNotifierItem
struct Item {
    shared: Arc<Mutex<Shared>>,
    poke: Poke,
}

impl Item {
    fn with<R>(&self, f: impl FnOnce(&Shared) -> R) -> R {
        f(&self.shared.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

#[zbus::interface(name = "org.kde.StatusNotifierItem")]
impl Item {
    #[zbus(property)]
    fn category(&self) -> String {
        "ApplicationStatus".into()
    }

    #[zbus(property)]
    fn id(&self) -> String {
        crate::linux::app_id()
    }

    #[zbus(property)]
    fn title(&self) -> String {
        self.with(|s| s.desc.tooltip.clone())
    }

    #[zbus(property)]
    fn status(&self) -> String {
        "Active".into()
    }

    #[zbus(property)]
    fn window_id(&self) -> i32 {
        0
    }

    #[zbus(property)]
    fn icon_name(&self) -> String {
        String::new()
    }

    #[zbus(property)]
    fn icon_pixmap(&self) -> Pixmaps {
        self.with(|s| s.pixmaps.clone())
    }

    #[zbus(property)]
    fn tool_tip(&self) -> ToolTip {
        self.with(|s| (String::new(), Vec::new(), s.desc.tooltip.clone(), String::new()))
    }

    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn menu(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from(MENU_PATH).expect("a valid object path")
    }

    /// 左のボタンで押した（帯が呼ぶ）
    fn activate(&self, x: i32, y: i32) {
        log::event("tray.click", &format!("x={x} y={y}"), "activate");
        self.poke.send(TrayInput::Click);
    }

    /// 中のボタンで押した（今は使わない）
    fn secondary_activate(&self, x: i32, y: i32) {
        log::event("tray.secondary", &format!("x={x} y={y}"), "ignored");
    }

    /// メニューを出すように頼まれた（帯は dbusmenu を使うので、ふつうは呼ばれない）
    fn context_menu(&self, x: i32, y: i32) {
        log::event("tray.context_menu", &format!("x={x} y={y}"), "ignored (the menu is dbusmenu)");
    }

    /// ホイール（今は使わない）
    fn scroll(&self, delta: i32, orientation: String) {
        log::event("tray.scroll", &format!("delta={delta} orientation={orientation}"), "ignored");
    }
}

/// com.canonical.dbusmenu
struct Menu {
    shared: Arc<Mutex<Shared>>,
    poke: Poke,
}

/// 1 つの節の (番号, 性質, 子)
type Layout = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

fn owned(v: Value<'_>) -> OwnedValue {
    OwnedValue::try_from(v).expect("no file descriptors in the menu")
}

/// 項目の性質（dbusmenu の名前）
fn properties(n: &TrayNode) -> HashMap<String, OwnedValue> {
    let mut p = HashMap::new();
    match n {
        TrayNode::Separator { .. } => {
            p.insert("type".into(), owned(Value::from("separator")));
        }
        TrayNode::Item { text, enabled, checked, children, .. } => {
            // _ は次の文字のアクセスキーの印なので、そのまま見せるには重ねる
            p.insert("label".into(), owned(Value::from(text.replace('_', "__"))));
            p.insert("enabled".into(), owned(Value::from(*enabled)));
            if let Some(c) = checked {
                p.insert("toggle-type".into(), owned(Value::from("checkmark")));
                p.insert("toggle-state".into(), owned(Value::from(*c as i32)));
            }
            if !children.is_empty() {
                p.insert("children-display".into(), owned(Value::from("submenu")));
            }
        }
    }
    p
}

fn id_of(n: &TrayNode) -> i32 {
    match n {
        TrayNode::Item { id, .. } | TrayNode::Separator { id } => *id as i32,
    }
}

/// 節の並びを dbusmenu の形にする（depth 段まで。0 なら子を入れない）
fn layout(nodes: &[TrayNode], depth: i32) -> Vec<OwnedValue> {
    nodes
        .iter()
        .map(|n| {
            let children = match n {
                TrayNode::Item { children, .. } if depth > 0 => layout(children, depth - 1),
                _ => Vec::new(),
            };
            owned(Value::from(Structure::from((id_of(n), properties(n), children))))
        })
        .collect()
}

/// 番号の節を探す
fn find(nodes: &[TrayNode], id: i32) -> Option<&TrayNode> {
    nodes.iter().find_map(|n| {
        if id_of(n) == id {
            return Some(n);
        }
        match n {
            TrayNode::Item { children, .. } => find(children, id),
            TrayNode::Separator { .. } => None,
        }
    })
}

impl Menu {
    fn with<R>(&self, f: impl FnOnce(&Shared) -> R) -> R {
        f(&self.shared.lock().unwrap_or_else(|e| e.into_inner()))
    }

    fn node(&self, id: i32) -> Option<TrayNode> {
        self.with(|s| find(&s.desc.menu, id).cloned())
    }
}

#[zbus::interface(name = "com.canonical.dbusmenu")]
impl Menu {
    #[zbus(property)]
    fn version(&self) -> u32 {
        3
    }

    #[zbus(property)]
    fn text_direction(&self) -> String {
        "ltr".into()
    }

    #[zbus(property)]
    fn status(&self) -> String {
        "normal".into()
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> Vec<String> {
        Vec::new()
    }

    /// parent_id の節から recursion_depth 段（-1 は全部）の木を返す（0 は根）
    fn get_layout(&self, parent_id: i32, recursion_depth: i32, _property_names: Vec<String>) -> (u32, Layout) {
        self.with(|s| {
            let depth = if recursion_depth < 0 { i32::MAX } else { recursion_depth };
            let (props, kids) = if parent_id == 0 {
                let mut p = HashMap::new();
                p.insert("children-display".into(), owned(Value::from("submenu")));
                (p, if depth == 0 { Vec::new() } else { layout(&s.desc.menu, depth - 1) })
            } else {
                match find(&s.desc.menu, parent_id) {
                    Some(n @ TrayNode::Item { children, .. }) => (properties(n), if depth == 0 { Vec::new() } else { layout(children, depth - 1) }),
                    Some(n) => (properties(n), Vec::new()),
                    None => (HashMap::new(), Vec::new()),
                }
            };
            log::event("tray.get_layout", &format!("parent={parent_id} depth={recursion_depth}"), &format!("revision={} children={}", s.revision, kids.len()));
            (s.revision, (parent_id, props, kids))
        })
    }

    fn get_group_properties(&self, ids: Vec<i32>, _property_names: Vec<String>) -> Vec<(i32, HashMap<String, OwnedValue>)> {
        ids.into_iter().filter_map(|id| self.node(id).map(|n| (id, properties(&n)))).collect()
    }

    fn get_property(&self, id: i32, name: String) -> zbus::fdo::Result<OwnedValue> {
        let n = self.node(id).ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("no item {id}")))?;
        properties(&n).remove(&name).ok_or_else(|| zbus::fdo::Error::InvalidArgs(format!("no property {name}")))
    }

    /// 項目を押した・開いた・閉じたなど（clicked だけを使う）
    fn event(&self, id: i32, event_id: String, _data: OwnedValue, _timestamp: u32) {
        log::event("tray.dbus_event", &format!("id={id} event={event_id}"), "");
        if event_id == "clicked" && id > 0 {
            self.poke.send(TrayInput::Choose(id as u32));
        }
    }

    fn event_group(&self, events: Vec<(i32, String, OwnedValue, u32)>) -> Vec<i32> {
        for (id, event_id, data, timestamp) in events {
            self.event(id, event_id, data, timestamp);
        }
        Vec::new()
    }

    /// メニューを開く前に、中身を作り直すか（作り直さない。中身はいつも新しい）
    fn about_to_show(&self, _id: i32) -> bool {
        false
    }

    fn about_to_show_group(&self, _ids: Vec<i32>) -> (Vec<i32>, Vec<i32>) {
        (Vec::new(), Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu() -> Vec<TrayNode> {
        let item = |id, text: &str, children| TrayNode::Item { id, text: text.into(), enabled: true, checked: None, children };
        vec![item(1, "Show", vec![]), TrayNode::Separator { id: 2 }, item(3, "Recent", vec![item(4, "a_b.txt", vec![]), TrayNode::Separator { id: 5 }]), item(6, "Quit", vec![])]
    }

    #[test]
    fn nodes_are_found_by_their_number_in_submenus_too() {
        let m = menu();
        assert_eq!(find(&m, 5), Some(&TrayNode::Separator { id: 5 }));
        assert!(matches!(find(&m, 4), Some(TrayNode::Item { id: 4, .. })));
        assert_eq!(find(&m, 9), None);
        // 段の数を限っても、並びの数は同じ（入れ子の中身だけを入れない）
        assert_eq!(layout(&m, 0).len(), 4);
        assert_eq!(layout(&m, i32::MAX).len(), 4);
    }

    #[test]
    fn labels_keep_underscores_and_check_items_say_their_state() {
        let p = properties(&TrayNode::Item { id: 3, text: "a_b.txt".into(), enabled: false, checked: Some(true), children: vec![] });
        assert_eq!(p["label"], owned(Value::from("a__b.txt")));
        assert_eq!(p["enabled"], owned(Value::from(false)));
        assert_eq!(p["toggle-type"], owned(Value::from("checkmark")));
        assert_eq!(p["toggle-state"], owned(Value::from(1i32)));
        assert_eq!(properties(&TrayNode::Separator { id: 2 })["type"], owned(Value::from("separator")));
    }
}
