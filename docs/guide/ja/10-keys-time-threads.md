<!-- covers: Ui::shortcut Ui::now Ui::request_frame_after Ui::waker Waker::wake Harness::hover Harness::wait sig=1dc37a39551057c3 -->
# キー・時間・ほかのスレッド

kargui は、何も変わらない間は眠っていて、CPU を使いません。入力があった時のほかに、キーを受け取った時、決めた時間がたった時、ほかのスレッドが起こした時にも、`view` を回します。

## ショートカット

部品が使わなかったキーは、`ui.shortcut(キー, 修飾キー)` で受け取ります。修飾キーは `Mods::CTRL`・`Mods::SHIFT`・`Mods::ALT`・`Mods::NONE` を `|` で組み合わせます。

```rust
use kargui::{Key, Mods};
# let (mut saved, mut saved_as) = (0, 0);
# kargui::test::example(|ui| {
if ui.shortcut(Key::S, Mods::CTRL) {
    saved += 1;
}
if ui.shortcut(Key::S, Mods::CTRL | Mods::SHIFT) {
    saved_as += 1;
}
# });
```

焦点のある部品が使うキーは、その部品へ先に渡します。入力欄は文字・数字・Enter・消すキー・Ctrl+A/C/V/X/Y/Z を、スライダーと選べる一覧は矢印を使います。それ以外（Ctrl+S、F5、閉じる物が無い時の Esc など）がアプリに届きます。`ui.enabled(false, ..)` の中では、いつも false です。

## 時間がたったら描き直す

`ui.now()` はこのフレームの時刻です。`ui.request_frame_after(時間)` は、その時間がたったら `view` を回すように頼みます。それまでは描かずに眠ります。

```rust
use std::time::Duration;
# let mut saved_at: Option<std::time::Instant> = None;
# kargui::test::example(|ui| {
// 「保存しました」を 3 秒だけ出す
if let Some(at) = saved_at {
    let shown = ui.now().saturating_duration_since(at);
    if shown < Duration::from_secs(3) {
        ui.label("保存しました");
        ui.request_frame_after(Duration::from_secs(3) - shown);
    }
}
# });
```

アニメーションのように毎フレーム描く時は、`ui.request_frame()` を使います。

## ほかのスレッドから描き直す

時間のかかる仕事（読み込み・計算・通信）は、ほかのスレッドで行います。結果はチャンネルで送り、`ui.waker()` で受け取った起こし役の `wake()` を呼びます。窓が起きて `view` が回るので、`view` の中でチャンネルから読みます。

```rust
use std::sync::mpsc::{Receiver, channel};
use kargui::{App, Ui, Waker};

struct Loader {
    rx: Receiver<String>,
    waker: Option<Waker>,
    lines: Vec<String>,
}

impl App for Loader {
    fn view(&mut self, ui: &mut Ui) {
        self.waker.get_or_insert_with(|| ui.waker());
        self.lines.extend(self.rx.try_iter());
        for line in &self.lines {
            ui.label(line);
        }
    }
}

let (tx, rx) = channel();
let mut h = kargui::test::Harness::new(Loader { rx, waker: None, lines: Vec::new() });
let waker = h.app().waker.clone().unwrap();
std::thread::spawn(move || {
    tx.send("読み込みました".to_string()).unwrap();
    waker.wake();
})
.join()
.unwrap();
h.frame();
assert!(h.find("読み込みました").is_some());
```

起こし役はどのスレッドからでも、何回呼んでもかまいません。窓が読む前の起こしは 1 回にまとめます。

## 試験では時間を進める

試験台は本当には待ちません。`h.wait(時間)` で時間を進め、`h.hover("名前")` でポインターを部品に乗せます。

```rust
use std::time::Duration;
use kargui::{App, Ui};

struct Bar;

impl App for Bar {
    fn view(&mut self, ui: &mut Ui) {
        ui.button("保存").tooltip("今の内容を保存します");
    }
}

let mut h = kargui::test::Harness::new(Bar);
h.hover("保存");
assert!(h.find("今の内容を保存します").is_none());
h.wait(Duration::from_millis(600));
assert!(h.find("今の内容を保存します").is_some());
```
