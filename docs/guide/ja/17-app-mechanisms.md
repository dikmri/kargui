<!-- covers: Ui::file_dialog Ui::chosen_files Ui::chosen_file Ui::file_dialog_cancelled FileDialog::open FileDialog::save FileDialog::multiple FileDialog::title FileDialog::filter FileDialog::file_name FileDialog::folder FileDialog::is_save FileDialog::allows_multiple App::close_requested Ui::quit Harness::file_dialog_key Harness::file_dialog Harness::choose_files Harness::cancel_file_dialog Harness::request_close Harness::is_closed sig=7e9d34a5ac6cda9a -->
# アプリの仕組み

## ファイルを開く・保存するダイアログ

システムのファイルのダイアログを出します。

```rust
use kargui::FileDialog;
# let mut text = String::new();
# kargui::test::example(|ui| {
if ui.button("開く").clicked() {
    ui.file_dialog("open", FileDialog::open().title("メモを開く").filter("テキスト", &["txt", "md"]));
}
if let Some(path) = ui.chosen_file("open") {
    text = std::fs::read_to_string(path).unwrap_or_default();
}
if ui.button("名前を付けて保存").clicked() {
    ui.file_dialog("save", FileDialog::save().file_name("メモ.txt").filter("テキスト", &["txt"]));
}
if let Some(path) = ui.chosen_file("save") {
    let _ = std::fs::write(path, &text);
}
# });
```

`ui.file_dialog(キー, ..)` で頼むと、そのフレームを描いた後でダイアログを出します。選んだファイルは、後のフレームで `ui.chosen_file(キー)` から 1 回だけ受け取れます。利用者が取り消した時は、次のフレームで `ui.file_dialog_cancelled(キー)` が 1 回だけ true を返します。ダイアログの間も、アプリは止まらずに描き続けます。

- `FileDialog::open()` は開く、`FileDialog::save()` は保存する（同じ名前のファイルがあれば、置き換えてよいかを聞く）
- `.filter(名前, &[拡張子])` で選べる種類、`.title(..)` で題、`.file_name(..)` で始めの名前、`.folder(..)` で始めのフォルダー
- `.multiple()` で複数のファイルを選べるようにし、`ui.chosen_files(キー)` で全部を受け取る

Linux では、xdg-desktop-portal のダイアログ（デスクトップのファイルを選ぶ画面）が開きます。macOS では、まだ何も起きません。

## 閉じる前の確認

保存していない変更がある時に、閉じる前に聞けます。

```rust
use kargui::{App, Ui};

#[derive(Default)]
struct Notes {
    unsaved: bool,
    asking: bool,
}

impl App for Notes {
    fn view(&mut self, ui: &mut Ui) {
        let mut discard = false;
        ui.dialog(&mut self.asking, "変更を捨てますか？", |ui| {
            discard = ui.button("捨てて閉じる").clicked();
        });
        if discard {
            ui.quit();
        }
    }

    fn close_requested(&mut self) -> bool {
        self.asking = self.unsaved;
        !self.unsaved
    }
}
# let mut h = kargui::test::Harness::new(Notes { unsaved: true, asking: false });
# h.request_close();
# assert!(!h.is_closed());
```

利用者が窓の閉じるボタン（か Alt+F4）を押すと、アプリの `close_requested` が呼ばれます。true を返すと閉じ、false を返すと閉じません。聞いた後で閉じる時は `ui.quit()` を呼びます。`ui.quit()` は、どこからでもアプリを終えます。

## 窓のアイコンと最小の大きさ

```rust
use kargui::{Image, Options};
# let png = Image::from_rgba(2, 2, vec![255; 16]).unwrap();
let mut options = Options::new("メモ");
options.icon = Some(png);
options.min_size = Some((360.0, 240.0));
# let _ = options;
```

`icon` は題の帯とタスクバーのアイコンです。ほかの窓は、自分のアイコンが無ければ主の窓のアイコンを使います。`min_size` は、利用者が小さくできる中の大きさの下限（論理ピクセル）です。

`icon` を渡さない時は、kargui のアイコン（濃い紺の角丸の四角に、光の線で描いた k）を出します。kargui はこのアイコンを画像のファイルから読まず、要る大きさごとに描きます。OS の無地のアイコンにしたい時は、`options.default_icon = false` にします。Wayland では、窓のアイコンを合成がアプリのデスクトップ ファイルから決めるので、変わるのはトレイのアイコンだけです。

## 題の帯に部品を置く

`options.custom_title_bar = true` の窓では、Windows でも kargui が題の帯を描きます。ビューの `ui.title_bar` で、帯の中にメニュー・検索・タブなどを置けます。

```rust
use kargui::{App, Options, Ui};

struct Notes {
    search: String,
}

impl App for Notes {
    fn view(&mut self, ui: &mut Ui) {
        ui.title_bar(|ui| {
            ui.label("メモ").bold();
            ui.text_input(&mut self.search).hint("探す");
        });
        ui.label("本文");
    }
}

let mut options = Options::new("メモ");
options.custom_title_bar = true;
# let _ = (options, Notes { search: String::new() });
```

- kargui は、帯の右端に最小化・最大化・閉じるのボタンを置きます。Windows 11 では、最大化のボタンに乗るとスナップレイアウト（窓の並べ方の候補）が出ます。
- 帯の何も無い所（押せない文字の上を含む）は、OS の題の帯と同じく、引っ張ると窓が動き、2 回押すと最大化し、右クリックで窓のメニューが出ます。画面の端へ寄せる操作も使えます。
- `ui.title_bar` に何も入れなければ、帯の左に窓の題を出します。入れた時は題を出さないので、出したい時はアプリが `ui.label` で置いてください。
- GNOME（合成が題の帯を描かない Wayland）では、`custom_title_bar` にかかわらず kargui が帯を描き、`ui.title_bar` の部品を帯に入れます。
- OS や合成が帯を描く所（X11、帯を描く Wayland、帯の無い合成）と、`custom_title_bar` が false の Windows では、`ui.title_bar` の部品は窓のいちばん上の行になります。どの OS でも使えるように、`ui.title_bar` はビューの最初に呼んでください。

## トレイ（通知領域のアイコン）

主の窓のビューで `ui.tray` を呼んでいる間、通知領域にアプリのアイコンを出します。アイコンには、ポインターを乗せた時の説明、押した時の知らせ、右クリックで開くメニューがあります。メニューは OS が描くので、項目・印の付く項目・区切り・入れ子のメニューを置けます。

```rust
use kargui::{App, Ui};

#[derive(Default)]
struct Notes {
    muted: bool,
    hide: bool,
}

impl App for Notes {
    fn view(&mut self, ui: &mut Ui) {
        let (mut show, mut quit) = (false, false);
        ui.tray(|t| {
            t.tooltip("メモ");
            show = t.clicked();
            t.menu(|m| {
                show |= m.item("窓を出す").clicked();
                m.check(&mut self.muted, "音を消す");
                m.separator();
                quit = m.item("終わる").clicked();
            });
        });
        if std::mem::take(&mut self.hide) {
            ui.hide_window();
        }
        if show {
            ui.show_window();
        }
        if quit {
            ui.quit();
        }
        ui.label("本文");
    }

    // 閉じるボタンでは終わらず、トレイに隠れる
    fn close_requested(&mut self) -> bool {
        self.hide = true;
        false
    }
}
# let _ = Notes::default();
```

- `ui` は `ui.tray` の中で使えないので、押した・選んだことを変数に覚え、`ui.tray` の後で窓を出す・アプリを終えるなどを行います。
- `t.clicked()` と `m.item(..).clicked()` は、押した・選んだフレームでだけ true になります。`m.check` は、選ぶと値を入れ替えます。
- アイコンの画像は `t.icon(&image)` で渡します。渡さなければ窓のアイコン（`Options::icon`）を使い、それも無ければ kargui の無地のアイコンを使います。画像はアプリの状態に持っておいてください。同じ画像なら OS に送り直しません。
- `ui.tray` を呼ばなくなると、アイコンを消します。トレイを作るのは主の窓のビューだけです（`App::window` のビューで呼んでも何もしません）。
- `ui.hide_window()` は窓を画面とタスクバーから隠し、`ui.show_window()` は隠した窓・最小化した窓を戻して前に出します。どちらも、出来事（トレイを押した・閉じた）の時だけ呼んでください。毎フレーム `ui.show_window()` を呼ぶと、利用者が最小化してもすぐ戻ります。
- Windows は、タスクバーの通知領域に出します。Linux は StatusNotifierItem の決まりで出し、KDE、タイル型の合成の帯（Waybar・Quickshell など）、AppIndicator の拡張機能を入れた GNOME が表示します。拡張機能の無い GNOME には出ません。
- Wayland には、最小化した窓を戻す頼みがありません。`ui.show_window()` が戻せるのは、`ui.hide_window()` で隠した窓だけです。

## 通知

`ui.notify` で通知を出します。Windows はトースト（通知センターにも残る）、Linux はデスクトップの通知の係（org.freedesktop.Notifications）に出します。通知には、題・本文・ボタン（3 つまで）を置けます。押された答えは、同じキーで `ui.notification_clicked` から受け取ります。

```rust
use kargui::{App, Notification, NotificationAction, Ui};

#[derive(Default)]
struct Timer {
    snoozed: u32,
}

impl App for Timer {
    fn view(&mut self, ui: &mut Ui) {
        if ui.button("時間を知らせる").clicked() {
            ui.notify("time", Notification::new("時間です").body("会議が始まります").button("snooze", "5 分後"));
        }
        match ui.notification_clicked("time") {
            Some(NotificationAction::Button(id)) if id == "snooze" => self.snoozed += 1,
            Some(_) => ui.show_window(),
            None => {}
        }
    }
}
# let _ = Timer::default();
```

- `ui.notify` は、呼んだ時に 1 回出す命令です。ボタンを押した時など、出来事の時に呼んでください。同じキーで出し直すと、前の通知を置き換えます。
- `ui.notification_clicked` は、通知の本文（`NotificationAction::Body`）かボタン（`NotificationAction::Button(id)`）が押された時に、1 回だけ答えを返します。押さずに閉じた時は何も返しません。
- `Notification::silent()` で、音を鳴らさずに出します。
- Windows では、最初の通知の時に、アプリの名前（`Options::title`）を利用者ごとのレジストリ（`HKEY_CURRENT_USER\Software\Classes\AppUserModelId\実行ファイルの名前`）に書いて登録します。トーストの上には、この名前が出ます。押した答えは、アプリが動いている間だけ届きます。アプリが終わった後に、通知センターに残った通知を押しても何も起きません。
- Linux では、GNOME・KDE・mako・dunst など、org.freedesktop.Notifications を持つデスクトップに出ます。ボタンを出すかどうかは、通知の係によります。

## ハイコントラスト

OS がハイコントラストの時、kargui は部品をハイコントラストの配色で描きます。OS の設定は、Windows では「設定 > アクセシビリティ > コントラスト テーマ」、Linux ではデスクトップのポータルのコントラストの設定（GNOME の「ハイコントラスト」など）です。Windows では、OS のテーマの色（黒地に水色の強調、白地に紫の強調など）をそのまま使います。Linux では、明暗に合わせて kargui の配色（`Theme::high_contrast_dark`・`Theme::high_contrast_light`）を使います。

```rust
use kargui::Options;

let mut options = Options::new("メモ");
// いつもハイコントラストで描く（None は OS に従う。Some(false) は使わない）
options.high_contrast = Some(true);
# let _ = options;
```

- `Options::high_contrast` の既定は `None` で、OS の設定に従います。`Some(true)` はいつもハイコントラスト、`Some(false)` は使いません。
- Windows では、アプリを動かしたままコントラスト テーマを切り替えても、開いている窓をすぐに描き直します。Linux では、起動の時に読みます。
- ハイコントラストの間は、影と半透明の塗りを描かず、境を縁で見せます。選んだ行には強調の色の縁を付け、選んだ文字は強調の上の文字の色で描き直します。押せない部品とヒントは、GrayText（暗い配色では緑）で描きます。
- `canvas` で絵を描く時は、`ui.high_contrast()` を見て、`ui.theme()` の色だけで描いてください。自分で決めた色や、薄い色を使わないでください。

```rust
# kargui::test::example(|ui| {
let line = if ui.high_contrast() { ui.theme().text } else { ui.theme().border };
# let _ = line;
# });
```

## 試験

試験台では、`h.file_dialog_key()` と `h.file_dialog()` で頼まれたダイアログを読み、`h.choose_files(&[..])` か `h.cancel_file_dialog()` で答えます。`h.request_close()` は閉じるボタンを押したことにし、`h.is_closed()` で閉じたかを確かめます。トレイは、`h.tray_shown()`・`h.tray_tooltip()`・`h.tray_menu()`（1 行ずつの文字）で読み、`h.tray_click()` で押し、`h.tray_choose("終わる")` で項目を選びます。窓が隠れているかは `h.window_shown()` で確かめます。通知は、`h.notifications()`（1 行ずつの文字）で読み、`h.click_notification("time", Some("snooze"))` でボタンを、`None` で本文を押します。`h.set_high_contrast(true)` は、OS のハイコントラストを入れたことにします。
