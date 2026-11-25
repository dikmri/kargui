//! Linux の窓（X11）。Wayland（wayland.rs）と同じく、OS との境目をこのファイルに閉じる。
//!
//! X サーバーとは x11rb で話す。libxcb は動く時に読み（dl-libxcb）、キー配列は libxkbcommon-x11 でサーバーの物を
//! そのまま読む（XWayland のキー配列は合成から来るので、名前から作ると合わないことがある）。修飾キーは XKB の
//! StateNotify で受け、押し続けはサーバーが繰り返す（DetectableAutoRepeat で、離した知らせを挟まない）。
//! 絵は CPU の道（cpu_frame）で描き、MIT-SHM の共有メモリ（無ければ PutImage）で窓に出す。サーバーが読み終えた
//! 知らせ（ShmCompletion）が来るまで、次の絵は写さない。倍率は Xft.dpi ÷ 96（無ければ 1）。
//! 待つのは poll の 1 か所（X の接続・ほかのスレッドから起こす eventfd・時間）。
//!
//! クリップボードは CLIPBOARD の選択（文字だけ。大きい物は INCR で受け取る）。読み上げ・OS の明暗の設定・
//! ファイルのダイアログは Wayland と同じ（linux.rs と portal.rs）。題の帯はウィンドウマネージャーが描く。
//!
//! 日本語入力は fcitx5 か IBus の D-Bus（ime_dbus.rs が選ぶ。fcitx.rs・ibus.rs）。キーをまず IME に渡し、
//! 使わなかったキーだけをアプリに渡す。
//!
//! ほかのアプリから引っ張ってきたファイルは XDND（版 5）。最初に動いた時（XdndPosition）に text/uri-list を受け取り、
//! Wayland と同じ FileDragEnter にする。
//!
//! まだ無い物: 窓の外への引っ張り出し。

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::os::fd::OwnedFd;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use accesskit::ActionRequest;
use rustix::event::{EventfdFlags, PollFd, PollFlags, Timespec, eventfd, poll};
use rustix::fs::{MemfdFlags, ftruncate, memfd_create};
use rustix::mm::{MapFlags, ProtFlags, mmap, munmap};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::Event as XEvent;
use x11rb::protocol::shm::{self, ConnectionExt as _};
use x11rb::protocol::xkb::{self, ConnectionExt as _};
use x11rb::protocol::xproto::{self, AtomEnum, ConnectionExt as _, EventMask, PropMode, Window};
use x11rb::wrapper::ConnectionExt as _;
use x11rb::xcb_ffi::XCBConnection;

use crate::app::{App, Frame, Options};
use crate::cpu_frame;
use crate::damage::Summary;
use crate::ime_dbus;
use crate::geom::{Color, IRect};
use crate::ime;
use crate::input::{A11yAction, Event, Mods};
use crate::linux::{A11ySink, DOUBLE_CLICK_DIST, DOUBLE_CLICK_MS, Keyboard, PortalSettings, WHEEL_STEP, app_id, key_for};
use crate::log;
use crate::runtime::Runtime;
use crate::text::Text;
use crate::ui::Id;
use crate::ui::context::{Clipboard, PointerShape};

/// クリップボードの答えを待つ長さ（Wayland と同じ）
const PASTE_TIMEOUT: Duration = Duration::from_secs(1);
/// 動いている間に次のフレームを描くまでの間（X11 にはフレームの知らせが無いので、60 Hz の時計で起きる）
const ANIMATION_STEP: Duration = Duration::from_millis(16);

x11rb::atom_manager! {
    /// 使う名前（atom）
    Atoms: AtomsCookie {
        WM_PROTOCOLS,
        WM_DELETE_WINDOW,
        _NET_WM_PING,
        _NET_WM_NAME,
        _NET_WM_PID,
        _NET_WM_ICON,
        _NET_ACTIVE_WINDOW,
        UTF8_STRING,
        CLIPBOARD,
        TARGETS,
        TEXT,
        INCR,
        TEXT_PLAIN_UTF8: b"text/plain;charset=utf-8",
        KARGUI_CLIP,
        XdndAware,
        XdndEnter,
        XdndPosition,
        XdndStatus,
        XdndLeave,
        XdndDrop,
        XdndFinished,
        XdndSelection,
        XdndTypeList,
        XdndActionCopy,
        TEXT_URI_LIST: b"text/uri-list",
    }
}

fn error(what: &str, e: impl std::fmt::Display) -> crate::Error {
    crate::Error(format!("x11: {what}: {e}"))
}

/// 窓の番号（X の窓の番号をそのまま使う）
type WinId = Window;

/// クリップボード（CLIPBOARD の選択）。持ち主になる見えない窓で受け渡す
struct Clip {
    conn: Rc<XCBConnection>,
    atoms: Atoms,
    /// 選択の受け渡しに使う見えない窓（PropertyChange を受ける）
    owner: Window,
    /// こちらが持ち主の時の文字（ほかのアプリが持ち主になったら SelectionClear で消す）
    ours: Option<String>,
    /// 最後の入力の時刻（X の時刻。持ち主になる時に渡す）
    time: u32,
    /// 貼り付けの答えを待つ間に届いた、ほかの知らせ（窓の処理が後で読む）
    stash: VecDeque<XEvent>,
}

impl Clip {
    /// 選択の答え（SelectionNotify）を待って、中身を読む。断られたら Some(None)、時間切れは None
    fn wait_selection(&mut self) -> Option<Option<Vec<u8>>> {
        let deadline = Instant::now() + PASTE_TIMEOUT;
        loop {
            while let Ok(Some(ev)) = self.conn.poll_for_event() {
                match ev {
                    XEvent::SelectionNotify(e) if e.requestor == self.owner => {
                        if e.property == x11rb::NONE {
                            return Some(None);
                        }
                        return Some(self.read_property());
                    }
                    other => self.stash.push_back(other),
                }
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            let mut fds = [PollFd::new(&*self.conn, PollFlags::IN)];
            let ts = Timespec { tv_sec: 0, tv_nsec: left.min(Duration::from_millis(20)).as_nanos() as _ };
            let _ = poll(&mut fds, Some(&ts));
        }
    }

    /// 受け渡しの窓の KARGUI_CLIP を読んで消す。INCR（大きい物）なら、少しずつ届く分をつなぐ
    fn read_property(&mut self) -> Option<Vec<u8>> {
        let r = self.conn.get_property(true, self.owner, self.atoms.KARGUI_CLIP, AtomEnum::ANY, 0, u32::MAX / 4).ok()?.reply().ok()?;
        if r.type_ != self.atoms.INCR {
            return Some(r.value);
        }
        let _ = self.conn.flush();
        let mut all = Vec::new();
        let deadline = Instant::now() + PASTE_TIMEOUT * 5;
        loop {
            while let Ok(Some(ev)) = self.conn.poll_for_event() {
                match ev {
                    XEvent::PropertyNotify(p) if p.window == self.owner && p.atom == self.atoms.KARGUI_CLIP && p.state == xproto::Property::NEW_VALUE => {
                        let part = self.conn.get_property(true, self.owner, self.atoms.KARGUI_CLIP, AtomEnum::ANY, 0, u32::MAX / 4).ok()?.reply().ok()?;
                        let _ = self.conn.flush();
                        if part.value.is_empty() {
                            log::event("clipboard.incr", "", &format!("bytes={}", all.len()));
                            return Some(all);
                        }
                        all.extend_from_slice(&part.value);
                    }
                    other => self.stash.push_back(other),
                }
            }
            if Instant::now() >= deadline {
                log::event("clipboard.incr", "", "timeout");
                return None;
            }
            let mut fds = [PollFd::new(&*self.conn, PollFlags::IN)];
            let ts = Timespec { tv_sec: 0, tv_nsec: 20_000_000 };
            let _ = poll(&mut fds, Some(&ts));
        }
    }
}

struct X11Clipboard(Rc<RefCell<Clip>>);

impl Clipboard for X11Clipboard {
    fn set(&mut self, text: &str) {
        let mut c = self.0.borrow_mut();
        let ok = c.conn.set_selection_owner(c.owner, c.atoms.CLIPBOARD, c.time).is_ok();
        let _ = c.conn.flush();
        c.ours = ok.then(|| text.to_string());
        log::event("clipboard.set", &format!("chars={} time={}", text.chars().count(), c.time), if ok { "owner" } else { "error" });
    }

    fn get(&mut self) -> Option<String> {
        let mut c = self.0.borrow_mut();
        // 自分の物は X サーバーを通さずに返す
        if let Some(t) = &c.ours {
            log::event("clipboard.get", "", &format!("ours chars={}", t.chars().count()));
            return Some(t.clone());
        }
        let t0 = Instant::now();
        for target in [c.atoms.UTF8_STRING, AtomEnum::STRING.into()] {
            let (owner, clip, prop, time) = (c.owner, c.atoms.CLIPBOARD, c.atoms.KARGUI_CLIP, c.time);
            if c.conn.convert_selection(owner, clip, target, prop, time).is_err() {
                return None;
            }
            let _ = c.conn.flush();
            match c.wait_selection() {
                Some(Some(bytes)) => {
                    let text = String::from_utf8_lossy(&bytes).into_owned();
                    log::event("clipboard.get", &format!("target={target}"), &format!("chars={} ms={:.1}", text.chars().count(), t0.elapsed().as_secs_f64() * 1000.0));
                    return Some(text);
                }
                Some(None) => continue,
                None => {
                    log::event("clipboard.get", &format!("target={target}"), "timeout");
                    return None;
                }
            }
        }
        log::event("clipboard.get", "", "refused (no text)");
        None
    }
}

/// ほかのアプリから引っ張ってきている物（XDND）
struct XDrag {
    win: WinId,
    /// 引っ張り元の窓
    source: Window,
    /// ファイル（text/uri-list）がある。場所の一覧は最初の XdndPosition で受け取る
    files: bool,
    received: bool,
    /// 窓のルート窓の上の位置（最初の XdndPosition で測る）
    origin: Option<(i32, i32)>,
    /// 窓の中の位置（物理ピクセル）
    at: (f32, f32),
    accepting: bool,
}

/// 日本語入力（fcitx5 か IBus の D-Bus）の状態
struct XIme {
    tx: Sender<ime_dbus::Cmd>,
    rx: Receiver<ime_dbus::Msg>,
    /// 入力の文脈ができた
    ready: bool,
    /// IME の焦点を持つ（持っていた）窓。焦点を外した後に届く確定もこの窓に渡す
    win: Option<WinId>,
    /// IME の焦点を置いている入力欄（None = 外している）
    enabled: Option<Id>,
    /// 最後に知らせた文字を打つ位置（ルート窓の上）と、その元の窓の中の位置
    rect: Option<(i32, i32, i32, i32)>,
    caret: Option<(WinId, i32, i32, i32, i32)>,
    /// 窓が動いた（ルート窓の上の位置を測り直す）
    moved: bool,
    /// 今見せている変換中の文字
    shown: String,
    /// IME の答えを待っているキー（番号, 窓, キー, 繰り返しか, 離したか）
    pending: VecDeque<(u64, WinId, u8, bool, bool)>,
    next_id: u64,
}

/// MIT-SHM の共有メモリ（窓の絵と同じ大きさ）
struct ShmBuf {
    seg: shm::Seg,
    map: *mut u8,
    len: usize,
    width: i32,
    height: i32,
}

impl ShmBuf {
    fn new(conn: &XCBConnection, width: i32, height: i32) -> Result<ShmBuf, crate::Error> {
        let len = (width.max(1) * height.max(1) * 4) as usize;
        let fd: OwnedFd = memfd_create("kargui-x11", MemfdFlags::CLOEXEC).map_err(|e| error("memfd", e))?;
        ftruncate(&fd, len as u64).map_err(|e| error("ftruncate", e))?;
        // SAFETY: 作ったばかりの memfd を、その長さで写す。外すのは free
        let map = unsafe { mmap(std::ptr::null_mut(), len, ProtFlags::READ | ProtFlags::WRITE, MapFlags::SHARED, &fd, 0) }.map_err(|e| error("mmap", e))? as *mut u8;
        let seg = conn.generate_id().map_err(|e| error("generate_id", e))?;
        let attached = conn.shm_attach_fd(seg, fd, true).map_err(|e| error("shm_attach_fd", e)).and_then(|c| c.check().map_err(|e| error("shm_attach_fd", e)));
        if let Err(e) = attached {
            // SAFETY: 上で写した所を外す
            unsafe {
                let _ = munmap(map as *mut _, len);
            }
            return Err(e);
        }
        Ok(ShmBuf { seg, map, len, width, height })
    }

    fn rows(&mut self) -> &mut [u32] {
        // SAFETY: map は len バイト（幅 × 高さ × 4）を写してある。u32 の並びとして使う
        unsafe { std::slice::from_raw_parts_mut(self.map as *mut u32, self.len / 4) }
    }

    fn free(self, conn: &XCBConnection) {
        let _ = conn.shm_detach(self.seg);
        // SAFETY: new で写した所を、同じ長さで外す
        unsafe {
            let _ = munmap(self.map as *mut _, self.len);
        }
    }
}

/// 窓 1 つ分（主の窓と、Ui::window で開いた窓）
struct Win {
    id: WinId,
    /// None は主の窓
    key: Option<String>,
    rt: Runtime,
    a11y: accesskit_unix::Adapter,
    a11y_actions: Arc<Mutex<Vec<ActionRequest>>>,
    /// CPU の道の絵（前のフレームの絵。変わった所だけ描き直す）と、その要約と背景の色
    pixels: Vec<u32>,
    summary: Summary,
    background: Color,
    /// 物理の大きさ
    size: (i32, i32),
    shm: Option<ShmBuf>,
    /// サーバーが共有メモリを読み終えていない（ShmCompletion を待つ）
    shm_busy: bool,
    /// 窓が画面に出た（MapNotify。出る前の絵は捨てられる）
    mapped: bool,
    /// 窓の一部が見えるようになった（Expose）。次は絵の全部を出す
    exposed: bool,
    events: Vec<Event>,
    need_frame: bool,
    why: &'static str,
    full: bool,
    animating: bool,
    wake_at: Option<Instant>,
    focused: bool,
    shown: bool,
    /// 今のポインターの形の名前
    cursor: &'static str,
    created: Instant,
}

impl Win {
    fn tag(&self) -> String {
        self.key.as_ref().map_or(String::new(), |k| format!(" win={k}"))
    }

    fn push(&mut self, e: Event) {
        self.events.push(e);
        self.need_frame = true;
        self.why = "input";
    }

    fn apply_a11y_actions(&mut self) {
        let requests: Vec<ActionRequest> = self.a11y_actions.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default();
        for r in requests {
            let action = match r.action {
                accesskit::Action::Click => A11yAction::Click,
                accesskit::Action::Focus => A11yAction::Focus,
                accesskit::Action::Increment => A11yAction::Increment,
                accesskit::Action::Decrement => A11yAction::Decrement,
                other => {
                    log::event("a11y.action", &format!("action={other:?}"), "ignored");
                    continue;
                }
            };
            self.push(Event::A11y { target: Id(r.target_node.0), action });
        }
    }
}

struct State {
    app: Box<dyn App>,
    conn: Rc<XCBConnection>,
    atoms: Atoms,
    root: Window,
    depth: u8,
    visual: xproto::Visualid,
    gc: xproto::Gcontext,
    /// 倍率（Xft.dpi ÷ 96）
    scale: f32,
    /// MIT-SHM が使える
    shm_ok: bool,
    windows: Vec<Win>,
    font_files: Vec<std::borrow::Cow<'static, [u8]>>,
    main_icon: Option<crate::Image>,
    xkb: Option<Keyboard>,
    /// 日本語入力（fcitx5 も IBus も無ければ None）
    ime: Option<XIme>,
    /// ほかのアプリから引っ張ってきている物
    dnd: Option<XDrag>,
    /// 押しているキー（サーバーが繰り返した押しを見分ける）
    pressed: Vec<u8>,
    clip: Rc<RefCell<Clip>>,
    cursor_handle: Option<x11rb::cursor::Handle>,
    cursors: HashMap<&'static str, xproto::Cursor>,
    wake: Arc<OwnedFd>,
    dialog_tx: Sender<(WinId, String, Vec<PathBuf>)>,
    dialog_rx: Receiver<(WinId, String, Vec<PathBuf>)>,
    dialog_open: bool,
    last_folder: Option<PathBuf>,
    settings_rx: Option<Receiver<PortalSettings>>,
    /// トレイ（主の窓のビューが Ui::tray を呼んでいる間）と、アプリがアイコンを渡さない時の画像
    tray: Option<crate::sni::Tray>,
    tray_fallback: Option<crate::Image>,
    running: bool,
    pointer_win: Option<WinId>,
    keyboard_win: Option<WinId>,
    px: f32,
    py: f32,
    left_down: bool,
    /// 前に押した時（ms, x, y, 何回目か）
    last_press: Option<(u32, f32, f32, u8)>,
    started: Instant,
}

/// 窓のアイコン（_NET_WM_ICON の形: 幅・高さ・乗算していない ARGB の並び）
fn net_wm_icon(image: &crate::Image) -> Vec<u32> {
    let d = &image.0;
    let mut out = Vec::with_capacity(2 + (d.width * d.height) as usize);
    out.push(d.width);
    out.push(d.height);
    for p in d.pixels.as_chunks::<4>().0 {
        let a = p[3] as u32;
        let un = |c: u8| (c as u32 * 255 + a / 2).checked_div(a).unwrap_or(0).min(255);
        out.push((a << 24) | (un(p[0]) << 16) | (un(p[1]) << 8) | un(p[2]));
    }
    out
}

impl State {
    fn index(&self, id: WinId) -> Option<usize> {
        self.windows.iter().position(|w| w.id == id)
    }

    fn win(&mut self, id: WinId) -> Option<&mut Win> {
        self.windows.iter_mut().find(|w| w.id == id)
    }

    fn physical(&self, logical: f32) -> u16 {
        (logical * self.scale).round().clamp(1.0, 32767.0) as u16
    }

    /// 窓を作って出す。最初の MapNotify が来てから描く
    #[allow(clippy::too_many_arguments)]
    fn new_window(&mut self, key: Option<String>, title: &str, size: (f32, f32), min_size: Option<(f32, f32)>, position: Option<(f32, f32)>, icon: Option<&crate::Image>, mut rt: Runtime) -> Result<WinId, crate::Error> {
        // 窓のキー（ドッキングが、別の窓に出したまとまりを見分ける）。画面の上の場所は、最初の ConfigureNotify で入る
        rt.cx.window_key = key.clone();
        let conn = self.conn.clone();
        let id = conn.generate_id().map_err(|e| error("generate_id", e))?;
        let (w, h) = (self.physical(size.0), self.physical(size.1));
        let (x, y) = position.map_or((0, 0), |(x, y)| (self.physical(x) as i16, self.physical(y) as i16));
        let mask = EventMask::EXPOSURE
            | EventMask::STRUCTURE_NOTIFY
            | EventMask::KEY_PRESS
            | EventMask::KEY_RELEASE
            | EventMask::BUTTON_PRESS
            | EventMask::BUTTON_RELEASE
            | EventMask::POINTER_MOTION
            | EventMask::ENTER_WINDOW
            | EventMask::LEAVE_WINDOW
            | EventMask::FOCUS_CHANGE;
        let bg = rt.cx.theme.background;
        let px = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u32;
        let aux = xproto::CreateWindowAux::new().background_pixel((px(bg[0]) << 16) | (px(bg[1]) << 8) | px(bg[2])).event_mask(mask);
        conn.create_window(self.depth, id, self.root, x, y, w, h, 0, xproto::WindowClass::INPUT_OUTPUT, self.visual, &aux).map_err(|e| error("create_window", e))?;
        let a = self.atoms;
        let _ = conn.change_property32(PropMode::REPLACE, id, a.WM_PROTOCOLS, AtomEnum::ATOM, &[a.WM_DELETE_WINDOW, a._NET_WM_PING]);
        let _ = conn.change_property8(PropMode::REPLACE, id, a._NET_WM_NAME, a.UTF8_STRING, title.as_bytes());
        let _ = conn.change_property8(PropMode::REPLACE, id, AtomEnum::WM_NAME, AtomEnum::STRING, title.as_bytes());
        let class = app_id();
        let _ = conn.change_property8(PropMode::REPLACE, id, AtomEnum::WM_CLASS, AtomEnum::STRING, format!("{class}\0{class}\0").as_bytes());
        let _ = conn.change_property32(PropMode::REPLACE, id, a._NET_WM_PID, AtomEnum::CARDINAL, &[std::process::id()]);
        // ほかのアプリから落とされる物を受け取れる（XDND の版 5）
        let _ = conn.change_property32(PropMode::REPLACE, id, a.XdndAware, AtomEnum::ATOM, &[5]);
        let mut hints = x11rb::properties::WmSizeHints::new();
        hints.min_size = min_size.map(|(mw, mh)| (self.physical(mw) as i32, self.physical(mh) as i32));
        if position.is_some() {
            hints.position = Some((x11rb::properties::WmSizeHintsSpecification::UserSpecified, x as i32, y as i32));
        }
        let _ = hints.set_normal_hints(&*conn, id);
        if let Some(img) = icon {
            let _ = conn.change_property32(PropMode::REPLACE, id, a._NET_WM_ICON, AtomEnum::CARDINAL, &net_wm_icon(img));
        }
        conn.map_window(id).map_err(|e| error("map_window", e))?;
        let _ = conn.flush();
        let a11y_actions = Arc::new(Mutex::new(Vec::new()));
        let sink = A11ySink { actions: a11y_actions.clone(), wake: self.wake.clone() };
        let a11y = accesskit_unix::Adapter::new(sink.clone(), sink.clone(), sink);
        let mut win = Win {
            id,
            key,
            rt,
            a11y,
            a11y_actions,
            pixels: Vec::new(),
            summary: Summary::default(),
            background: [0.0; 4],
            size: (w as i32, h as i32),
            shm: None,
            shm_busy: false,
            mapped: false,
            exposed: false,
            events: Vec::new(),
            need_frame: true,
            why: "first",
            full: true,
            animating: false,
            wake_at: None,
            focused: false,
            shown: false,
            cursor: "",
            created: if self.windows.is_empty() { self.started } else { Instant::now() },
        };
        win.rt.resize(w as f32, h as f32, self.scale);
        log::event("window.size", &format!("logical={:.0}x{:.0} scale={:.3}{}", size.0, size.1, self.scale, win.tag()), &format!("physical={w}x{h}"));
        self.windows.push(win);
        Ok(id)
    }

    fn runtime(&self, title: &str, dark: bool, font: &crate::Font) -> Result<Runtime, crate::Error> {
        let text = Text::new()?;
        let mut rt = Runtime::new(text, dark, Box::new(X11Clipboard(self.clip.clone())), title.to_string());
        let writer = self.wake.clone();
        rt.cx.waker = crate::wake::Waker::new(move || {
            let _ = rustix::io::write(&*writer, &1u64.to_ne_bytes());
        });
        rt.cx.text.load_fonts(&self.font_files);
        rt.cx.font = rt.cx.text.fonts.id(font);
        Ok(rt)
    }

    /// Ui::window で頼まれた窓を開く。X11 では置く場所とアイコンも渡せる（前面に出すかはウィンドウマネージャーが決める）
    fn open_window(&mut self, r: crate::ui::context::WindowRequest) {
        let main = &self.windows[0].rt;
        let (dark, font) = (main.cx.dark, main.cx.text.fonts.get(main.cx.font).clone());
        let icon = r.icon.clone().or_else(|| self.main_icon.clone());
        let made = self.runtime(&r.title, dark, &font).and_then(|rt| self.new_window(Some(r.key.clone()), &r.title, r.size, r.min_size, r.position, icon.as_ref(), rt));
        match made {
            Ok(id) => log::event("window.open_other", &format!("key={} size={:.0}x{:.0} position={:?}", r.key, r.size.0, r.size.1, r.position), &format!("id=0x{id:x}")),
            Err(e) => log::event("window.open_other", &format!("key={}", r.key), &format!("error={e}")),
        }
    }

    fn close_window(&mut self, id: WinId) {
        let Some(i) = self.index(id) else { return };
        let mut w = self.windows.remove(i);
        if let Some(s) = w.shm.take() {
            s.free(&self.conn);
        }
        let _ = self.conn.destroy_window(id);
        let _ = self.conn.flush();
        log::event("window.close_other", &format!("key={}", w.key.as_deref().unwrap_or("")), "ok");
        drop(w);
        for slot in [&mut self.pointer_win, &mut self.keyboard_win] {
            if *slot == Some(id) {
                *slot = None;
            }
        }
    }

    fn sync_windows(&mut self, requested: Vec<crate::ui::context::WindowRequest>) {
        let new: Vec<_> = requested.iter().filter(|r| !self.windows.iter().any(|w| w.key.as_deref() == Some(r.key.as_str()))).cloned().collect();
        for r in new {
            self.open_window(r);
        }
        let gone: Vec<WinId> = self.windows.iter().filter(|w| w.key.as_ref().is_some_and(|k| !requested.iter().any(|r| &r.key == k))).map(|w| w.id).collect();
        for id in gone {
            self.close_window(id);
        }
    }

    /// 窓の閉じるボタン（WM_DELETE_WINDOW）
    fn close_requested(&mut self, id: WinId) {
        let key = self.win(id).and_then(|w| w.key.clone());
        match key {
            None => {
                let ok = self.app.close_requested();
                log::event("window.close_requested", "", &format!("allowed={ok}"));
                if ok {
                    self.running = false;
                } else if let Some(w) = self.windows.first_mut() {
                    w.need_frame = true;
                }
            }
            Some(key) => {
                log::event("window.close_requested", &format!("key={key}"), "closed by the user");
                if let Some(main) = self.windows.first_mut() {
                    main.rt.cx.closed_windows.insert(key);
                    main.need_frame = true;
                    main.why = "input";
                }
            }
        }
    }

    fn open_file_dialog(&mut self, win: WinId, key: String, mut d: crate::file_dialog::FileDialog) {
        if self.dialog_open {
            log::event("file_dialog", &format!("key={key}"), "ignored (a dialog is already open)");
            return;
        }
        self.dialog_open = true;
        if d.folder.is_none() {
            d.folder = self.last_folder.clone();
        }
        let parent = format!("x11:{win:x}");
        log::event("file_dialog", &format!("key={key} save={} multiple={} filters={} folder={:?} parent={parent}", d.save, d.allows_multiple(), d.filters.len(), d.folder), "portal");
        if !crate::linux::spawn_file_dialog(win, key, d, parent, self.dialog_tx.clone(), self.wake.clone()) {
            self.dialog_open = false;
        }
    }

    /// 窓を出す（地図に載せて前に出す）か隠す（地図から外す）。Ui::show_window・hide_window
    fn present(&mut self, id: WinId, show: bool) {
        if show {
            let _ = self.conn.map_window(id);
            let _ = self.conn.configure_window(id, &xproto::ConfigureWindowAux::new().stack_mode(xproto::StackMode::ABOVE));
            // 窓の管理役に、前に出して焦点を移すよう頼む（1 はアプリからの頼み）
            let ev = xproto::ClientMessageEvent::new(32, id, self.atoms._NET_ACTIVE_WINDOW, [1, 0, 0, 0, 0]);
            let _ = self.conn.send_event(false, self.root, EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT, ev);
            log::event("window.show", &format!("win={id}"), "map");
        } else {
            let _ = self.conn.unmap_window(id);
            log::event("window.hide", &format!("win={id}"), "unmap");
        }
        let _ = self.conn.flush();
    }

    /// 別のスレッドから届いた答え（ファイルのダイアログ・OS の設定）を受け取る
    fn take_answers(&mut self) {
        while let Ok((win, key, files)) = self.dialog_rx.try_recv() {
            if let Some(dir) = files.first().and_then(|f| f.parent()) {
                self.last_folder = Some(dir.to_path_buf());
            }
            self.dialog_open = false;
            if let Some(w) = self.win(win) {
                w.rt.cx.file_results.insert(key, files);
                w.need_frame = true;
            }
        }
        if let Some(rx) = &self.settings_rx
            && let Ok(s) = rx.try_recv()
        {
            self.settings_rx = None;
            self.apply_settings(s, "late (after the first frame)");
        }
        // トレイを押した・項目を選んだ（主の窓の入力にする）
        let inputs: Vec<crate::ui::tray::TrayInput> = self.tray.as_ref().map(|t| t.rx.try_iter().collect()).unwrap_or_default();
        for t in inputs {
            if let Some(w) = self.windows.iter_mut().find(|w| w.key.is_none()) {
                log::event("tray.input", &format!("{t:?}"), "main window");
                w.events.push(crate::input::Event::Tray(t));
                w.need_frame = true;
                w.why = "input";
            }
        }
    }

    /// 主の窓のビューが作ったトレイの中身を、トレイのスレッドに渡す（無ければトレイを消す）
    fn sync_tray(&mut self, want: Option<crate::ui::tray::TrayDesc>) {
        match want {
            Some(d) => {
                if self.tray.is_none() {
                    log::event("tray.start", &format!("tip={:?}", d.tooltip), "spawn");
                    self.tray = Some(crate::sni::spawn(self.tray_fallback.clone(), self.wake.clone()));
                }
                if let Some(t) = self.tray.as_mut() {
                    t.set(&d);
                }
            }
            None => {
                if self.tray.take().is_some() {
                    log::event("tray.stop", "", "the view stopped making the tray");
                }
            }
        }
    }

    fn apply_settings(&mut self, s: PortalSettings, when: &str) {
        if let Some(d) = s.dark {
            log::event("theme.system", &format!("dark={d}"), when);
            for w in &mut self.windows {
                w.rt.cx.set_dark(d);
                w.need_frame = true;
            }
        }
    }

    fn mods(&self) -> Mods {
        self.xkb.as_ref().map_or(Mods::NONE, |k| k.mods())
    }

    /// ポインターの形を名前で決めて、窓に付ける（無い名前は古い名前で探す）
    fn set_cursor(&mut self, id: WinId, name: &'static str) {
        let Some(i) = self.index(id) else { return };
        if self.windows[i].cursor == name {
            return;
        }
        let fallback: &[&str] = match name {
            "text" => &["text", "xterm"],
            "pointer" => &["pointer", "hand2"],
            "ns-resize" => &["ns-resize", "sb_v_double_arrow"],
            "ew-resize" => &["ew-resize", "sb_h_double_arrow"],
            "move" => &["move", "fleur"],
            "nwse-resize" => &["nwse-resize", "bottom_right_corner"],
            "not-allowed" => &["not-allowed", "crossed_circle"],
            _ => &["default", "left_ptr"],
        };
        let cursor = match self.cursors.get(name) {
            Some(&c) => c,
            None => {
                let mut found = x11rb::NONE;
                if let Some(h) = &self.cursor_handle {
                    for n in fallback {
                        if let Ok(c) = h.load_cursor(&*self.conn, n)
                            && c != x11rb::NONE
                        {
                            found = c;
                            break;
                        }
                    }
                }
                self.cursors.insert(name, found);
                found
            }
        };
        let _ = self.conn.change_window_attributes(id, &xproto::ChangeWindowAttributesAux::new().cursor(cursor));
        self.windows[i].cursor = name;
        log::event("cursor.shape", &format!("x={} y={}{}", self.px, self.py, self.windows[i].tag()), &format!("{name} id={cursor}"));
    }

    /// ポインターの下の部品から形を決める（Wayland の update_shape と同じ）
    fn update_shape(&mut self) {
        let Some(id) = self.pointer_win else { return };
        let Some(i) = self.index(id) else { return };
        let (x, y) = (self.px, self.py);
        let name = match self.windows[i].rt.cx.pointer_shape(x, y) {
            PointerShape::NsResize => "ns-resize",
            PointerShape::EwResize => "ew-resize",
            PointerShape::Move => "move",
            PointerShape::NwseResize => "nwse-resize",
            PointerShape::Text => "text",
            PointerShape::Pointer => "pointer",
            PointerShape::Default => "default",
        };
        self.set_cursor(id, name);
    }

    fn pointer_motion(&mut self, id: WinId, x: f32, y: f32) {
        self.px = x;
        self.py = y;
        self.pointer_win = Some(id);
        let left_down = self.left_down;
        let Some(w) = self.win(id) else { return };
        let cx = &w.rt.cx;
        let inner_drag = cx.dnd.as_ref().is_some_and(|d| !d.external);
        if cx.drag.is_some() || cx.pressed.is_some() || cx.press_source.is_some() || inner_drag {
            let mut shape = None;
            if left_down {
                w.push(Event::PointerMove { x, y });
                if let Some(ok) = w.rt.cx.drag_cursor(x, y) {
                    shape = Some(if ok { "default" } else { "not-allowed" });
                }
            } else if inner_drag || w.rt.cx.press_source.is_some() {
                log::event("drag.lost", &format!("x={x} y={y}"), "cancel");
                w.push(Event::PointerCancel);
            } else {
                w.push(Event::PointerUp { x, y });
            }
            if let Some(s) = shape {
                self.set_cursor(id, s);
            }
            return;
        }
        let hover = w.rt.hover_at(x, y);
        let bar = w.rt.cx.scrollbar_at(x, y).map(|l| l.id);
        if hover != w.rt.cx.hover {
            log::event("hover", &format!("x={x} y={y}"), &format!("id={}", hover.map_or("none".to_string(), |h| format!("{:x}", h.0))));
        }
        if hover != w.rt.cx.hover || bar != w.rt.cx.bar_hover {
            w.rt.cx.set_hover(hover);
            w.rt.cx.bar_hover = bar;
            w.need_frame = true;
            w.why = "input";
        }
        self.update_shape();
    }

    fn key_down(&mut self, id: WinId, code: u32, repeat: bool) {
        let Some(kb) = self.xkb.as_ref() else { return };
        let mods = kb.mods();
        let key = key_for(kb.base_sym(code), kb.sym(code));
        let text = if mods.ctrl || mods.alt { String::new() } else { kb.utf8(code) };
        let Some(w) = self.win(id) else { return };
        if let Some(key) = key {
            if !repeat {
                log::event("key", &format!("key={key:?} mods={mods:?}"), "sent");
            }
            w.push(Event::Key { key, mods });
        }
        for c in text.chars().filter(|c| !c.is_control()) {
            if !repeat {
                log::event("char", &format!("u=0x{:04X}", c as u32), "insert");
            }
            w.push(Event::Char(c));
        }
    }

    /// このキーを IME に渡すか（文脈ができていて、この窓の入力欄に焦点がある時。答えを待つキーがある間も、順番を守るため渡す）
    fn ime_takes(&self, win: WinId) -> bool {
        self.ime.as_ref().is_some_and(|m| m.ready && ((m.enabled.is_some() && m.win == Some(win)) || !m.pending.is_empty()))
    }

    fn send_key_to_ime(&mut self, win: WinId, code: u8, state: u16, time: u32, repeat: bool, release: bool) {
        let keyval = self.xkb.as_ref().map_or(0, |k| k.sym(code as u32));
        let Some(m) = self.ime.as_mut() else { return };
        let id = m.next_id;
        m.next_id += 1;
        m.pending.push_back((id, win, code, repeat, release));
        let _ = m.tx.send(ime_dbus::Cmd::Key { id, keyval, keycode: code as u32, state: state as u32, release, time });
    }

    /// IME から届いた知らせ（キーの答え・確定・変換中の文字・送り返したキー）を、IME の焦点の窓の入力にする
    fn take_ime(&mut self) {
        while let Some(msg) = self.ime.as_ref().and_then(|m| m.rx.try_recv().ok()) {
            match msg {
                ime_dbus::Msg::Ready(engine) => {
                    if let Some(m) = self.ime.as_mut() {
                        m.ready = true;
                    }
                    log::event("ime.ready", engine.name(), "ok");
                    for w in &mut self.windows {
                        w.need_frame = true;
                    }
                }
                ime_dbus::Msg::Failed(e) => {
                    log::event("ime.unavailable", "fcitx5 / ibus", &format!("error={e} (keys go straight to the app)"));
                    let pending: Vec<_> = self.ime.take().map(|mut m| m.pending.drain(..).collect()).unwrap_or_default();
                    for (_, win, code, repeat, release) in pending {
                        if !release {
                            self.key_down(win, code as u32, repeat);
                        }
                    }
                    return;
                }
                ime_dbus::Msg::KeyDone { id, handled } => {
                    let Some(m) = self.ime.as_mut() else { return };
                    let mut earlier = Vec::new();
                    let mut hit = None;
                    while let Some(p) = m.pending.pop_front() {
                        if p.0 == id {
                            hit = Some(p);
                            break;
                        }
                        earlier.push(p);
                    }
                    // 答えの来なかった前のキー（無いはず）は、失わないようにアプリに渡す
                    for (_, win, code, repeat, release) in earlier {
                        if !release {
                            self.key_down(win, code as u32, repeat);
                        }
                    }
                    if let Some((_, win, code, repeat, release)) = hit {
                        if handled {
                            if !release && !repeat {
                                log::event("ime.key", &format!("code={code}"), "used by the IME");
                            }
                        } else if !release {
                            self.key_down(win, code as u32, repeat);
                        }
                    }
                }
                ime_dbus::Msg::Commit(text) => {
                    let Some(m) = self.ime.as_mut() else { return };
                    m.shown.clear();
                    let win = m.win;
                    log::event("ime.commit", &format!("text={text}"), &format!("win={win:?}"));
                    if let Some(w) = win.and_then(|id| self.win(id)) {
                        w.push(Event::ImePreedit(String::new()));
                        w.push(Event::ImeCommit(text));
                    }
                }
                ime_dbus::Msg::Preedit(text, clauses) => {
                    let Some(m) = self.ime.as_mut() else { return };
                    let shown = format!(" clauses={:?} target={:?} cursor={:?}", clauses.bounds, clauses.target, clauses.cursor);
                    let events = ime::preedit_events(&m.shown, text.clone(), clauses);
                    log::event("ime.preedit", &format!("text={text:?}"), &format!("events={}{}", events.len(), if text.is_empty() { "" } else { &shown }));
                    m.shown = text;
                    let win = m.win;
                    if let Some(w) = win.and_then(|id| self.win(id)) {
                        for e in events {
                            w.push(e);
                        }
                    }
                }
                ime_dbus::Msg::Forward { keyval, state, release } => {
                    if release {
                        continue;
                    }
                    let win = self.ime.as_ref().and_then(|m| m.win);
                    let mods = Mods { shift: state & 1 != 0, ctrl: state & 4 != 0, alt: state & 8 != 0 };
                    let key = key_for(keyval, keyval);
                    let text = if mods.ctrl || mods.alt { String::new() } else { self.xkb.as_ref().map_or(String::new(), |k| k.keysym_text(keyval)) };
                    log::event("ime.forward", &format!("keyval=0x{keyval:x} state={state}"), &format!("key={key:?} text={text:?}"));
                    if let Some(w) = win.and_then(|id| self.win(id)) {
                        if let Some(k) = key {
                            w.push(Event::Key { key: k, mods });
                        }
                        for c in text.chars().filter(|c| !c.is_control()) {
                            w.push(Event::Char(c));
                        }
                    }
                }
            }
        }
    }

    /// 窓 i で、焦点のある部品が入力欄なら IME の焦点を置き、そうでなければ外す。文字を打つ位置が動いたら知らせる。
    /// kargui が変換中の文字を確定した時（入力欄の外を押したなど）は、IME の変換を捨てさせる（Reset）
    fn update_ime(&mut self, i: usize) {
        let (id, focused) = (self.windows[i].id, self.windows[i].focused);
        let keyboard = focused && self.keyboard_win == Some(id);
        let cancel = std::mem::take(&mut self.windows[i].rt.cx.ime_cancel);
        let Some(m) = self.ime.as_ref().filter(|m| m.ready) else { return };
        let cx = &self.windows[i].rt.cx;
        // パスワードの欄では IME に焦点を置かない（Context::ime_target）
        let focus = cx.ime_target().filter(|_| keyboard);
        // ほかの窓が IME の焦点を持っていて、この窓の入力欄に焦点が無ければ触らない
        if m.win.is_some_and(|w| w != id) && focus.is_none() {
            return;
        }
        let caret = cx.caret.map(|c| (id, c.x.floor() as i32, c.y.floor() as i32, c.w.ceil().max(1.0) as i32, c.h.ceil().max(1.0) as i32));
        // ルート窓の上の位置は、キャレットか窓が動いた時だけ測り直す（X サーバーとの往復が 1 回要る）
        let rect = match caret {
            Some(c) if focus.is_some() && (m.moved || m.caret != Some(c)) => {
                self.conn.translate_coordinates(id, self.root, 0, 0).ok().and_then(|r| r.reply().ok()).map(|t| (t.dst_x as i32 + c.1, t.dst_y as i32 + c.2, c.3, c.4))
            }
            _ => m.rect,
        };
        let Some(m) = self.ime.as_mut() else { return };
        let mut cmds = Vec::new();
        if cancel && !m.shown.is_empty() {
            cmds.push(ime_dbus::Cmd::Reset);
            m.shown.clear();
        }
        let was_on = m.enabled.is_some() && m.win == Some(id);
        let now_on = focus.is_some();
        if now_on && !was_on {
            cmds.push(ime_dbus::Cmd::FocusIn);
            m.win = Some(id);
        } else if !now_on && was_on {
            cmds.push(ime_dbus::Cmd::FocusOut);
        }
        let changed = m.enabled != focus;
        m.enabled = focus;
        if now_on {
            m.caret = caret;
            m.moved = false;
            if let Some(r) = rect
                && (Some(r) != m.rect || !was_on)
            {
                cmds.push(ime_dbus::Cmd::Rect(r.0, r.1, r.2, r.3));
                m.rect = Some(r);
            }
        } else {
            m.rect = None;
            m.caret = None;
        }
        if changed || cancel {
            let fmt = |f: Option<Id>| f.map_or("none".to_string(), |f| format!("{:x}", f.0));
            log::event("ime.focus", &format!("why={} to={} rect={:?}{}", if cancel { "cancel" } else { "focus" }, fmt(focus), m.rect, self.windows[i].tag()), &format!("cmds={}", cmds.len()));
        } else if cmds.iter().any(|c| matches!(c, ime_dbus::Cmd::Rect(..))) {
            log::event("ime.rect", &format!("{:?}", m.rect), "sent");
        }
        let Some(m) = self.ime.as_ref() else { return };
        for c in cmds {
            let _ = m.tx.send(c);
        }
    }

    /// 引っ張り元に XDND の知らせを送る（XdndStatus・XdndFinished）
    fn xdnd_send(&self, source: Window, kind: xproto::Atom, data: [u32; 5]) {
        let ev = xproto::ClientMessageEvent::new(32, source, kind, data);
        let _ = self.conn.send_event(false, source, EventMask::NO_EVENT, ev);
        let _ = self.conn.flush();
    }

    /// XDND の知らせ（入った・動いた・出た・落とした）。ファイルなら Wayland・Windows と同じ FileDrag* にする
    fn xdnd(&mut self, e: xproto::ClientMessageEvent) {
        let a = self.atoms;
        let d = e.data.as_data32();
        let win = e.window;
        if e.type_ == a.XdndEnter {
            let (source, version) = (d[0], d[1] >> 24);
            // 種類が 4 つ以上なら、引っ張り元の XdndTypeList に全部ある
            let types: Vec<u32> = if d[1] & 1 != 0 {
                self.conn.get_property(false, source, a.XdndTypeList, AtomEnum::ATOM, 0, 1024).ok().and_then(|c| c.reply().ok()).and_then(|r| r.value32().map(|v| v.collect())).unwrap_or_default()
            } else {
                d[2..5].iter().copied().filter(|&t| t != x11rb::NONE).collect()
            };
            let files = types.contains(&a.TEXT_URI_LIST);
            log::event("drag.enter", &format!("source=0x{source:x} version={version} types={}", types.len()), &format!("files={files}"));
            if let Some(old) = self.dnd.take()
                && old.files
                && let Some(w) = self.win(old.win)
            {
                w.push(Event::FileDragLeave);
            }
            self.dnd = Some(XDrag { win, source, files, received: false, origin: None, at: (0.0, 0.0), accepting: false });
        } else if e.type_ == a.XdndPosition {
            let Some(mut g) = self.dnd.take().filter(|g| g.source == d[0]) else { return };
            let (rx, ry, time) = ((d[2] >> 16) as i16 as i32, (d[2] & 0xffff) as i16 as i32, d[3]);
            if g.origin.is_none() {
                g.origin = self.conn.translate_coordinates(win, self.root, 0, 0).ok().and_then(|c| c.reply().ok()).map(|t| (t.dst_x as i32, t.dst_y as i32));
            }
            let (ox, oy) = g.origin.unwrap_or((0, 0));
            g.at = ((rx - ox) as f32, (ry - oy) as f32);
            if g.files && !g.received {
                // 場所の一覧を受け取る（クリップボードと同じ受け渡しの窓と待ち方）
                g.received = true;
                let t = Instant::now();
                let bytes = {
                    let mut c = self.clip.borrow_mut();
                    let (owner, prop) = (c.owner, a.KARGUI_CLIP);
                    let asked = c.conn.convert_selection(owner, a.XdndSelection, a.TEXT_URI_LIST, prop, time).is_ok();
                    let _ = c.conn.flush();
                    if asked { c.wait_selection().flatten() } else { None }
                };
                let paths: Vec<PathBuf> = bytes
                    .map(|b| crate::file_dialog::uri_list(&String::from_utf8_lossy(&b)).into_iter().map(|p| PathBuf::from(std::ffi::OsString::from_vec(p))).collect())
                    .unwrap_or_default();
                log::event("drag.files", &format!("at={:.0},{:.0}", g.at.0, g.at.1), &format!("files={} ms={:.1}", paths.len(), t.elapsed().as_secs_f64() * 1000.0));
                g.files = !paths.is_empty();
                if g.files
                    && let Some(w) = self.win(win)
                {
                    w.push(Event::FileDragEnter { x: g.at.0, y: g.at.1, paths });
                }
            } else if g.files
                && let Some(w) = self.win(win)
            {
                w.push(Event::FileDragOver { x: g.at.0, y: g.at.1 });
            }
            // 受け取れるかは、前のフレームの配置でその場で答える（Wayland・Windows と同じ）
            let ok = g.files && self.windows.iter().find(|w| w.id == win).is_some_and(|w| w.rt.cx.accepts_files_at(g.at.0, g.at.1));
            if ok != g.accepting {
                log::event("drag.accept", &format!("at={:.0},{:.0}", g.at.0, g.at.1), &format!("accept={ok}"));
            }
            g.accepting = ok;
            let source = g.source;
            self.dnd = Some(g);
            // 位置の知らせを続けて送ってもらう（2）。受け取れる時は写す動き
            self.xdnd_send(source, a.XdndStatus, [win, ok as u32 | 2, 0, 0, if ok { a.XdndActionCopy } else { x11rb::NONE }]);
        } else if e.type_ == a.XdndLeave {
            if let Some(g) = self.dnd.take() {
                if g.files
                    && let Some(w) = self.win(g.win)
                {
                    w.push(Event::FileDragLeave);
                }
                log::event("drag.leave", "", "ok");
            }
        } else if e.type_ == a.XdndDrop {
            let Some(g) = self.dnd.take().filter(|g| g.source == d[0]) else { return };
            let ok = g.files && g.accepting;
            let (x, y) = g.at;
            if let Some(w) = self.win(g.win) {
                if ok {
                    w.push(Event::FileDrop { x, y });
                } else if g.files {
                    w.push(Event::FileDragLeave);
                }
            }
            self.xdnd_send(g.source, a.XdndFinished, [win, ok as u32, if ok { a.XdndActionCopy } else { x11rb::NONE }, 0, 0]);
            log::event("drag.drop", &format!("at={x:.0},{y:.0}"), if ok { "dropped" } else { "not accepted" });
        }
    }

    /// ほかのアプリがこちらのクリップボードを読みに来た（SelectionRequest）
    fn selection_request(&mut self, e: xproto::SelectionRequestEvent) {
        let a = self.atoms;
        let ours = self.clip.borrow().ours.clone();
        // 古いアプリは property を NONE で送る。その時は target に書く
        let prop = if e.property == x11rb::NONE { e.target } else { e.property };
        let text_targets = [a.UTF8_STRING, a.TEXT_PLAIN_UTF8, AtomEnum::STRING.into(), a.TEXT];
        let answered = match &ours {
            Some(text) if e.selection == a.CLIPBOARD => {
                if e.target == a.TARGETS {
                    let list = [a.TARGETS, a.UTF8_STRING, a.TEXT_PLAIN_UTF8, AtomEnum::STRING.into(), a.TEXT];
                    self.conn.change_property32(PropMode::REPLACE, e.requestor, prop, AtomEnum::ATOM, &list).is_ok()
                } else if text_targets.contains(&e.target) {
                    let kind = if e.target == a.TEXT { a.UTF8_STRING } else { e.target };
                    // 1 回の頼みに入る大きさを超える物（INCR が要る）は、まだ渡さない
                    let max = self.conn.maximum_request_bytes().saturating_sub(64);
                    text.len() <= max && self.conn.change_property8(PropMode::REPLACE, e.requestor, prop, kind, text.as_bytes()).is_ok()
                } else {
                    false
                }
            }
            _ => false,
        };
        let reply = xproto::SelectionNotifyEvent {
            response_type: xproto::SELECTION_NOTIFY_EVENT,
            sequence: 0,
            time: e.time,
            requestor: e.requestor,
            selection: e.selection,
            target: e.target,
            property: if answered { prop } else { x11rb::NONE },
        };
        let _ = self.conn.send_event(false, e.requestor, EventMask::NO_EVENT, reply);
        let _ = self.conn.flush();
        log::event("clipboard.request", &format!("target={} requestor=0x{:x}", e.target, e.requestor), if answered { "sent" } else { "refused" });
    }

    /// X の知らせを 1 つ処理する
    fn handle(&mut self, ev: XEvent) {
        let focus_in = matches!(ev, XEvent::FocusIn(_));
        match ev {
            XEvent::Expose(e) if e.count == 0 => {
                if let Some(w) = self.win(e.window) {
                    w.exposed = true;
                    w.need_frame = true;
                    w.why = "expose";
                }
            }
            XEvent::MapNotify(e) => {
                if let Some(w) = self.win(e.window) {
                    w.mapped = true;
                    w.exposed = true;
                    w.need_frame = true;
                    log::event("window.mapped", &w.tag(), "ok");
                }
            }
            XEvent::ConfigureNotify(e) => {
                if let Some(m) = self.ime.as_mut() {
                    m.moved = true;
                }
                let scale = self.scale;
                if let Some(w) = self.win(e.window) {
                    let size = (e.width as i32, e.height as i32);
                    if size != w.size {
                        w.size = size;
                        w.rt.resize(size.0 as f32, size.1 as f32, scale);
                        w.summary = Summary::default();
                        w.full = true;
                        w.need_frame = true;
                        w.why = "size";
                        log::event("window.size", &format!("physical={}x{} scale={scale:.3}{}", size.0, size.1, w.tag()), "configure");
                    }
                }
                // 画面の上の場所（中の左上を画面の座標で読み、論理の画素にする。外枠の左上も同じ所とする）。ほかの窓は
                // 場所が変わったら描き直す（ドッキングが窓の場所を並べ方に写す）
                if self.win(e.window).is_some() {
                    let place = self.conn.translate_coordinates(e.window, self.root, 0, 0).ok().and_then(|c| c.reply().ok()).map(|t| {
                        let (x, y) = (t.dst_x as f32 / scale, t.dst_y as f32 / scale);
                        crate::ui::context::WindowPlace { outer: (x, y), client: (x, y), unit: 1.0 / scale }
                    });
                    if let Some(w) = self.win(e.window)
                        && place.is_some()
                        && place != w.rt.cx.place
                    {
                        w.rt.cx.place = place;
                        if w.key.is_some() {
                            w.need_frame = true;
                            w.why = "place";
                        }
                    }
                }
            }
            XEvent::ClientMessage(e) if [self.atoms.XdndEnter, self.atoms.XdndPosition, self.atoms.XdndLeave, self.atoms.XdndDrop].contains(&e.type_) => self.xdnd(e),
            XEvent::ClientMessage(e) if e.type_ == self.atoms.WM_PROTOCOLS => {
                let data = e.data.as_data32();
                if data[0] == self.atoms.WM_DELETE_WINDOW {
                    self.close_requested(e.window);
                } else if data[0] == self.atoms._NET_WM_PING {
                    // 動いていることをウィンドウマネージャーに返す
                    let mut reply = e;
                    reply.window = self.root;
                    let _ = self.conn.send_event(false, self.root, EventMask::SUBSTRUCTURE_NOTIFY | EventMask::SUBSTRUCTURE_REDIRECT, reply);
                }
            }
            XEvent::FocusIn(e) | XEvent::FocusOut(e) if e.detail != xproto::NotifyDetail::POINTER => {
                let focused = focus_in;
                if focused {
                    self.keyboard_win = Some(e.event);
                } else {
                    self.pressed.clear();
                    if self.keyboard_win == Some(e.event) {
                        self.keyboard_win = None;
                    }
                }
                if let Some(w) = self.win(e.event)
                    && w.focused != focused
                {
                    w.focused = focused;
                    w.a11y.update_window_focus_state(focused);
                    w.need_frame = true;
                    log::event("window.focus", &format!("focused={focused}{}", w.tag()), "ok");
                }
            }
            XEvent::EnterNotify(e) => {
                let s = (e.event_x as f32, e.event_y as f32);
                self.pointer_win = Some(e.event);
                self.cursor_reset(e.event);
                self.pointer_motion(e.event, s.0, s.1);
                self.update_shape();
            }
            XEvent::LeaveNotify(e) => {
                if let Some(w) = self.win(e.event) {
                    let had = w.rt.cx.hover.is_some();
                    let bar = w.rt.cx.bar_hover.take().is_some();
                    w.rt.cx.set_hover(None);
                    if had || bar {
                        w.need_frame = true;
                        w.why = "input";
                    }
                    log::event("pointer.leave", &w.tag(), &format!("had_hover={had}"));
                }
                if self.pointer_win == Some(e.event) && !self.left_down {
                    self.pointer_win = None;
                }
            }
            XEvent::MotionNotify(e) => {
                self.clip.borrow_mut().time = e.time;
                self.pointer_motion(e.event, e.event_x as f32, e.event_y as f32);
            }
            XEvent::ButtonPress(e) => self.button(e.event, e.detail, true, e.time, e.event_x as f32, e.event_y as f32),
            XEvent::ButtonRelease(e) => self.button(e.event, e.detail, false, e.time, e.event_x as f32, e.event_y as f32),
            XEvent::KeyPress(e) => {
                self.clip.borrow_mut().time = e.time;
                let repeat = self.pressed.contains(&e.detail);
                if !repeat {
                    self.pressed.push(e.detail);
                }
                if self.ime_takes(e.event) {
                    self.send_key_to_ime(e.event, e.detail, u16::from(e.state), e.time, repeat, false);
                } else {
                    self.key_down(e.event, e.detail as u32, repeat);
                }
            }
            XEvent::KeyRelease(e) => {
                self.pressed.retain(|&k| k != e.detail);
                // IME は離したキーも見る（Shift だけを押して離すと、かなと英字を切り替える IME がある）
                if self.ime_takes(e.event) {
                    self.send_key_to_ime(e.event, e.detail, u16::from(e.state), e.time, false, true);
                }
            }
            XEvent::XkbStateNotify(e) => {
                if let Some(k) = self.xkb.as_mut() {
                    let m = |v: xproto::ModMask| u16::from(v) as u32;
                    k.update_mask((m(e.base_mods), m(e.latched_mods), m(e.locked_mods)), (e.base_group as i32 as u32, e.latched_group as i32 as u32, u8::from(e.locked_group) as u32));
                }
            }
            XEvent::XkbNewKeyboardNotify(_) | XEvent::XkbMapNotify(_) => {
                let raw = self.conn.get_raw_xcb_connection();
                let ok = self.xkb.as_mut().is_some_and(|k| k.set_x11_keymap(raw));
                log::event("keyboard.keymap", "reload", if ok { "ok" } else { "error" });
            }
            XEvent::ShmCompletion(e) => {
                if let Some(w) = self.win(e.drawable) {
                    w.shm_busy = false;
                    if w.need_frame {
                        w.why = "shm";
                    }
                }
            }
            XEvent::SelectionRequest(e) => self.selection_request(e),
            XEvent::SelectionClear(e) => {
                if e.selection == self.atoms.CLIPBOARD {
                    self.clip.borrow_mut().ours = None;
                    log::event("clipboard.lost", &format!("owner=0x{:x}", e.owner), "ok");
                }
            }
            XEvent::Error(e) => log::event("x11.error", &format!("{e:?}"), "ignored"),
            _ => {}
        }
    }

    /// 窓に入った時は、前の形を忘れる（ほかの窓の形のままにしない）
    fn cursor_reset(&mut self, id: WinId) {
        if let Some(w) = self.win(id) {
            w.cursor = "";
        }
    }

    fn button(&mut self, id: WinId, button: u8, pressed: bool, time: u32, x: f32, y: f32) {
        self.clip.borrow_mut().time = time;
        self.px = x;
        self.py = y;
        self.pointer_win = Some(id);
        let shift = self.mods().shift;
        let last_press = self.last_press;
        let scale = self.scale;
        let Some(w) = self.win(id) else { return };
        if button != 1 {
            log::event("button", &format!("button={button} pressed={pressed} x={x} y={y}{}", w.tag()), "ok");
        }
        match (button, pressed) {
            (1, true) => {
                let target = w.rt.cx.hit(x, y).map(|h| h.id);
                let d = DOUBLE_CLICK_DIST * scale;
                let count = match last_press {
                    Some((t0, x0, y0, n)) if time.wrapping_sub(t0) <= DOUBLE_CLICK_MS && (x - x0).abs() <= d && (y - y0).abs() <= d => (n % 3) + 1,
                    _ => 1,
                };
                log::event("click", &format!("x={x} y={y} count={count}{}", w.tag()), &format!("target={target:?}"));
                w.push(Event::PointerDown { x, y, shift, count });
                self.left_down = true;
                self.last_press = Some((time, x, y, count));
            }
            (1, false) => {
                w.push(Event::PointerUp { x, y });
                self.left_down = false;
            }
            // 右クリックは離した時（Windows の WM_CONTEXTMENU・Wayland と同じ）
            (3, false) => {
                log::event("context_menu", &format!("x={x} y={y}"), "sent");
                w.push(Event::ContextMenu { x, y });
            }
            // ホイールは 1 刻みが 1 回の押し（4 が上、5 が下）
            (4, true) | (5, true) => {
                let dy = if button == 4 { -WHEEL_STEP } else { WHEEL_STEP };
                log::event("wheel", &format!("button={button} x={x} y={y}"), &format!("dy={dy:.1}"));
                w.push(Event::Wheel { x, y, dy });
            }
            _ => {}
        }
    }

    /// 窓 i の 1 フレーム: 入力を渡してビューを回し、描いて画面に出す
    fn frame(&mut self, i: usize) {
        let t = Instant::now();
        let app = &mut self.app;
        let w = &mut self.windows[i];
        let why = w.why;
        let events = std::mem::take(&mut w.events);
        let out = match w.key.clone() {
            None => w.rt.frame(&mut **app, events),
            Some(k) => w.rt.frame_with(&mut |ui: &mut crate::ui::Ui| app.window(&k, ui), events),
        };
        let (id, is_main) = (w.id, w.key.is_none());
        let quit = std::mem::take(&mut w.rt.cx.quit);
        let dialog = w.rt.cx.file_dialog_request.take();
        let requested = is_main.then(|| w.rt.cx.windows.clone());
        let tray = is_main.then(|| w.rt.cx.tray.clone());
        let show = w.rt.cx.show_request.take();
        w.wake_at = out.wake_after.map(|d| Instant::now() + d);
        if out.want_frame {
            w.wake_at = Some(w.wake_at.map_or(Instant::now() + ANIMATION_STEP, |t| t.min(Instant::now() + ANIMATION_STEP)));
        }
        w.animating = out.want_frame;
        w.need_frame = false;
        if quit {
            log::event("window.quit", "", "ok");
            self.running = false;
        }
        if let Some((key, d)) = dialog {
            self.open_file_dialog(id, key, d);
        }
        if let Some(t) = tray {
            self.sync_tray(t);
        }
        if let Some(s) = show {
            self.present(id, s);
        }
        let w = &mut self.windows[i];
        let rt = &mut w.rt;
        w.a11y.update_if_active(|| rt.a11y().0);
        let t_build = t.elapsed().as_secs_f64() * 1000.0;
        let drawn = self.draw(i, why, t_build);
        if drawn {
            let w = &mut self.windows[i];
            let first = !w.shown;
            w.shown = true;
            if first {
                log::event("window.shown", &format!("physical={}x{} scale={:.3}{}", w.size.0, w.size.1, self.scale, w.tag()), &format!("ms={:.1}", w.created.elapsed().as_secs_f64() * 1000.0));
            }
            if is_main {
                let number = if first { 0 } else { w.rt.frames - 1 };
                self.app.presented(&Frame { number, since_start: self.started.elapsed() });
            }
        }
        self.update_ime(i);
        if why == "input" {
            for o in self.windows.iter_mut().filter(|o| o.id != id && !o.need_frame) {
                o.need_frame = true;
                o.why = "sync";
            }
        }
        if let Some(requested) = requested {
            self.sync_windows(requested);
        }
    }

    /// 窓 i の絵を描いて出す。出せたら true
    fn draw(&mut self, i: usize, why: &str, t_build: f64) -> bool {
        let t_draw = Instant::now();
        let conn = self.conn.clone();
        let (gc, depth, shm_ok) = (self.gc, self.depth, self.shm_ok);
        let w = &mut self.windows[i];
        let (pw, ph) = w.size;
        if w.shm_busy {
            // サーバーがまだ前の絵を読んでいる。ShmCompletion が来たら描く
            w.need_frame = true;
            log::event("render", &format!("why={why}{}", w.tag()), "busy (the server is reading the last picture)");
            return false;
        }
        if w.pixels.len() != (pw * ph) as usize {
            w.pixels = vec![0; (pw * ph) as usize];
            w.summary = Summary::default();
            w.full = true;
            if let Some(s) = w.shm.take() {
                s.free(&conn);
            }
        }
        if shm_ok && w.shm.is_none() {
            match ShmBuf::new(&conn, pw, ph) {
                Ok(s) => w.shm = Some(s),
                Err(e) => log::event("shm.init", &format!("size={pw}x{ph}{}", w.tag()), &format!("error={e} (PutImage)")),
            }
        }
        let bg = w.rt.cx.theme.background;
        let full = w.full || bg != w.background;
        let drawn = cpu_frame::draw(&mut w.pixels, pw, ph, &w.rt.draw, &w.rt.cx.text.atlas, bg, &w.summary, full);
        w.summary = drawn.summary;
        w.background = bg;
        w.full = false;
        let mut rects: Vec<IRect> = drawn.present;
        if std::mem::take(&mut w.exposed) {
            rects = vec![IRect { x: 0, y: 0, w: pw, h: ph }];
        }
        let rects: Vec<IRect> = rects.into_iter().filter_map(|r| clip_rect(r, pw, ph)).collect();
        let mode = if w.shm.is_some() { "shm" } else { "put_image" };
        if let Some(s) = w.shm.as_mut().filter(|s| s.width == pw && s.height == ph) {
            let rows = s.rows();
            for r in &rects {
                for y in r.y..r.y + r.h {
                    let a = (y * pw + r.x) as usize;
                    let b = a + r.w as usize;
                    rows[a..b].copy_from_slice(&w.pixels[a..b]);
                }
            }
            for (k, r) in rects.iter().enumerate() {
                let last = k + 1 == rects.len();
                let _ = conn.shm_put_image(w.id, gc, pw as u16, ph as u16, r.x as u16, r.y as u16, r.w as u16, r.h as u16, r.x as i16, r.y as i16, depth, xproto::ImageFormat::Z_PIXMAP.into(), last, s.seg, 0);
            }
            w.shm_busy = !rects.is_empty();
        } else {
            // 1 回の頼みに入る行ずつ送る
            let max = conn.maximum_request_bytes().saturating_sub(64).max(4096);
            for r in &rects {
                let per_row = (r.w * 4) as usize;
                let rows_per = (max / per_row).max(1) as i32;
                let mut y = r.y;
                while y < r.y + r.h {
                    let n = rows_per.min(r.y + r.h - y);
                    let mut data = Vec::with_capacity(per_row * n as usize);
                    for yy in y..y + n {
                        let a = (yy * pw + r.x) as usize;
                        for p in &w.pixels[a..a + r.w as usize] {
                            data.extend_from_slice(&p.to_le_bytes());
                        }
                    }
                    let _ = conn.put_image(xproto::ImageFormat::Z_PIXMAP, w.id, gc, r.w as u16, n as u16, r.x as i16, y as i16, 0, depth, &data);
                    y += n;
                }
            }
        }
        let _ = conn.flush();
        let t_draw = t_draw.elapsed().as_secs_f64() * 1000.0;
        if !w.animating {
            log::event(
                "render",
                &format!("why={why} {} rects={} quads={} mode={mode}{}", drawn.mode, rects.len(), w.rt.draw.quads.len(), w.tag()),
                &format!("ms={:.2} draw_ms={t_draw:.2}", t_build + t_draw),
            );
        }
        true
    }

    /// 次の知らせ（貼り付けの間に取っておいた物が先）
    fn next_event(&self) -> Option<XEvent> {
        if let Some(e) = self.clip.borrow_mut().stash.pop_front() {
            return Some(e);
        }
        match self.conn.poll_for_event() {
            Ok(e) => e,
            Err(e) => {
                log::event("x11.connection", "", &format!("error={e}"));
                None
            }
        }
    }
}

/// 窓の外にはみ出た所を切る（空なら None）
fn clip_rect(r: IRect, w: i32, h: i32) -> Option<IRect> {
    let (x0, y0) = (r.x.max(0), r.y.max(0));
    let (x1, y1) = ((r.x + r.w).min(w), (r.y + r.h).min(h));
    (x1 > x0 && y1 > y0).then_some(IRect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 })
}

/// Xft.dpi（X のリソース）から倍率を決める。無い・読めない時は 1
fn scale_from_dpi(dpi: Option<f32>) -> f32 {
    match dpi {
        Some(d) if d.is_finite() && d > 0.0 => (d / 96.0).clamp(1.0, 4.0),
        _ => 1.0,
    }
}

pub(crate) fn run(options: Options, app: Box<dyn App>) -> Result<(), crate::Error> {
    let started = Instant::now();
    let wake_fd = Arc::new(eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK).map_err(|e| error("eventfd", e))?);
    let (conn, screen) = XCBConnection::connect(None).map_err(|e| error("connect (DISPLAY, libxcb.so.1)", e))?;
    let conn = Rc::new(conn);
    let setup = conn.setup();
    let scr = &setup.roots[screen];
    let (root, depth, visual) = (scr.root, scr.root_depth, scr.root_visual);
    let bpp = setup.pixmap_formats.iter().find(|f| f.depth == depth).map_or(0, |f| f.bits_per_pixel);
    let atoms = Atoms::new(&*conn).map_err(|e| error("intern_atom", e))?.reply().map_err(|e| error("intern_atom", e))?;
    // 倍率（Xft.dpi）
    let db = x11rb::resource_manager::new_from_default(&*conn).ok();
    let dpi = db.as_ref().and_then(|d| d.get_value::<f32>("Xft.dpi", "").ok().flatten());
    let scale = scale_from_dpi(dpi);
    // MIT-SHM（memfd を渡す形。1.2 から）
    let shm_ok = conn.extension_information(shm::X11_EXTENSION_NAME).ok().flatten().is_some()
        && conn.shm_query_version().ok().and_then(|c| c.reply().ok()).is_some_and(|v| (v.major_version, v.minor_version) >= (1, 2));
    // キー配列（XKB と libxkbcommon-x11）
    let xkb_ok = conn.xkb_use_extension(1, 0).ok().and_then(|c| c.reply().ok()).is_some_and(|r| r.supported);
    let mut kb = Keyboard::new();
    if xkb_ok {
        if let Some(x) = xkbcommon_dl::x11::xkbcommon_x11_option() {
            let (mut major, mut minor, mut ev, mut er) = (0u16, 0u16, 0u8, 0u8);
            // SAFETY: 生きている libxcb の接続と、書いてもらう 4 つの値を渡す
            unsafe {
                (x.xkb_x11_setup_xkb_extension)(
                    conn.get_raw_xcb_connection() as *mut _,
                    1,
                    0,
                    xkbcommon_dl::x11::xkb_x11_setup_xkb_extension_flags::XKB_X11_SETUP_XKB_EXTENSION_NO_FLAGS,
                    &mut major,
                    &mut minor,
                    &mut ev,
                    &mut er,
                );
            }
        }
        let ok = kb.as_mut().is_some_and(|k| k.set_x11_keymap(conn.get_raw_xcb_connection()));
        log::event("keyboard.keymap", "x11", if ok { "ok" } else { "error=libxkbcommon-x11.so could not be loaded or the keymap could not be read" });
        let parts = xkb::MapPart::KEY_TYPES
            | xkb::MapPart::KEY_SYMS
            | xkb::MapPart::MODIFIER_MAP
            | xkb::MapPart::EXPLICIT_COMPONENTS
            | xkb::MapPart::KEY_ACTIONS
            | xkb::MapPart::KEY_BEHAVIORS
            | xkb::MapPart::VIRTUAL_MODS
            | xkb::MapPart::VIRTUAL_MOD_MAP;
        let events = xkb::EventType::NEW_KEYBOARD_NOTIFY | xkb::EventType::MAP_NOTIFY | xkb::EventType::STATE_NOTIFY;
        let core = xkb::ID::USE_CORE_KBD.into();
        let _ = conn.xkb_select_events(core, 0u16.into(), events, parts, parts, &xkb::SelectEventsAux::new());
        // 押し続けで、離した知らせを挟まない（押した知らせだけを繰り返す）
        let detectable = conn.xkb_per_client_flags(core, xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT, xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT, 0u32.into(), 0u32.into(), 0u32.into()).ok().and_then(|c| c.reply().ok());
        log::event("keyboard.repeat", "", &format!("detectable={}", detectable.is_some_and(|r| u32::from(r.value) & u32::from(xkb::PerClientFlag::DETECTABLE_AUTO_REPEAT) != 0)));
    } else {
        log::event("keyboard.xkb", "", "error=the XKB extension is missing (keys will not work)");
    }
    let gc = conn.generate_id().map_err(|e| error("generate_id", e))?;
    conn.create_gc(gc, root, &xproto::CreateGCAux::new().graphics_exposures(0)).map_err(|e| error("create_gc", e))?;
    // クリップボードの受け渡しに使う見えない窓
    let owner = conn.generate_id().map_err(|e| error("generate_id", e))?;
    conn.create_window(0, owner, root, -10, -10, 1, 1, 0, xproto::WindowClass::INPUT_ONLY, x11rb::COPY_FROM_PARENT, &xproto::CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE))
        .map_err(|e| error("create_window (clipboard)", e))?;
    let cursor_handle = db.as_ref().and_then(|d| x11rb::cursor::Handle::new(&*conn, screen, d).ok()).and_then(|c| c.reply().ok());
    log::event(
        "x11.setup",
        &format!(
            "screen={screen} depth={depth} bpp={bpp} dpi={dpi:?} scale={scale:.3} shm={shm_ok} xkb={xkb_ok} cursor_theme={} max_request={}",
            cursor_handle.is_some(),
            conn.maximum_request_bytes()
        ),
        &format!("ms={:.1}", started.elapsed().as_secs_f64() * 1000.0),
    );
    if depth != 24 && depth != 32 || bpp != 32 {
        return Err(error("visual", format!("depth {depth} with {bpp} bits per pixel is not supported (24 or 32 bits with 32 bits per pixel)")));
    }
    let settings_rx = crate::linux::spawn_settings(options.dark.is_none(), false, wake_fd.clone());
    let clip = Rc::new(RefCell::new(Clip { conn: conn.clone(), atoms, owner, ours: None, time: x11rb::CURRENT_TIME, stash: VecDeque::new() }));
    let (dialog_tx, dialog_rx) = channel();
    let mut st = State {
        app,
        conn: conn.clone(),
        atoms,
        root,
        depth,
        visual,
        gc,
        scale,
        shm_ok,
        windows: Vec::new(),
        font_files: options.fonts.clone(),
        main_icon: options.icon.clone(),
        xkb: kb,
        ime: None,
        dnd: None,
        pressed: Vec::new(),
        clip,
        cursor_handle,
        cursors: HashMap::new(),
        wake: wake_fd.clone(),
        dialog_tx,
        dialog_rx,
        dialog_open: false,
        last_folder: None,
        settings_rx,
        tray: None,
        tray_fallback: options.icon.clone(),
        running: true,
        pointer_win: None,
        keyboard_win: None,
        px: 0.0,
        py: 0.0,
        left_down: false,
        last_press: None,
        started,
    };
    if st.xkb.is_some() {
        let display = format!("x11:{}", std::env::var("DISPLAY").unwrap_or_default());
        let (tx, rx) = ime_dbus::spawn(app_id(), display, wake_fd.clone());
        st.ime = Some(XIme { tx, rx, ready: false, win: None, enabled: None, rect: None, caret: None, moved: true, shown: String::new(), pending: VecDeque::new(), next_id: 1 });
    }
    let rt = st.runtime(&options.title, options.dark.unwrap_or(false), &options.font)?;
    let local = crate::linux::local_date().zip(crate::linux::local_time()).map_or("unknown".to_string(), |((y, m, d), (h, min))| format!("{y}-{m:02}-{d:02} {h:02}:{min:02}"));
    log::event("window.create", &format!("title={:?} size={:.0}x{:.0} class={} local={local}", options.title, options.size.0, options.size.1, app_id()), "ok");
    let icon = options.icon.clone();
    st.new_window(None, &options.title, options.size, options.min_size, None, icon.as_ref(), rt)?;
    // OS の明暗の答えを、最初の絵の前に 150 ms まで待つ
    let answer = st.settings_rx.as_ref().map(|rx| rx.recv_timeout(Duration::from_millis(150)));
    match answer {
        Some(Ok(s)) => {
            st.settings_rx = None;
            let when = format!("waited_ms={:.1}", started.elapsed().as_secs_f64() * 1000.0);
            st.apply_settings(s, &when);
        }
        Some(Err(_)) => log::event("portal.settings", "", "not answered in 150 ms (light until it is)"),
        None => {}
    }

    while st.running {
        // 届いている知らせを全部処理する
        while let Some(ev) = st.next_event() {
            st.handle(ev);
            if !st.running {
                break;
            }
        }
        if !st.running {
            break;
        }
        let now = Instant::now();
        for w in &mut st.windows {
            if w.wake_at.is_some_and(|t| t <= now) {
                w.wake_at = None;
                if !w.animating {
                    log::event("timer", &w.tag(), "frame");
                }
                w.need_frame = true;
                w.why = if w.animating { "animation" } else { "timer" };
            }
        }
        let ids: Vec<WinId> = st.windows.iter().map(|w| w.id).collect();
        for id in ids {
            if let Some(i) = st.index(id)
                && st.windows[i].need_frame
                && st.windows[i].mapped
                && !st.windows[i].shm_busy
            {
                st.frame(i);
            }
            if !st.running {
                break;
            }
        }
        if !st.running {
            break;
        }
        if let Err(e) = conn.flush() {
            log::event("x11.flush", "", &format!("error={e}"));
            return Err(error("flush", e));
        }
        // 描いている間に届いた知らせがあれば、待たずに処理する
        if !st.clip.borrow().stash.is_empty() {
            continue;
        }
        if let Ok(Some(ev)) = conn.poll_for_event() {
            st.handle(ev);
            continue;
        }
        let deadline = st.windows.iter().filter_map(|w| w.wake_at).min();
        let timeout = deadline.map(|d| d.saturating_duration_since(Instant::now()));
        let (woke, hup) = {
            let mut fds = [PollFd::new(&*conn, PollFlags::IN), PollFd::new(&*wake_fd, PollFlags::IN)];
            let ts = timeout.map(|d| Timespec { tv_sec: d.as_secs() as _, tv_nsec: d.subsec_nanos() as _ });
            match poll(&mut fds, ts.as_ref()) {
                Ok(_) | Err(rustix::io::Errno::INTR) => {}
                Err(e) => return Err(error("poll", e)),
            }
            (fds[1].revents().contains(PollFlags::IN), fds[0].revents().intersects(PollFlags::ERR | PollFlags::HUP))
        };
        if hup {
            log::event("x11.connection", "", "closed by the server");
            break;
        }
        if woke {
            let mut buf = [0u8; 8];
            let _ = rustix::io::read(&*wake_fd, &mut buf);
            log::event("wake", "from=waker", "frame");
            for w in &mut st.windows {
                w.rt.cx.waker.take_woken();
                w.need_frame = true;
                w.why = "wake";
                w.apply_a11y_actions();
            }
            st.take_answers();
            st.take_ime();
        }
    }
    log::event("loop.end", "", "ok");
    let ids: Vec<WinId> = st.windows.iter().map(|w| w.id).collect();
    for id in ids.into_iter().rev() {
        if let Some(i) = st.index(id) {
            let mut w = st.windows.remove(i);
            if let Some(s) = w.shm.take() {
                s.free(&conn);
            }
            let _ = conn.destroy_window(id);
        }
    }
    let _ = conn.flush();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dpi_sets_the_scale() {
        assert_eq!(scale_from_dpi(None), 1.0);
        assert_eq!(scale_from_dpi(Some(96.0)), 1.0);
        assert_eq!(scale_from_dpi(Some(144.0)), 1.5);
        assert_eq!(scale_from_dpi(Some(192.0)), 2.0);
        // 96 より小さい・おかしい値は 1（小さくはしない）
        assert_eq!(scale_from_dpi(Some(72.0)), 1.0);
        assert_eq!(scale_from_dpi(Some(f32::NAN)), 1.0);
    }

    #[test]
    fn present_rects_stay_inside_the_window() {
        assert_eq!(clip_rect(IRect { x: -5, y: 10, w: 20, h: 5 }, 100, 50), Some(IRect { x: 0, y: 10, w: 15, h: 5 }));
        assert_eq!(clip_rect(IRect { x: 90, y: 45, w: 20, h: 20 }, 100, 50), Some(IRect { x: 90, y: 45, w: 10, h: 5 }));
        assert_eq!(clip_rect(IRect { x: 100, y: 0, w: 5, h: 5 }, 100, 50), None);
    }

    #[test]
    fn window_icon_is_straight_argb() {
        // 乗算済みの半分の赤（128, 0, 0, 128）は、乗算していない赤（255, 0, 0）に戻す
        let img = crate::Image::premultiplied(2, 1, vec![128, 0, 0, 128, 0, 0, 0, 0]);
        assert_eq!(net_wm_icon(&img), vec![2, 1, 0x80ff_0000, 0]);
    }
}
