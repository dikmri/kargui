//! フレームをまたいで持つ状態と、入力の振り分け。
//!
//! クリックがどの部品に当たったかは、前のフレームの配置（ユーザーが見ていた画面）で決める。
//! 入力はビューの 1 回目（入力を渡す回）で部品に届き、2 回目（描く回）は入力なしで描く。

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::geom::Rect;
use crate::input::{A11yAction, Event, Key, Mods};
use crate::log;
use crate::text::Text;
use crate::theme::Theme;
use crate::ui::{Id, OverlayKind};
use crate::ime::{Clauses, ImeField};
use crate::ui::edit::EditState;
use crate::icon::Icon;
use crate::text::Slot;
use crate::wake::Waker;

/// クリップボードの読み書き（窓では OS のもの、試験台では手元の文字列）
pub(crate) trait Clipboard {
    fn get(&mut self) -> Option<String>;
    fn set(&mut self, text: &str);
}

/// 試験台で使う、手元の文字列だけのクリップボード
#[derive(Default)]
pub(crate) struct MemoryClipboard(pub Option<String>);

impl Clipboard for MemoryClipboard {
    fn get(&mut self) -> Option<String> {
        self.0.clone()
    }
    fn set(&mut self, text: &str) {
        self.0 = Some(text.to_string());
    }
}

/// 押したり焦点を受けたりできる部品の、前のフレームでの場所
#[derive(Clone, Debug)]
pub(crate) struct Hit {
    pub id: Id,
    pub rect: Rect,
    /// 当たりの余白を含めた範囲（小さい部品だけ rect より広い）
    pub slop: Rect,
    pub focusable: bool,
    pub text_input: bool,
    /// 焦点がある時に矢印などのキーを受ける（スライダー）
    pub arrows: bool,
    /// 上下の矢印と PageUp/PageDown も受ける（複数行の入力欄と数の入力欄）
    pub vertical: bool,
    /// 押せる。押せない部品も当たりには入れ、下の部品へクリックが抜けないようにする
    pub enabled: bool,
    /// ポインターの引っ張りを自分で使う（入力欄・スライダー・仕切り・描く面）。この上からはドラッグを始めない
    pub drags: bool,
    /// リンク（ポインターを手の形にする）
    pub link: bool,
}

/// 前のフレームで重ねて出した物（閉じる・焦点を戻す・Tab を閉じ込めるのに使う）
#[derive(Clone, Debug)]
pub(crate) struct OverlayInfo {
    pub id: Id,
    pub anchor: Option<Id>,
    pub kind: OverlayKind,
    pub rect: Rect,
    /// 中の焦点を持てる部品（Tab の順）
    pub focus: Vec<Id>,
}

/// アプリが描く題の帯の様子。合成が題の帯を描かない時（Wayland の GNOME など）に、窓の処理が入れる
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Csd {
    pub title: String,
    /// 窓が前面にある（題を濃く描く）
    pub focused: bool,
    /// 最大化している（最大化のボタンを「元に戻す」にする）
    pub maximized: bool,
    /// 最大化・全画面・敷き詰め（窓の縁が画面の端や隣の窓に付くので、縁の線を引かない）
    pub tiled: bool,
    /// 左と右に並べるボタン（OS の設定の順。GNOME の既定は右に閉じるだけ）
    pub left: Vec<CsdButton>,
    pub right: Vec<CsdButton>,
    /// Windows の形（Options::custom_title_bar）。ボタンは右端に付けた幅 46 の四角で、窓の縁と影は OS が描く
    pub windows: bool,
}

// GNOME の設定を読むのは Linux（Wayland）だけ
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
impl Csd {
    /// GNOME の button-layout（org.gnome.desktop.wm.preferences。「appmenu:minimize,maximize,close」の形）を
    /// 左と右のボタンに分ける。「:」の前が左、後が右。kargui に無い名前（appmenu・icon・spacer など）は飛ばす
    pub fn buttons(layout: &str) -> (Vec<CsdButton>, Vec<CsdButton>) {
        if layout.trim().is_empty() {
            return (vec![], vec![CsdButton::Close]);
        }
        let mut seen = Vec::new();
        let mut side = |part: &str| {
            let mut out = Vec::new();
            for name in part.split(',') {
                let b = match name.trim() {
                    "minimize" => CsdButton::Minimize,
                    "maximize" => CsdButton::Maximize,
                    "close" => CsdButton::Close,
                    _ => continue,
                };
                if !seen.contains(&b) {
                    seen.push(b);
                    out.push(b);
                }
            }
            out
        };
        let (left, right) = layout.split_once(':').unwrap_or((layout, ""));
        (side(left), side(right))
    }
}

/// 題の帯のボタン
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CsdButton {
    Minimize,
    Maximize,
    Close,
}

/// 題の帯の操作で、窓の処理に頼む事
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CsdRequest {
    /// 窓を動かす（押したまま動かした）
    Move,
    /// 最大化と元の大きさを切り替える（ボタンか、帯を 2 回押した）
    ToggleMaximize,
    Minimize,
    Close,
    /// 窓のメニュー（帯を右クリックした位置。物理ピクセル）
    Menu { x: f32, y: f32 },
}

/// 窓の画面の上の場所。どれも Options::position と同じ形（Windows は画面の画素、X11 は論理の画素）
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WindowPlace {
    /// 外枠の左上
    pub outer: (f32, f32),
    /// 中の左上
    pub client: (f32, f32),
    /// 物理ピクセル 1 つが、この形でいくつか（Windows は 1、X11 は 1 / 倍率）
    pub unit: f32,
}

/// 開くように頼まれた窓（Ui::window）
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WindowRequest {
    pub key: String,
    pub title: String,
    /// 中の大きさ（論理ピクセル）と、置く場所（無ければ OS に任せる）
    pub size: (f32, f32),
    pub position: Option<(f32, f32)>,
    /// kargui が題の帯を描くか（Options::custom_title_bar）
    pub custom_title_bar: bool,
    /// 開いた時に前面に出すか（Options::activate）
    pub activate: bool,
    /// 窓のアイコン（無ければ主の窓のアイコン）と、中の大きさの下限
    pub icon: Option<crate::Image>,
    pub min_size: Option<(f32, f32)>,
}

/// ツールチップを出すか
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TipState {
    Hidden,
    /// ポインターが乗ってから、まだ時間がたっていない（出す時刻）
    Waiting(Instant),
    Shown,
}

/// ポインターが止まってからツールチップを出すまでの時間
pub(crate) const TOOLTIP_DELAY: Duration = Duration::from_millis(500);

/// 点 (x, y) から四角までの距離の 2 乗（中なら 0）
fn distance2(r: &Rect, x: f32, y: f32) -> f32 {
    let dx = (r.x - x).max(0.0).max(x - r.right());
    let dy = (r.y - y).max(0.0).max(y - r.bottom());
    dx * dx + dy * dy
}

/// ポインターの形（窓の仕組みごとに、OS の形の名前に直して使う）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PointerShape {
    /// ふつうの矢印
    Default,
    /// 文字を選ぶ I の形
    Text,
    /// 押せるリンクの手の形
    Pointer,
    /// 上下の仕切りを動かす形
    NsResize,
    /// 左右の仕切りを動かす形
    EwResize,
    /// 動かす形（浮いたまとまりの、動かす印の上）
    Move,
    /// 右下へ広げる形（浮いたまとまりの、大きさを変える印の上）
    NwseResize,
}

/// 一覧の、前のフレームでの場所と中身の高さ（物理ピクセル）
#[derive(Clone, Debug)]
pub(crate) struct ListInfo {
    pub id: Id,
    pub rect: Rect,
    pub content_h: f32,
    /// スクロールの棒のつまみ（中身が一覧より高いときだけ）
    pub thumb: Option<Rect>,
    /// スクロールの棒を押せる範囲（つまみの列。一覧の高さ全部）
    pub bar: Option<Rect>,
    /// 中身を描き終えた時の当たりの数（後から上に重ねた浮いたまとまりに隠れているかを見る。中に浮いたまとまりを持つ
    /// 外側の入れ物は、浮いたまとまりより後の数になるので隠れない）
    pub order: usize,
}

/// つかんでいるつまみ
#[derive(Clone, Copy, Debug)]
pub(crate) struct ScrollDrag {
    pub list: Id,
    /// つかんだ所の、つまみの上の端からの距離（物理ピクセル）
    pub grab: f32,
    /// つかんでから届いた移動の数（記録用）
    pub moves: u32,
}

/// 引っ張っている物（Handle::drag_source。押して DRAG_START より動かした所から）
pub(crate) struct Dnd {
    pub source: Id,
    /// 荷物。元の部品がビューで渡す（引っ張り始めた後の最初のビューで入る）
    pub payload: Option<Box<dyn std::any::Any>>,
    /// 元の部品の見えていた範囲（影の大きさ）と、つかんだ所の、範囲の左上からの距離（物理ピクセル）
    pub rect: Rect,
    pub grab: (f32, f32),
    /// 影に書く文字（元の部品の読み上げの名前）
    pub text: String,
    /// 今のポインターの位置
    pub at: (f32, f32),
    /// ポインターの下の、この荷物を受け取れる部品（強調して描く）
    pub over: Option<Id>,
    /// ほかのアプリ（エクスプローラーなど）から来た引っ張り。荷物は Vec<PathBuf>。影は描かない（OS が描く）
    pub external: bool,
}

/// ほかのアプリからの引っ張りの、元の部品の代わりの名前
pub(crate) const EXTERNAL_SOURCE: Id = Id(0x6669_6c65_7300); // "files"

/// 動いている値（Ui::animate）: from から to へ、start から dur の間に、減速の曲線で動く
#[derive(Clone, Copy, Debug)]
pub(crate) struct Anim {
    pub from: f32,
    pub to: f32,
    pub start: Instant,
    pub dur: Duration,
    /// 最後に使ったフレームの番号（使われなくなった物をフレームの終わりで捨てる）
    pub seen: u64,
}

impl Anim {
    /// now の値。曲線は WinUI と同じく「速く始まってゆっくり止まる」（3 乗の減速）
    pub fn value(&self, now: Instant) -> f32 {
        if self.done(now) {
            return self.to;
        }
        let t = (now.saturating_duration_since(self.start).as_secs_f32() / self.dur.as_secs_f32()).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        self.from + (self.to - self.from) * eased
    }

    pub fn done(&self, now: Instant) -> bool {
        self.dur.is_zero() || now >= self.start + self.dur
    }
}

/// 離した所で受け取る物。荷物はビューで元の部品から受け取り（押す・動かす・離すが 1 回に届いた時はここで初めて
/// 入る）、そろったら Handle::dropped が渡す。フレームの終わりまでに受け取られなければ捨てる
pub(crate) struct PendingDrop {
    pub source: Id,
    /// 離した所の受け取る部品（上の物から）と、受け取る荷物の型
    pub targets: Vec<(Id, std::any::TypeId)>,
    pub payload: Option<Box<dyn std::any::Any>>,
    /// 離した所（物理ピクセル。ドッキングは、受け取る部品の中のどこかで落とす先を決める）
    pub at: (f32, f32),
}

/// 押してから横か縦にこれより（論理ピクセル）動かしたら引っ張り始める。それまではクリック（Windows の SM_CXDRAG・
/// SM_CYDRAG と同じ 4）
pub(crate) const DRAG_START: f32 = 4.0;

/// 前のフレームの配置
#[derive(Default)]
pub(crate) struct Prev {
    /// 描いた順。後ろほど上に重なる
    pub hits: Vec<Hit>,
    /// Tab で移る順
    pub focus_order: Vec<Id>,
    pub lists: Vec<ListInfo>,
    /// 入力欄の場所と、文字の左端の x（物理ピクセル）
    pub inputs: HashMap<Id, (Rect, f32)>,
    /// パスワードの欄（inputs の中で、日本語入力を切る物）
    pub passwords: HashSet<Id>,
    /// 複数行の入力欄の、文字の左上（スクロールを入れた窓の座標）、折り返しの幅、見える高さ（物理ピクセル）
    pub areas: HashMap<Id, (f32, f32, f32, f32)>,
    /// 一覧の行の中の、焦点を持てる部品 → (一覧の id, 行の範囲)
    pub rows: HashMap<Id, (Id, Rect)>,
    /// 選べる一覧の行 → (一覧の id, 何行目)
    pub items: HashMap<Id, (Id, usize)>,
    /// 押せない部品（読み上げソフトから届いた操作を捨てるため）
    pub disabled: HashSet<Id>,
    /// 重ねて出した物（描いた順。後ろほど上）
    pub overlays: Vec<OverlayInfo>,
    /// 右クリックのメニューを持つ部品と、その見える範囲、中身を描き終えた時の当たりの数（描いた順。後ろほど内側か上）
    pub context_regions: Vec<(Id, Rect, usize)>,
    /// メニューバーの見出し（並んだ順）
    pub menubar: Vec<Id>,
    /// アプリが描く題の帯の範囲（Csd がある時。空いた所を押すと窓を動かす）
    pub title_bar: Option<Rect>,
    /// Windows の形の題の帯の、最大化のボタンの四角（窓の処理が HTMAXBUTTON と答える所）
    pub caption_max: Option<Rect>,
    /// 数の入力欄の刻み（Up・Down で使う。NumberInput::step は作った後で付くので、前のフレームの値を使う）
    pub steps: HashMap<Id, f64>,
    /// 数の入力欄の小さいボタン → 欄（押しても焦点を欄に残す）
    pub spins: HashMap<Id, Id>,
    /// 仕切り → 入れ物の始まりと長さ（つかんで動かす時と、広げられる限りに使う。物理ピクセル）
    pub splits: HashMap<Id, (f32, f32)>,
    /// ドッキングの入れ物 → 全体とまとまりの場所（タブを落とした先を決める）
    pub docks: HashMap<Id, super::dock::DockGeom>,
    /// 仕切り → 上下に分けるか（ポインターの形を決める）
    pub split_bars: HashMap<Id, bool>,
    /// 決まったポインターの形を持つ部品（浮いたまとまりの、動かす印と大きさを変える印）
    pub shapes: HashMap<Id, PointerShape>,
    /// ふつうの部品の上に重ねた物（ドッキングの浮いたまとまり）の範囲と、描き始めた時の当たりの数。それより前に描いた
    /// 一覧・スクロールの棒・右クリックの範囲は、この範囲の中では隠れている
    pub occluders: Vec<(Rect, usize)>,
    /// 自由に描く面の場所（ポインターを面の座標にする）
    pub canvases: HashMap<Id, Rect>,
    /// 引っ張り出せる部品（Handle::drag_source）: 見えている範囲、影に書く文字、描いた時の当たりの数
    /// （後から上に描いた物に隠れているかを見る）。描いた順
    pub drag_sources: Vec<(Id, Rect, String, usize)>,
    /// 落とせる部品（Handle::dropped）: 見えている範囲、受け取る荷物の型、描いた時の当たりの数。描いた順
    pub drop_targets: Vec<(Id, Rect, std::any::TypeId, usize)>,
}

pub(crate) struct Context {
    pub scale: f32,
    /// 窓の中の大きさ（物理ピクセル）
    pub width: f32,
    pub height: f32,
    pub dark: bool,
    pub theme: Theme,
    pub text: Text,
    pub prev: Prev,
    pub edits: HashMap<Id, EditState>,
    /// 入力欄の横のスクロール（物理ピクセル）
    pub edit_scroll: HashMap<Id, f32>,
    /// 一覧のスクロールの位置（論理ピクセル）
    pub scrolls: HashMap<Id, f32>,
    /// このフレームの時刻（Ui::now。試験台は skew で進める）
    pub now: Instant,
    /// 動く部品（進み具合・輪）の位相の起点
    pub epoch: Instant,
    /// 試験台が進めた時間（Harness::wait）
    pub skew: Duration,
    /// 試験台の止めた時計（起点からの時間）。Some なら本当の時刻を使わず、Harness::wait で進めた分だけ進む
    /// （写真と試験が毎回同じになるように）
    pub frozen: Option<Duration>,
    /// 次に起こす時刻（Ui::request_frame_after とツールチップ）。窓はタイマーで起きる
    pub wake_at: Option<Instant>,
    /// ほかのスレッドから窓を起こす係（Ui::waker）
    pub waker: Waker,
    /// 形の表に入れたアイコン（アイコン, 画素の大きさ）。表を作り直したら（generation が変わったら）忘れる
    icons: HashMap<(Icon, u32), Option<Slot>>,
    icons_gen: u64,
    /// 形の表に入れた、自由に描く面の線と多角形（形を表す数 → 表の場所）。表を作り直したら忘れる
    masks: HashMap<u64, Option<Slot>>,
    masks_gen: u64,
    pub focus: Option<Id>,
    pub hover: Option<Id>,
    /// ポインターが今の部品に乗った時刻（ツールチップを出すまでの時間を数える）
    pub hover_since: Option<Instant>,
    /// Esc や押したことで消したツールチップの部品（ポインターか焦点が別の部品へ移るまで出さない）
    pub tooltip_hidden: Option<Id>,
    /// 重ねて出した物 → 開く前に焦点があった部品（閉じた時に戻す）
    pub overlay_return: HashMap<Id, Id>,
    /// 重ねて出した物 → 開いた時に焦点を置く部品（無ければ中の最初の部品。ドロップダウンは選んでいる項目）
    pub overlay_focus_hint: HashMap<Id, Id>,
    /// 開いているドロップダウン（使う側の書き方を簡単にするため、kargui が持つ）
    pub open: HashSet<Id>,
    /// 開いているメニューの列（一番外のメニューから入れ子の順）。メニューは同時に 1 列だけ開く
    pub menus: Vec<Id>,
    /// 数の入力欄で打っている途中の文字（焦点がある間だけ。外れたら決めて消す）
    /// 数の入力の小さいボタンを押したままの間: 欄 → （押しているボタン, 次に進める時）
    pub spin_hold: HashMap<Id, (Id, Instant)>,
    pub number_text: HashMap<Id, String>,
    /// ツールバー → 最後に焦点があったボタン（Tab はそこにだけ止まる）
    pub toolbar_last: HashMap<Id, Id>,
    /// 木の表示の枝 → 開いているか（kargui が持つ。最初は閉じている）
    pub expanded: HashMap<Id, bool>,
    /// 表の列の幅（表, 何列目 → 論理ピクセル）。つまみで変えた幅を覚える
    pub column_widths: HashMap<(Id, usize), f32>,
    /// 押している列のつまみと、押した所のつまみの真ん中からのずれ（物理ピクセル）。押している間、ずれを保って幅を決める
    pub column_grab: Option<(Id, f32)>,
    /// 押している浮いたまとまりの印と、押した時の浮いたまとまりの場所と大きさ（論理ピクセル）、押した所（物理ピクセル）
    pub float_grab: Option<(Id, [f32; 4], (f32, f32))>,
    /// 最後に押した位置（物理ピクセル）。押す・動かす・離すが 1 つのフレームに届いても、押した所が分かる
    pub press_at: Option<(f32, f32)>,
    /// このフレームで押した所（物理ピクセル。浮き出しを閉じただけの押しは入れない）。ビューの 1 回目の後で消す
    pub press_now: Option<(f32, f32)>,
    /// 日付の部品の言葉（Ui::set_date_text）
    pub date_text: crate::date::DateText,
    /// 今日の日付を決めておく（試験台は 2026-11-22。None なら OS に聞く）
    pub today: Option<crate::date::Date>,
    /// 開いている暦の、焦点のある日（日付の欄 → 日付）
    pub calendar_cursor: HashMap<Id, crate::date::Date>,
    /// 時刻の欄の分の刻み（前のフレームで .minute_step() が決めた物。時刻の欄 → 分）
    pub minute_steps: HashMap<Id, u32>,
    /// 色の選択の言葉
    pub color_text: crate::color::ColorText,
    /// 開いている色の選択の状態（色の欄 → 状態）
    pub color_state: HashMap<Id, crate::color::PickerState>,
    /// 色の選択の設定（前のフレームの .alpha() と .swatches()。色の欄 → 設定）
    pub color_options: HashMap<Id, crate::color::PickerOptions>,
    /// 色の選択の濃淡の絵（作り直さないよう、最近の 8 枚を覚える）
    pub color_images: Vec<(String, crate::image::Image)>,
    /// この回のビューで開くように頼まれた窓（Ui::window。窓の処理と試験台がそろえる）
    pub windows: Vec<WindowRequest>,
    /// 利用者が閉じた窓のキー（次のビューの Ui::window が open を false にする）
    pub closed_windows: HashSet<String>,
    /// この Context の窓のキー（Ui::window で開いた窓。主の窓は None）
    pub window_key: Option<String>,
    /// アプリが窓の場所を決められるか（Ui::window の位置が効き、窓の場所を読める。Wayland は false）
    pub places_windows: bool,
    /// 窓の画面の上の場所（窓の処理が、開いた時と動いた時に入れる。分からなければ None）
    pub place: Option<WindowPlace>,
    /// 出すように頼まれたファイルのダイアログ（キーと中身。窓の処理がフレームの後で出す）
    pub file_dialog_request: Option<(String, crate::file_dialog::FileDialog)>,
    /// ファイルのダイアログで選んだファイル（キー → ファイル。Ui::chosen_files が 1 回だけ取り出す）
    pub file_results: HashMap<String, Vec<std::path::PathBuf>>,
    /// アプリを終える（Ui::quit。窓の処理が主の窓を閉じる）
    pub quit: bool,
    /// トレイの中身（主の窓のビューの Ui::tray。回すたびに作り直す。窓の処理がフレームの後で OS に渡す）
    pub tray: Option<crate::ui::tray::TrayDesc>,
    /// OS から届いたトレイの知らせ（入力を渡す回のビューの Ui::tray が読む。窓の処理がフレームを回す前に、左クリック
    /// とメニューの選択がまとめて届くことがあるので並びで持つ）
    pub tray_input: Vec<crate::ui::tray::TrayInput>,
    /// 窓を出す（true）か隠す（false）頼み（Ui::show_window・hide_window。窓の処理がフレームの後で取り出す）
    pub show_request: Option<bool>,
    /// 出すように頼まれた通知（キーと中身。窓の処理がフレームの後で取り出す）
    pub notify_requests: Vec<(String, crate::ui::notify::Notification)>,
    /// 押された通知の答え（キー → 答え。窓の処理が入れ、Ui::notification_clicked が 1 回だけ取り出す）
    pub notification_results: HashMap<String, crate::ui::notify::NotificationAction>,
    /// アプリが描く題の帯（合成が描かない時。Linux の GNOME など。窓の処理が毎フレーム入れる）
    pub csd: Option<Csd>,
    /// 題の帯の操作で、窓の処理に頼む事（動かす・最大化など。窓の処理がフレームの後で取り出す）
    pub csd_request: Option<CsdRequest>,
    /// 題の帯の空いた所を押した位置（動かしたら「窓を動かす」を頼む）
    pub title_press: Option<(f32, f32)>,
    /// この回の clicked はキー（Enter・Space）か読み上げソフトから来た（ポインターの位置を見ない）
    pub clicked_by_key: bool,
    /// ビューの途中で、前に作った部品が見た状態を変えた（メニューを開いた・閉じた）。もう一度回す
    pub again: bool,
    /// ポインターが止まった時の動き（入れ子のメニューの開閉）を済ませた項目と、乗った時刻
    pub rested: Option<(Id, Instant)>,
    /// 重ねて出した物が閉じた時に焦点を移す先（overlay_return より先に使う。None なら焦点を外す）
    pub overlay_after: HashMap<Id, Option<Id>>,
    /// F10 か見出しを押してメニューバーへ入る前の焦点（選んだ後と Esc で戻す）
    pub bar_return: Option<Id>,
    /// 右クリックのメニューを開く頼み（対象の部品と、出す場所）。キーボードなら部品の左下
    pub context_request: Option<(Id, (f32, f32))>,
    /// 開いている右クリックのメニューの場所（物理ピクセル）
    pub context_at: (f32, f32),
    pub pressed: Option<Id>,
    /// 押した所にあった、引っ張り出せる部品と押した位置（まだ動かしていない。動かしたら dnd になる）
    pub press_source: Option<(Id, f32, f32)>,
    /// 引っ張っている物
    pub dnd: Option<Dnd>,
    /// 離した所で受け取る物（フレームの終わりまで持つ。入力の回の後の描く回でも受け取れる）
    pub drop: Option<PendingDrop>,
    /// ファイル（Vec<PathBuf>）を受け取る部品がある。窓の処理はこれを見て、OS にファイルを落とせる窓だと届け出る
    /// （使わないアプリは届け出ず、OLE の準備をしない）
    pub wants_files: bool,
    /// 動いている値（Ui::animate。部品の id か、アプリの鍵から作った id → 動き）
    pub anims: HashMap<Id, Anim>,
    /// 利用者が OS で動きを減らしている（Windows の「アニメーション効果」が切）。動きは目標へ飛ぶ
    pub reduce_motion: bool,
    /// 窓の書体（Options::font と Ui::set_font。部品が書体を決めていない時に使う）
    pub font: crate::text::FontId,
    /// フレームの番号（Runtime が数える。使われなくなった動きを捨てるのに使う）
    pub frame_no: u64,
    /// ポインターが乗っているスクロールの棒（一覧の id）
    pub bar_hover: Option<Id>,
    /// つかんでいるスクロールの棒のつまみ
    pub drag: Option<ScrollDrag>,
    /// キーボードで焦点を動かしたあとだけ、焦点の輪を描く
    pub focus_visible: bool,
    // ---- この回の入力 ----
    pub clicked: Option<Id>,
    /// 閉じる重ねた物（外を押した・Esc）。Ui::popup と Ui::dialog が open を false にする
    pub closing: HashSet<Id>,
    /// 焦点のある入力欄へ届ける入力
    pub keys: Vec<Event>,
    /// 部品が使わなかったキー（Ui::shortcut）
    pub app_keys: Vec<(Key, Mods)>,
    /// キャレットが見えるようにスクロールする複数行の入力欄（打った・動かした時。描く時に使って消す）
    pub reveal: Option<Id>,
    /// 入力欄の中のクリック（場所と Shift）
    pub pointer_text: Option<(Id, f32, f32, bool, u8)>,
    /// 入力欄を押したまま動かした最後の位置（入力欄, x, y）。押した所から選ぶ
    pub pointer_text_drag: Option<(Id, f32, f32)>,
    /// 押している入力欄（押したまま動かした時に選ぶため）
    pub press_text: Option<Id>,
    /// 押している部品の上のポインターの位置（押した・動かした・離した時に入れる）
    pub pointer: Option<(f32, f32)>,
    /// 配置の後に、いちばん近いスクロールの入れ物の上端へ持ってくる部品（Handle::scroll_into_view）
    pub scroll_into_view: Option<Id>,
    /// このフレームでポインターを離した部品（押した部品の上で離した時。スライダーが押した位置を読む）
    pub released: Option<Id>,
    /// 変換中の文字（焦点のある入力欄の中に描く）
    pub preedit: String,
    /// 変換中の文字の文節（IME が知らせた時だけ）
    pub preedit_clauses: Option<Clauses>,
    /// 焦点が移る前に、元の入力欄へ確定する変換中の文字（入力欄の id と文字）
    pub pending_commit: Option<(Id, String)>,
    /// IME の変換を取り消すように頼む（変換中の文字をこちらで確定した時）。窓の処理が読んで戻す
    pub ime_cancel: bool,
    pub want_frame: bool,
    /// ドッキングにタブを引っ張っている間に、最後に記録したポインターの下（まとまりと落とす先）。変わった時だけ記録する
    pub dock_zone_log: Option<String>,
    pub clipboard: Box<dyn Clipboard>,
    // ---- 描いた結果 ----
    /// 文字を打つ位置と、変換中の文字の範囲（物理ピクセル）。IME に知らせる
    pub caret: Option<Rect>,
    pub preedit_rect: Option<Rect>,
    pub input_rect: Option<Rect>,
    /// 焦点のある入力欄の写し（IME の問い合わせに答えるため）
    pub ime_field: Option<ImeField>,
}

impl Context {
    pub fn new(text: Text, dark: bool, clipboard: Box<dyn Clipboard>) -> Self {
        Self {
            scale: 1.0,
            width: 0.0,
            height: 0.0,
            dark,
            theme: if dark { Theme::dark() } else { Theme::light() },
            text,
            prev: Prev::default(),
            edits: HashMap::new(),
            edit_scroll: HashMap::new(),
            scrolls: HashMap::new(),
            now: Instant::now(),
            epoch: Instant::now(),
            skew: Duration::ZERO,
            frozen: None,
            wake_at: None,
            waker: Waker::new(|| {}),
            icons: HashMap::new(),
            icons_gen: 0,
            masks: HashMap::new(),
            masks_gen: 0,
            focus: None,
            hover: None,
            hover_since: None,
            tooltip_hidden: None,
            overlay_return: HashMap::new(),
            overlay_focus_hint: HashMap::new(),
            open: HashSet::new(),
            menus: Vec::new(),
            spin_hold: HashMap::new(),
            number_text: HashMap::new(),
            toolbar_last: HashMap::new(),
            expanded: HashMap::new(),
            column_widths: HashMap::new(),
            column_grab: None,
            float_grab: None,
            press_at: None,
            press_now: None,
            date_text: crate::date::DateText::english(),
            today: None,
            calendar_cursor: HashMap::new(),
            minute_steps: HashMap::new(),
            color_text: crate::color::ColorText::english(),
            color_state: HashMap::new(),
            color_options: HashMap::new(),
            color_images: Vec::new(),
            windows: Vec::new(),
            closed_windows: HashSet::new(),
            window_key: None,
            places_windows: true,
            place: None,
            file_dialog_request: None,
            file_results: HashMap::new(),
            quit: false,
            tray: None,
            tray_input: Vec::new(),
            show_request: None,
            notify_requests: Vec::new(),
            notification_results: HashMap::new(),
            csd: None,
            csd_request: None,
            title_press: None,
            clicked_by_key: false,
            again: false,
            rested: None,
            overlay_after: HashMap::new(),
            bar_return: None,
            context_request: None,
            context_at: (0.0, 0.0),
            pressed: None,
            press_source: None,
            dnd: None,
            drop: None,
            wants_files: false,
            anims: HashMap::new(),
            reduce_motion: false,
            font: crate::text::FontId(0),
            frame_no: 0,
            bar_hover: None,
            drag: None,
            focus_visible: false,
            clicked: None,
            closing: HashSet::new(),
            keys: Vec::new(),
            app_keys: Vec::new(),
            reveal: None,
            pointer_text: None,
            pointer_text_drag: None,
            press_text: None,
            pointer: None,
            released: None,
            scroll_into_view: None,
            preedit: String::new(),
            preedit_clauses: None,
            pending_commit: None,
            ime_cancel: false,
            want_frame: false,
            dock_zone_log: None,
            clipboard,
            caret: None,
            preedit_rect: None,
            input_rect: None,
            ime_field: None,
        }
    }

    pub fn set_dark(&mut self, dark: bool) {
        if self.dark != dark {
            self.dark = dark;
            self.theme = if dark { Theme::dark() } else { Theme::light() };
        }
    }

    /// ポインターが乗っている部品を変える（変わった時だけ、乗った時刻を覚え直す）
    pub fn set_hover(&mut self, id: Option<Id>) {
        if id != self.hover {
            self.hover = id;
            self.hover_since = id.map(|_| self.clock());
            self.tooltip_hidden = None;
        }
    }

    /// id の部品のツールチップを出すか。キーボードで焦点が来た時はすぐ、ポインターは止まってから
    pub fn tip_state(&self, id: Id) -> TipState {
        if self.tooltip_hidden == Some(id) || self.pressed == Some(id) {
            TipState::Hidden
        } else if self.focus_visible && self.focus == Some(id) {
            TipState::Shown
        } else if self.hover == Some(id) {
            match self.hover_since {
                Some(t) if self.now.saturating_duration_since(t) >= TOOLTIP_DELAY => TipState::Shown,
                Some(t) => TipState::Waiting(t + TOOLTIP_DELAY),
                None => TipState::Hidden,
            }
        } else {
            TipState::Hidden
        }
    }

    /// size × size 画素のアイコンの、形の表の中の場所（初めてなら塗って入れる）
    /// 自由に描く面の線や多角形の濃さの形（w×h）を、形の表に入れる。同じ形は前に入れた場所を使う
    pub fn mask_slot(&mut self, key: u64, w: u32, h: u32, make: impl FnOnce() -> Vec<u8>) -> Option<Slot> {
        if self.masks_gen != self.text.atlas.generation {
            self.masks.clear();
            self.masks_gen = self.text.atlas.generation;
        }
        if let Some(s) = self.masks.get(&key) {
            return *s;
        }
        let px = make();
        let slot = match self.text.atlas.alloc(w, h) {
            Some((x, y)) => {
                self.text.atlas.write(x, y, w, h, &px);
                Some(Slot { x: x as u16, y: y as u16, w: w as u16, h: h as u16, left: 0, top: 0 })
            }
            None => {
                log::event("mask.atlas", &format!("w={w} h={h}"), "full; reset");
                self.text.reset();
                return None;
            }
        };
        self.masks.insert(key, slot);
        slot
    }

    pub fn icon_slot(&mut self, icon: Icon, size: u32) -> Option<Slot> {
        if self.icons_gen != self.text.atlas.generation {
            self.icons.clear();
            self.icons_gen = self.text.atlas.generation;
        }
        if let Some(s) = self.icons.get(&(icon, size)) {
            return *s;
        }
        if size == 0 {
            return None;
        }
        let px = crate::icon::rasterize(icon, size);
        let slot = match self.text.atlas.alloc(size, size) {
            Some((x, y)) => {
                self.text.atlas.write(x, y, size, size, &px);
                Some(Slot { x: x as u16, y: y as u16, w: size as u16, h: size as u16, left: 0, top: 0 })
            }
            None => {
                // 表がいっぱいなら作り直す（次のフレームで文字もアイコンも入れ直す）
                log::event("icon.atlas", &format!("icon={icon:?} size={size}"), "full; reset");
                self.text.reset();
                return None;
            }
        };
        self.icons.insert((icon, size), slot);
        slot
    }

    /// 今の時刻（試験台が進めた分を足す）
    pub fn clock(&self) -> Instant {
        match self.frozen {
            Some(t) => self.epoch + t,
            None => Instant::now() + self.skew,
        }
    }

    /// id の値を target へ dur かけて動かし、今の値を返す。初めての時と、動きを減らす設定の時は target をそのまま返す。
    /// target が変わったら、今いる所から動き直す。動いている間は次のフレームを頼む（止まれば何も描かない）
    pub fn animate(&mut self, id: Id, target: f32, dur: Duration) -> f32 {
        let (now, frame) = (self.now, self.frame_no);
        if self.reduce_motion || dur.is_zero() {
            self.anims.insert(id, Anim { from: target, to: target, start: now, dur: Duration::ZERO, seen: frame });
            return target;
        }
        // 初めて呼んだ値は、止まった形で覚える（動かないのにフレームを頼まない）
        let a = self.anims.entry(id).or_insert(Anim { from: target, to: target, start: now, dur: Duration::ZERO, seen: frame });
        if a.to != target {
            let at = a.value(now);
            *a = Anim { from: at, to: target, start: now, dur, seen: frame };
        }
        a.seen = frame;
        let v = a.value(now);
        if !a.done(now) {
            self.want_frame = true;
        }
        v
    }

    /// t までに起こしてもらう（早い方を残す）
    pub fn wake_by(&mut self, t: Instant) {
        self.wake_at = Some(self.wake_at.map_or(t, |w| w.min(t)));
    }

    /// 一番上の、外を押すと閉じる浮き出し（その上にダイアログがあれば無し）
    fn top_popup(&self) -> Option<&OverlayInfo> {
        self.prev.overlays.iter().rev().find(|o| o.kind != OverlayKind::Tooltip).filter(|o| o.kind.light_dismiss())
    }

    /// 開いているメニューの列を depth 個に切る（深いメニューを閉じる）。変えたらビューを回し直す
    pub fn menus_truncate(&mut self, depth: usize) {
        if self.menus.len() > depth {
            self.menus.truncate(depth);
            self.again = true;
        }
    }

    /// depth の深さに pop のメニューを開く（それより深いメニューは閉じる）。変えたらビューを回し直す
    pub fn menus_open(&mut self, depth: usize, pop: Id) {
        if self.menus.get(depth) == Some(&pop) && self.menus.len() == depth + 1 {
            return;
        }
        self.menus.truncate(depth);
        if self.menus.len() == depth {
            self.menus.push(pop);
        }
        self.again = true;
    }

    /// 前のフレームのメニュー（入れ子を含む）
    fn menu_layers(&self) -> Vec<Id> {
        self.prev.overlays.iter().filter(|l| l.kind == OverlayKind::Menu).map(|l| l.id).collect()
    }

    /// 外を押した時に一緒に閉じる物（メニューなら開いている入れ子のメニュー全部）と、その範囲
    fn dismiss_layers(&self, o: &OverlayInfo) -> Vec<(Id, Rect)> {
        if o.kind == OverlayKind::Menu {
            self.prev.overlays.iter().filter(|l| l.kind == OverlayKind::Menu).map(|l| (l.id, l.rect)).collect()
        } else {
            vec![(o.id, o.rect)]
        }
    }

    /// 部品 f がメニューの中にあるか
    fn in_menu(&self, f: Id) -> bool {
        self.prev.overlays.iter().any(|l| l.kind == OverlayKind::Menu && l.focus.contains(&f))
    }

    /// 一番上のダイアログ（焦点を中に閉じ込める）
    fn top_dialog(&self) -> Option<&OverlayInfo> {
        self.prev.overlays.iter().rev().find(|o| o.kind == OverlayKind::Dialog)
    }

    /// 日本語入力（IME）を向ける入力欄: 焦点のある入力欄。パスワードの欄では IME を切る（打った字を IME の変換や
    /// 学習に渡さない。Windows のパスワードの欄（ES_PASSWORD）と同じ）。窓の処理はこれで IME を入れるか決める
    pub fn ime_target(&self) -> Option<Id> {
        self.focus.filter(|f| self.prev.inputs.contains_key(f) && !self.prev.passwords.contains(f))
    }

    /// 前のフレームで (x, y) にあった、押せる部品（上に重なっている物）。
    /// 本来の範囲で見つからなければ、当たりの余白の中で一番近い部品にする（隣の部品の範囲は奪わない）
    pub fn hit(&self, x: f32, y: f32) -> Option<&Hit> {
        if let Some(h) = self.prev.hits.iter().rev().find(|h| h.rect.contains(x, y)) {
            return Some(h);
        }
        self.prev
            .hits
            .iter()
            .filter(|h| h.slop.contains(x, y))
            .min_by(|a, b| distance2(&a.rect, x, y).total_cmp(&distance2(&b.rect, x, y)))
    }

    /// (x, y) にあるスクロールの棒の一覧。部品の本来の範囲の中なら None（部品が勝つ）。
    /// ただし棒を横に全部覆う当たり（選べる一覧の行、複数行の入力欄そのもの）は棒に譲る。
    /// 覆われたままだと、棒を押しても行が選ばれるだけで、つまみを引けなかった
    pub fn scrollbar_at(&self, x: f32, y: f32) -> Option<&ListInfo> {
        let l = self.prev.lists.iter().rev().find(|l| l.bar.is_some_and(|b| b.contains(x, y)) && !self.hidden(l.order, x, y))?;
        let bar = l.bar?;
        // 棒は部品の右の端の外まで押せるので、覆うかどうかは部品の中にある分で決める
        let bar_right = bar.right().min(l.rect.right());
        let covers_bar = |r: &Rect| r.x <= bar.x && r.right() >= bar_right;
        if self.prev.hits.iter().any(|h| h.rect.contains(x, y) && !covers_bar(&h.rect)) {
            return None;
        }
        Some(l)
    }

    /// (x, y) で見せるポインターの形。仕切りの上は上下か左右の矢印、入力欄の上は I の形、押せるリンクの上は手の形。
    /// スクロールの棒の上は、入力欄の中でも矢印（押すと棒が動くので）
    pub fn pointer_shape(&self, x: f32, y: f32) -> PointerShape {
        if self.scrollbar_at(x, y).is_some() {
            return PointerShape::Default;
        }
        let hit = self.hit(x, y);
        if let Some(&s) = hit.and_then(|h| self.prev.shapes.get(&h.id)) {
            return s;
        }
        match hit.and_then(|h| self.prev.split_bars.get(&h.id).copied()) {
            Some(true) => PointerShape::NsResize,
            Some(false) => PointerShape::EwResize,
            None if hit.is_some_and(|h| h.text_input) => PointerShape::Text,
            None if hit.is_some_and(|h| h.link && h.enabled) => PointerShape::Pointer,
            None => PointerShape::Default,
        }
    }

    /// 一覧のスクロールの位置（論理ピクセル）を、端で止めて設定する
    fn set_scroll(&mut self, l: &ListInfo, v: f32) -> f32 {
        let max = ((l.content_h - l.rect.h) / self.scale).max(0.0);
        let v = v.clamp(0.0, max);
        self.scrolls.insert(l.id, v);
        v
    }

    /// スクロールの棒を押した。つまみならつかみ、空いた所ならそちらへ一覧の高さ 1 つ分動かす
    fn press_scrollbar(&mut self, l: &ListInfo, y: f32) {
        let Some(thumb) = l.thumb else { return };
        let now = self.scrolls.get(&l.id).copied().unwrap_or(0.0);
        if y >= thumb.y && y < thumb.bottom() {
            self.drag = Some(ScrollDrag { list: l.id, grab: y - thumb.y, moves: 0 });
            log::event("scrollbar.grab", &format!("y={y} thumb_y={:.0}", thumb.y), &format!("scroll={now:.0}"));
        } else {
            let page = l.rect.h / self.scale;
            let up = y < thumb.y;
            let v = self.set_scroll(l, if up { now - page } else { now + page });
            log::event("scrollbar.page", &format!("y={y} dir={}", if up { "up" } else { "down" }), &format!("scroll={v:.0}"));
        }
    }

    /// つかんだつまみを y まで動かす（つかんだ所がポインターの下に来るように）
    fn drag_scrollbar(&mut self, d: ScrollDrag, y: f32) {
        let Some(l) = self.prev.lists.iter().find(|l| l.id == d.list).cloned() else { return };
        let Some(thumb) = l.thumb else { return };
        let track = l.rect.h - thumb.h;
        let f = if track > 0.0 { ((y - d.grab - l.rect.y) / track).clamp(0.0, 1.0) } else { 0.0 };
        let max = ((l.content_h - l.rect.h) / self.scale).max(0.0);
        self.set_scroll(&l, f * max);
    }

    /// 描いた時の当たりの数が order の部品（範囲 r）が、(x, y) で後から描いた物（重ねて出した物など）に隠れているか。
    /// 中の部品（r の中に収まる当たり）は隠したことにしない
    /// 描き終えた時の当たりの数が order の物が、(x, y) で後から重ねた浮いたまとまりに隠れているか（浮いたまとまりを
    /// 描き始める前に描き終えた物だけが隠れる）
    pub(crate) fn hidden(&self, order: usize, x: f32, y: f32) -> bool {
        self.prev.occluders.iter().any(|&(r, o)| o >= order && r.contains(x, y))
    }

    fn covered(&self, r: &Rect, order: usize, x: f32, y: f32) -> bool {
        let inside = |q: &Rect| q.x >= r.x - 0.5 && q.y >= r.y - 0.5 && q.right() <= r.right() + 0.5 && q.bottom() <= r.bottom() + 0.5;
        self.prev.hits.iter().skip(order).any(|h| h.rect.contains(x, y) && !inside(&h.rect))
    }

    /// (x, y) で上に見えている、引っ張り出せる部品（内側の物）
    fn source_at(&self, x: f32, y: f32) -> Option<&(Id, Rect, String, usize)> {
        self.prev.drag_sources.iter().rev().find(|s| s.1.contains(x, y) && !self.covered(&s.1, s.3, x, y))
    }

    /// (x, y) の落とせる部品と受け取る荷物の型（上の物・内側の物から）
    fn targets_at(&self, x: f32, y: f32) -> Vec<(Id, std::any::TypeId)> {
        self.prev.drop_targets.iter().rev().filter(|t| t.1.contains(x, y) && !self.covered(&t.1, t.3, x, y)).map(|t| (t.0, t.2)).collect()
    }

    /// 引っ張っている荷物を (x, y) で受け取る部品（荷物がまだ届いていなければ None）
    fn target_for(&self, d: &Dnd, x: f32, y: f32) -> Option<Id> {
        let ty = d.payload.as_ref().map(|p| (**p).type_id())?;
        self.targets_at(x, y).into_iter().find(|t| t.1 == ty).map(|t| t.0)
    }

    /// 引っ張っている間のポインターの形: Some(true) は受け取れる所（ふつうの矢印）、Some(false) は受け取れない所
    /// （禁止の形）。引っ張っていなければ None。荷物がまだ届いていない（引っ張り始めた直後の）間は矢印
    pub fn drag_cursor(&self, x: f32, y: f32) -> Option<bool> {
        let d = self.dnd.as_ref()?;
        Some(d.payload.is_none() || self.target_for(d, x, y).is_some())
    }

    /// 押した所 (x0, y0) から (x, y) まで動かしたので、引っ張り始める。押していた部品は押したことにしない
    fn start_drag(&mut self, source: Id, x0: f32, y0: f32, x: f32, y: f32) {
        self.press_source = None;
        let Some((_, rect, text, _)) = self.prev.drag_sources.iter().find(|s| s.0 == source).cloned() else { return };
        self.pressed = None;
        self.pointer = None;
        self.tooltip_hidden = self.hover;
        log::event("drag.start", &format!("source={:x} text={text:?}", source.0), &format!("at={x:.0},{y:.0}"));
        self.dock_zone_log = None;
        self.dnd = Some(Dnd { source, payload: None, rect, grab: (x0 - rect.x, y0 - rect.y), text, at: (x, y), over: None, external: false });
    }

    /// (x, y) にファイル（Vec<PathBuf>）を受け取る部品があるか。窓の処理が OS の問い合わせ（IDropTarget）にその場で答える
    pub fn accepts_files_at(&self, x: f32, y: f32) -> bool {
        let ty = std::any::TypeId::of::<Vec<std::path::PathBuf>>();
        self.targets_at(x, y).iter().any(|t| t.1 == ty)
    }

    /// ほかのアプリからファイルを引っ張って入ってきた。荷物は初めから分かっている
    fn enter_files(&mut self, x: f32, y: f32, paths: Vec<std::path::PathBuf>) {
        self.cancel_drag("another drag entered");
        log::event("drag.files_enter", &format!("files={} at={x:.0},{y:.0}", paths.len()), &format!("accepts={}", self.accepts_files_at(x, y)));
        let text = format!("{} files", paths.len());
        self.dnd = Some(Dnd { source: EXTERNAL_SOURCE, payload: Some(Box::new(paths)), rect: Rect::new(x, y, 0.0, 0.0), grab: (0.0, 0.0), text, at: (x, y), over: None, external: true });
        self.move_drag(x, y);
    }

    /// 引っ張っている物を (x, y) へ動かし、下の受け取る部品を決め直す
    fn move_drag(&mut self, x: f32, y: f32) {
        let over = self.dnd.as_ref().and_then(|d| self.target_for(d, x, y));
        if let Some(d) = self.dnd.as_mut() {
            d.at = (x, y);
            if d.over != over {
                log::event("drag.over", &format!("source={:x} at={x:.0},{y:.0}", d.source.0), &format!("target={:?}", over.map(|o| format!("{:x}", o.0))));
                d.over = over;
            }
        }
    }

    /// (x, y) で離した。受け取る部品があれば、ビューで渡す（荷物の型が分からない時は、そこの受け取る部品を全部候補にする）
    fn finish_drag(&mut self, d: Dnd, x: f32, y: f32) {
        let targets = self.targets_at(x, y);
        let ty = d.payload.as_ref().map(|p| (**p).type_id());
        let takes = match ty {
            Some(t) => targets.iter().any(|g| g.1 == t),
            None => !targets.is_empty(),
        };
        if takes {
            log::event("drag.drop", &format!("source={:x} at={x:.0},{y:.0}", d.source.0), &format!("targets={} payload={}", targets.len(), ty.is_some()));
            self.drop = Some(PendingDrop { source: d.source, targets, payload: d.payload, at: (x, y) });
        } else {
            log::event("drag.cancel", &format!("source={:x} at={x:.0},{y:.0}", d.source.0), "why=no-target");
        }
    }

    /// 引っ張りをやめる（Esc・マウスのつかみが外れた）
    fn cancel_drag(&mut self, why: &str) {
        self.press_source = None;
        if let Some(d) = self.dnd.take() {
            log::event("drag.cancel", &format!("source={:x}", d.source.0), &format!("why={why}"));
        }
    }

    fn is_text_input(&self, id: Id) -> bool {
        self.prev.hits.iter().any(|h| h.id == id && h.text_input)
    }

    fn move_focus(&mut self, back: bool) {
        let order = match self.top_dialog() {
            Some(d) => &d.focus,
            None => &self.prev.focus_order,
        };
        if order.is_empty() {
            return;
        }
        let at = self.focus.and_then(|f| order.iter().position(|&o| o == f));
        let next = match (at, back) {
            (None, false) => 0,
            (None, true) => order.len() - 1,
            (Some(i), false) => (i + 1) % order.len(),
            (Some(i), true) => (i + order.len() - 1) % order.len(),
        };
        self.focus = Some(order[next]);
    }

    /// 入力を部品へ振り分ける。ビューの 1 回目の前に呼ぶ
    pub fn handle(&mut self, events: Vec<Event>) {
        for e in events {
            match e {
                Event::PointerDown { x, y, shift, count } => {
                    self.cancel_drag("pressed again");
                    self.commit_preedit("pointer");
                    // 押した部品のツールチップは消す
                    self.tooltip_hidden = self.hover;
                    // 浮き出しの外（出す元の部品も除く）を押したら閉じる。そのクリックは下の部品に届けない
                    if let Some(o) = self.top_popup().cloned() {
                        let layers = self.dismiss_layers(&o);
                        let on = |id: Id| self.prev.hits.iter().find(|h| h.id == id).is_some_and(|h| h.rect.contains(x, y));
                        // メニューは、開いている入れ子のメニュー全部と、一番外の出す元と、メニューバーの見出しを内側とみなす
                        let root = if o.kind == OverlayKind::Menu { self.prev.overlays.iter().find(|l| l.kind == OverlayKind::Menu).unwrap_or(&o) } else { &o };
                        let on_anchor = root.anchor.is_some_and(on);
                        let on_bar = o.kind == OverlayKind::Menu && self.prev.menubar.iter().any(|&b| on(b));
                        if !layers.iter().any(|(_, r)| r.contains(x, y)) && !on_anchor && !on_bar {
                            log::event("popup.close", &format!("x={x} y={y} why=outside"), &format!("id={:x}", o.id.0));
                            for (id, _) in layers {
                                self.closing.insert(id);
                            }
                            self.pressed = None;
                            continue;
                        }
                    }
                    // 押した所（浮いたまとまりの上なら、ドッキングがいちばん上にする）
                    self.press_now = Some((x, y));
                    // 部品の本来の範囲が先。次にスクロールの棒、最後に部品の当たりの余白
                    if let Some(l) = self.scrollbar_at(x, y).cloned() {
                        self.pressed = None;
                        self.press_scrollbar(&l, y);
                        continue;
                    }
                    // 押せない部品の上では、何も無い所を押したのと同じ（下の部品にも届かない）
                    let hit = self.hit(x, y).cloned().filter(|h| h.enabled);
                    // 題の帯の何も無い所: 2 回押したら最大化を切り替え、1 回なら押したまま動かした時に窓を動かす
                    if hit.is_none() && self.prev.title_bar.is_some_and(|r| r.contains(x, y)) {
                        if count >= 2 {
                            self.csd_request = Some(CsdRequest::ToggleMaximize);
                        } else {
                            self.title_press = Some((x, y));
                        }
                        self.pressed = None;
                        log::event("title_bar.press", &format!("x={x} y={y} count={count}"), "");
                        continue;
                    }
                    // メニューバーの見出しを押した時は、元の焦点を覚える（項目を選んだら戻す）
                    let on_bar = hit.as_ref().is_some_and(|h| self.prev.menubar.contains(&h.id));
                    if on_bar && !self.focus.is_some_and(|f| self.prev.menubar.contains(&f) || self.in_menu(f)) {
                        self.bar_return = self.focus;
                    }
                    self.pressed = hit.as_ref().map(|h| h.id);
                    self.pointer = Some((x, y));
                    self.press_at = Some((x, y));
                    self.column_grab = None;
                    self.focus_visible = false;
                    // 引っ張り出せる部品の上なら、動かした時に引っ張り始める（引っ張りを自分で使う部品の上は除く）
                    if !hit.as_ref().is_some_and(|h| h.drags) {
                        self.press_source = self.source_at(x, y).map(|s| (s.0, x, y));
                    }
                    // 焦点を持てる部品を押した時だけ焦点を移す。何も無い所や、焦点を持たない部品（文字など）を
                    // 押しても、焦点は前の部品に残す（Windows・GTK・Qt の部品と同じ。ウェブのページとは違う）。
                    // 数の入力欄の小さいボタンは、焦点を欄に置く
                    let spin = hit.as_ref().and_then(|h| self.prev.spins.get(&h.id).copied());
                    match spin.or(hit.as_ref().filter(|h| h.focusable).map(|h| h.id)) {
                        Some(f) => self.focus = Some(f),
                        None => {
                            if let Some(f) = self.focus {
                                log::event("focus.kept", &format!("x={x} y={y}"), &format!("target={:x}", f.0));
                            }
                        }
                    }
                    self.press_text = None;
                    if let Some(h) = hit.filter(|h| h.text_input) {
                        self.pointer_text = Some((h.id, x, y, shift, count));
                        self.pointer_text_drag = None;
                        self.press_text = Some(h.id);
                    }
                }
                Event::PointerUp { x, y } => {
                    if self.title_press.take().is_some() {
                        continue;
                    }
                    if let Some(d) = self.drag.take() {
                        let v = self.scrolls.get(&d.list).copied().unwrap_or(0.0);
                        log::event("scrollbar.release", &format!("y={y} moves={}", d.moves), &format!("scroll={v:.0}"));
                        self.pressed = None;
                        continue;
                    }
                    self.press_source = None;
                    // ほかのアプリからの引っ張りは、OS の「落とした」（FileDrop）で終える
                    if let Some(d) = self.dnd.take_if(|d| !d.external) {
                        self.finish_drag(d, x, y);
                        self.pressed = None;
                        continue;
                    }
                    let hit = self.hit(x, y).filter(|h| h.enabled).map(|h| h.id);
                    if hit.is_some() && hit == self.pressed {
                        self.clicked = hit;
                        self.clicked_by_key = false;
                    }
                    if self.pressed.is_some() {
                        self.pointer = Some((x, y));
                    }
                    self.released = self.pressed.take();
                    self.press_text = None;
                }
                Event::PointerMove { x, y } => {
                    // 題の帯を押したまま、引っ張りと同じだけ動いたら窓を動かす（その先は合成が動かす）
                    if let Some((x0, y0)) = self.title_press {
                        if (x - x0).abs().max((y - y0).abs()) > DRAG_START * self.scale {
                            self.title_press = None;
                            self.csd_request = Some(CsdRequest::Move);
                            log::event("title_bar.move", &format!("x={x} y={y}"), "");
                        }
                        continue;
                    }
                    if let Some(d) = self.drag.as_mut() {
                        d.moves += 1;
                        let d = *d;
                        self.drag_scrollbar(d, y);
                    } else if self.dnd.is_some() {
                        self.move_drag(x, y);
                    } else if let Some((src, x0, y0)) =
                        // Windows の SM_CXDRAG・SM_CYDRAG と同じく、横か縦のどちらかが超えたら（押した点からの距離ではない）
                        self.press_source.filter(|&(_, x0, y0)| (x - x0).abs().max((y - y0).abs()) > DRAG_START * self.scale)
                    {
                        self.start_drag(src, x0, y0, x, y);
                        self.move_drag(x, y);
                    } else if self.pressed.is_some() {
                        self.pointer = Some((x, y));
                        // 入力欄を押したまま動かした: 押した所から、ここまでを選ぶ
                        if let Some(t) = self.press_text.filter(|&t| self.pressed == Some(t)) {
                            self.pointer_text_drag = Some((t, x, y));
                        }
                    }
                }
                // マウスのつかみが外れたら、窓の中の引っ張りだけをやめる。ほかのアプリからの引っ張りは、マウスをつかんで
                // いるのがそのアプリなので関係ない（落とす直前に、ボタンの離れたマウスの動きが届くことがある）
                Event::PointerCancel => {
                    self.title_press = None;
                    if !self.dnd.as_ref().is_some_and(|d| d.external) {
                        self.cancel_drag("capture lost");
                    }
                }
                Event::FileDragEnter { x, y, paths } => self.enter_files(x, y, paths),
                Event::FileDragOver { x, y } => {
                    if self.dnd.as_ref().is_some_and(|d| d.external) {
                        self.move_drag(x, y);
                    }
                }
                Event::FileDragLeave => {
                    if self.dnd.as_ref().is_some_and(|d| d.external) {
                        self.cancel_drag("left the window");
                    }
                }
                Event::FileDrop { x, y } => {
                    if let Some(d) = self.dnd.take_if(|d| d.external) {
                        self.finish_drag(d, x, y);
                    }
                }
                // 引っ張っている間の Esc は、引っ張りだけをやめる
                Event::Key { key: Key::Escape, .. } if self.dnd.is_some() => self.cancel_drag("escape"),
                Event::Wheel { x, y, dy } => {
                    // ダイアログの後ろはスクロールしない
                    if self.top_dialog().is_some_and(|d| !d.rect.contains(x, y)) {
                        continue;
                    }
                    if let Some(l) = self.prev.lists.iter().rev().find(|l| l.rect.contains(x, y) && !self.hidden(l.order, x, y)) {
                        let max = ((l.content_h - l.rect.h) / self.scale).max(0.0);
                        let v = self.scrolls.entry(l.id).or_insert(0.0);
                        *v = (*v + dy).clamp(0.0, max);
                    }
                }
                // Esc は一番上の重ねた物を閉じる（ツールチップは消すだけ）
                Event::Key { key: Key::Escape, .. } if !self.prev.overlays.is_empty() => {
                    let o = self.prev.overlays.last().unwrap().clone();
                    if o.kind == OverlayKind::Tooltip {
                        self.tooltip_hidden = o.anchor;
                    } else {
                        self.commit_preedit("escape");
                        log::event("overlay.close", "why=escape", &format!("id={:x} kind={:?}", o.id.0, o.kind));
                        self.closing.insert(o.id);
                    }
                }
                // F10: メニューバーの最初の見出しへ（見出しにいれば元の場所へ戻る）
                Event::Key { key: Key::F10, mods } if mods == Mods::NONE && !self.prev.menubar.is_empty() => {
                    self.commit_preedit("f10");
                    let in_bar = self.focus.is_some_and(|f| self.prev.menubar.contains(&f));
                    for id in self.menu_layers() {
                        self.closing.insert(id);
                    }
                    if in_bar {
                        self.focus = self.bar_return.take();
                    } else {
                        if !self.focus.is_some_and(|f| self.in_menu(f)) {
                            self.bar_return = self.focus;
                        }
                        self.focus = Some(self.prev.menubar[0]);
                        self.focus_visible = true;
                    }
                }
                // Shift+F10 とアプリケーションキー: 焦点のある部品（かそれを含む部品）の右クリックのメニューを、部品の左下に開く
                Event::Key { key: Key::F10, mods: Mods { shift: true, ctrl: false, alt: false } } | Event::Key { key: Key::ContextMenu, .. } => {
                    self.commit_preedit("context");
                    let target = self.focus.and_then(|f| {
                        let r = self.prev.hits.iter().find(|h| h.id == f)?.rect;
                        let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
                        let &(t, _, _) = self.prev.context_regions.iter().rev().find(|&&(id, rr, _)| id == f || rr.contains(cx, cy))?;
                        Some((t, (r.x, r.bottom())))
                    });
                    log::event("context_menu.key", "", &format!("target={:?}", target.map(|t| t.0)));
                    if let Some(req) = target {
                        for id in self.menu_layers() {
                            self.closing.insert(id);
                        }
                        self.context_request = Some(req);
                    }
                }
                // トレイのアイコンを押した・メニューの項目を選んだ（主の窓のビューの Ui::tray が読む）
                Event::Tray(t) => {
                    self.tray_input.push(t);
                }
                // 右クリック: 開いている物の外なら閉じて、その場所の部品の右クリックのメニューを開く
                Event::ContextMenu { x, y } => {
                    self.commit_preedit("context");
                    if let Some(o) = self.top_popup().cloned() {
                        let layers = self.dismiss_layers(&o);
                        if layers.iter().any(|(_, r)| r.contains(x, y)) {
                            continue;
                        }
                        for (id, _) in layers {
                            self.closing.insert(id);
                        }
                    }
                    if self.top_dialog().is_some_and(|d| !d.rect.contains(x, y)) {
                        continue;
                    }
                    // 題の帯の何も無い所は、窓のメニュー（OS が出す）
                    if self.hit(x, y).is_none() && self.prev.title_bar.is_some_and(|r| r.contains(x, y)) {
                        self.csd_request = Some(CsdRequest::Menu { x, y });
                        log::event("title_bar.menu", &format!("x={x} y={y}"), "");
                        continue;
                    }
                    let target = self.prev.context_regions.iter().rev().find(|&&(_, r, o)| r.contains(x, y) && !self.hidden(o, x, y)).map(|&(t, _, _)| t);
                    log::event("context_menu.pointer", &format!("x={x} y={y}"), &format!("target={target:?}"));
                    if let Some(t) = target {
                        self.context_request = Some((t, (x, y)));
                    }
                }
                // Ctrl+Tab は焦点を動かさない（タブの切り替え。アプリへ）
                Event::Key { key: Key::Tab, mods } if !mods.ctrl => {
                    self.commit_preedit("tab");
                    // メニューと選択の一覧は Tab で閉じる（焦点は開いた元へ戻る）
                    if let Some(o) = self.top_popup().filter(|o| matches!(o.kind, OverlayKind::Menu | OverlayKind::ListBox)).cloned() {
                        self.focus_visible = true;
                        for (id, _) in self.dismiss_layers(&o) {
                            self.closing.insert(id);
                        }
                        continue;
                    }
                    self.focus_visible = true;
                    self.tooltip_hidden = None;
                    self.move_focus(mods.shift);
                    self.reveal_focus();
                    // Tab で浮き出しの外へ出たら閉じる
                    if let Some(o) = self.top_popup()
                        && !self.focus.is_some_and(|f| o.focus.contains(&f)) {
                            self.closing.insert(o.id);
                        }
                }
                Event::Key { key: Key::Enter | Key::Space, mods: Mods { ctrl: false, alt: false, .. } }
                    if self.focus.is_some_and(|f| !self.is_text_input(f)) =>
                {
                    self.clicked = self.focus;
                    self.clicked_by_key = true;
                }
                // 古い木を見た読み上げソフトから、押せない部品への操作が届くことがある
                Event::A11y { target, .. } if self.prev.disabled.contains(&target) => {
                    log::event("a11y.disabled", &format!("target={:x}", target.0), "ignored");
                }
                Event::A11y { target, action: A11yAction::Click } => {
                    self.commit_preedit("a11y");
                    self.clicked = Some(target);
                    self.clicked_by_key = true;
                }
                Event::A11y { target, action: A11yAction::Focus } => {
                    self.commit_preedit("a11y");
                    self.focus = Some(target);
                    self.reveal_focus();
                }
                Event::A11y { target, action: a @ (A11yAction::Increment | A11yAction::Decrement) } => {
                    self.commit_preedit("a11y");
                    self.focus = Some(target);
                    // 数の入力欄は上下、スライダーは左右
                    let vertical = self.prev.hits.iter().any(|h| h.id == target && h.vertical);
                    let key = match (a == A11yAction::Increment, vertical) {
                        (true, true) => Key::Up,
                        (false, true) => Key::Down,
                        (true, false) => Key::Right,
                        (false, false) => Key::Left,
                    };
                    self.keys.push(Event::Key { key, mods: Mods::default() });
                }
                Event::ImePreedit(s) => {
                    // 変換が始まった時に入力欄の選択を消すため、入力欄にも届ける
                    if self.preedit.is_empty() && !s.is_empty() && self.focus.is_some_and(|f| self.is_text_input(f)) {
                        self.keys.push(Event::ImePreedit(s.clone()));
                    }
                    self.preedit = s;
                    // 文節の区切りは、新しい変換中の文字のすぐ後に届く。古い区切りは捨てる
                    self.preedit_clauses = None;
                }
                Event::ImeClauses { bounds, target, cursor } => {
                    if !self.preedit.is_empty() {
                        self.preedit_clauses = Some(Clauses { bounds, target, cursor });
                    }
                }
                // キーは、焦点のある部品が使う物ならその部品へ、使わない物はアプリへ（Ui::shortcut）
                Event::Key { key, mods } => {
                    let text = self.focus.is_some_and(|f| self.is_text_input(f));
                    let arrows = self.focus.is_some_and(|f| self.prev.hits.iter().any(|h| h.id == f && h.arrows));
                    let vertical = self.focus.is_some_and(|f| self.prev.hits.iter().any(|h| h.id == f && h.vertical));
                    let lines = vertical && !mods.alt && !mods.ctrl && matches!(key, Key::Up | Key::Down | Key::PageUp | Key::PageDown);
                    if (text && (text_input_takes(key, mods) || lines)) || (arrows && arrows_take(key, mods)) {
                        self.keys.push(Event::Key { key, mods });
                    } else {
                        self.app_keys.push((key, mods));
                    }
                }
                e @ (Event::Char(_) | Event::ImeCommit(_) | Event::ImeSelect { .. }) => {
                    if self.focus.is_some_and(|f| self.is_text_input(f)) {
                        if matches!(e, Event::ImeCommit(_)) {
                            self.preedit.clear();
                        }
                        self.keys.push(e);
                    }
                }
            }
        }
    }

    /// 焦点のある部品が一覧の行の中にあって、行が全部は見えていなければ、見えるように一覧をスクロールする
    fn reveal_focus(&mut self) {
        let Some(f) = self.focus else { return };
        let Some(&(list, row)) = self.prev.rows.get(&f) else { return };
        let Some(l) = self.prev.lists.iter().find(|l| l.id == list).cloned() else { return };
        let now = self.scrolls.get(&list).copied().unwrap_or(0.0);
        let dy = if row.bottom() > l.rect.bottom() {
            row.bottom() - l.rect.bottom()
        } else if row.y < l.rect.y {
            row.y - l.rect.y
        } else {
            return;
        };
        self.set_scroll(&l, now + dy / self.scale);
    }

    /// 変換の途中で焦点が動きそうな時（押す・Tab・読み上げソフトの操作）、変換中の文字を今の入力欄へ確定する。
    /// IME の側の変換は取り消す（IME に確定させると、結果が届く前に焦点が移って別の入力欄に入るため）
    fn commit_preedit(&mut self, why: &str) {
        if self.preedit.is_empty() {
            return;
        }
        let Some(f) = self.focus.filter(|&f| self.is_text_input(f)) else { return };
        let t = std::mem::take(&mut self.preedit);
        self.preedit_clauses = None;
        log::event("ime.commit_on_focus", &format!("why={why} text={t}"), "cancel_ime");
        self.pending_commit = Some((f, t));
        self.ime_cancel = true;
    }

    /// ビューの 1 回目が終わったら、使わなかった入力を捨てる
    pub fn end_input(&mut self) {
        self.pending_commit = None;
        self.clicked = None;
        self.closing.clear();
        self.app_keys.clear();
        self.keys.clear();
        self.pointer_text = None;
        self.released = None;
        self.press_now = None;
        self.context_request = None;
        self.tray_input.clear();
    }
}

/// 1 行の入力欄が使うキー。Alt の組み合わせ・F キー・PageUp/PageDown・Insert・上下の矢印は使わないのでアプリへ。
/// Ctrl は、全部を選ぶ・写す・貼る・切る・やり直す・元に戻すと、単語ごとの移動と消去だけ
fn text_input_takes(key: Key, m: Mods) -> bool {
    use Key::*;
    if m.alt {
        return false;
    }
    if m.ctrl {
        return matches!(key, A | C | V | X | Y | Z | Left | Right | Home | End | Backspace | Delete);
    }
    !matches!(key, F1 | F2 | F3 | F4 | F5 | F6 | F7 | F8 | F9 | F10 | F11 | F12 | PageUp | PageDown | Insert | Escape | Up | Down)
}

/// 矢印で動かす部品（スライダー・選べる一覧の行）が使うキー
fn arrows_take(key: Key, m: Mods) -> bool {
    !m.ctrl && !m.alt && matches!(key, Key::Left | Key::Right | Key::Up | Key::Down | Key::Home | Key::End)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_bar_buttons_follow_the_gnome_button_layout() {
        use CsdButton::*;
        // GNOME の既定（appmenu は kargui には無いので飛ばす）
        assert_eq!(Csd::buttons("appmenu:close"), (vec![], vec![Close]));
        assert_eq!(Csd::buttons(":minimize,maximize,close"), (vec![], vec![Minimize, Maximize, Close]));
        // 左に並べる設定。知らない名前と空白・重なりは飛ばす
        assert_eq!(Csd::buttons("close,minimize:"), (vec![Close, Minimize], vec![]));
        assert_eq!(Csd::buttons(" close , spacer, icon:maximize,maximize"), (vec![Close], vec![Maximize]));
        // 「:」が無ければ全部左（GTK と同じ）
        assert_eq!(Csd::buttons("close"), (vec![Close], vec![]));
        // 設定に無いボタンは出さない（GTK と同じ）。設定を読めなかった時（空）は GNOME の既定と同じく閉じるだけ
        assert_eq!(Csd::buttons("minimize:"), (vec![Minimize], vec![]));
        assert_eq!(Csd::buttons(""), (vec![], vec![Close]));
    }

    #[test]
    fn text_fields_keep_editing_keys_and_pass_the_rest() {
        assert!(text_input_takes(Key::A, Mods::NONE));
        assert!(text_input_takes(Key::A, Mods::SHIFT));
        assert!(text_input_takes(Key::A, Mods::CTRL));
        assert!(text_input_takes(Key::Left, Mods::CTRL | Mods::SHIFT));
        assert!(text_input_takes(Key::Enter, Mods::NONE));
        assert!(!text_input_takes(Key::S, Mods::CTRL));
        assert!(!text_input_takes(Key::A, Mods::ALT));
        assert!(!text_input_takes(Key::F5, Mods::NONE));
        assert!(!text_input_takes(Key::Down, Mods::NONE));
        assert!(arrows_take(Key::Right, Mods::SHIFT));
        assert!(!arrows_take(Key::Right, Mods::CTRL));
        assert!(!arrows_take(Key::Enter, Mods::NONE));
    }
}
