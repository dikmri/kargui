//! fcitx5 の D-Bus（org.fcitx.Fcitx.InputContext1）で日本語入力を受ける。X11 の窓で使う（Wayland は text-input-v3）。
//!
//! GTK・Qt・SDL の入力の部品と同じ道で、キーを IME に渡し（ProcessKeyEvent）、IME が使わなかったキーだけを
//! アプリに渡す。変換中の文字は書式付きの区切り（UpdateFormattedPreedit）、確定の文字は CommitString で届く。
//! D-Bus の呼び出しは待つので、窓の処理は止めない: 頼みは 1 本のスレッドが順に送り、知らせは別のスレッドが
//! 受け取る。どちらも答えを列に積んで eventfd に書き、窓の処理（poll）を起こす。

use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedObjectPath;

use crate::log;

const SERVICE: &str = "org.fcitx.Fcitx5";
const IM_PATH: &str = "/org/freedesktop/portal/inputmethod";
const IM_IFACE: &str = "org.fcitx.Fcitx.InputMethod1";
const IC_IFACE: &str = "org.fcitx.Fcitx.InputContext1";
/// 頼む機能（fcitx5 の CapabilityFlag）: 変換中の文字をアプリが描く（Preedit 1<<1）・書式付き（FormattedPreedit 1<<4）・
/// キーの答えより後に確定と送り返しを届ける（KeyEventOrderFix 1<<37）。焦点が離れた時の確定は fcitx5 に任せる
/// （ClientUnfocusCommit を立てない。Windows の IME と同じく、変換中の文字は確定する）
const CAPS: u64 = (1 << 1) | (1 << 4) | (1 << 37);

/// 窓の処理から IME への頼み
pub(crate) enum Cmd {
    FocusIn,
    FocusOut,
    /// 変換を捨てる（kargui が自分で確定した後）
    Reset,
    /// 文字を打つ位置（X のルート窓の上の物理ピクセル）。候補の窓をその下に出す
    Rect(i32, i32, i32, i32),
    /// キー（keyval は X の keysym、keycode は X のキーの番号、state は X の修飾キーの旗）
    Key { id: u64, keyval: u32, keycode: u32, state: u32, release: bool, time: u32 },
}

/// IME から窓の処理への知らせ
pub(crate) enum Msg {
    /// 入力の文脈ができた
    Ready,
    /// fcitx5 が無い・つなげない（日本語入力なしで続ける）
    Failed(String),
    /// キーを IME が使ったか（使わなければアプリに渡す）
    KeyDone { id: u64, handled: bool },
    Commit(String),
    /// 書式付きの区切り（文字, 書式の旗）と、UTF-8 のバイトのカーソル
    Preedit(Vec<(String, i32)>, i32),
    /// IME が送り返したキー（アプリに渡す）
    Forward { keyval: u32, state: u32, release: bool },
}

/// 入力の文脈を作る（program はアプリの名前、display は「x11:DISPLAY」。fcitx5 は表示ごとに焦点の組を分ける）
fn connect(program: &str, display: &str) -> zbus::Result<Proxy<'static>> {
    let conn = Connection::session()?;
    let im = Proxy::new(&conn, SERVICE, IM_PATH, IM_IFACE)?;
    let args: Vec<(&str, &str)> = vec![("program", program), ("display", display)];
    let (path, _uuid): (OwnedObjectPath, Vec<u8>) = im.call("CreateInputContext", &(args,))?;
    let ic = Proxy::new(&conn, SERVICE, path.as_str().to_string(), IC_IFACE)?;
    ic.call::<_, _, ()>("SetCapability", &(CAPS,))?;
    Ok(ic)
}

/// fcitx5 の入力の文脈を、別のスレッドで作り始める。頼みは返す Sender に送り、知らせは Receiver で受ける
pub(crate) fn spawn(program: String, display: String, wake: Arc<OwnedFd>) -> (Sender<Cmd>, Receiver<Msg>) {
    let (cmd_tx, cmd_rx) = channel::<Cmd>();
    let (msg_tx, msg_rx) = channel::<Msg>();
    let poke = move |tx: &Sender<Msg>, wake: &Arc<OwnedFd>, m: Msg| {
        let _ = tx.send(m);
        let _ = rustix::io::write(&**wake, &1u64.to_ne_bytes());
    };
    let spawned = std::thread::Builder::new().name("kargui-fcitx".into()).spawn(move || {
        let t = std::time::Instant::now();
        let ic = match connect(&program, &display) {
            Ok(ic) => ic,
            Err(e) => {
                poke(&msg_tx, &wake, Msg::Failed(e.to_string()));
                return;
            }
        };
        log::event("fcitx.connect", &format!("display={display} path={}", ic.path()), &format!("ms={:.1}", t.elapsed().as_secs_f64() * 1000.0));
        // 知らせを受けるスレッド（受け取りは止まって待つので、頼みと分ける）
        let (sig, sig_tx, sig_wake) = (ic.clone(), msg_tx.clone(), wake.clone());
        let _ = std::thread::Builder::new().name("kargui-fcitx-signals".into()).spawn(move || {
            let Ok(signals) = sig.receive_all_signals() else {
                log::event("fcitx.signals", "", "error=could not listen");
                return;
            };
            for m in signals {
                let member = m.header().member().map(|n| n.to_string()).unwrap_or_default();
                let body = m.body();
                let msg = match member.as_str() {
                    "CommitString" => body.deserialize::<(String,)>().ok().map(|(s,)| Msg::Commit(s)),
                    "UpdateFormattedPreedit" => body.deserialize::<(Vec<(String, i32)>, i32)>().ok().map(|(segs, cursor)| Msg::Preedit(segs, cursor)),
                    "ForwardKey" => body.deserialize::<(u32, u32, bool)>().ok().map(|(keyval, state, release)| Msg::Forward { keyval, state, release }),
                    _ => None,
                };
                if let Some(msg) = msg {
                    poke(&sig_tx, &sig_wake, msg);
                }
            }
        });
        poke(&msg_tx, &wake, Msg::Ready);
        // 頼みを順に送る（窓の処理が Sender を捨てたら終わる）
        for c in cmd_rx {
            let r = match c {
                Cmd::FocusIn => ic.call::<_, _, ()>("FocusIn", &()),
                Cmd::FocusOut => ic.call::<_, _, ()>("FocusOut", &()),
                Cmd::Reset => ic.call::<_, _, ()>("Reset", &()),
                Cmd::Rect(x, y, w, h) => ic.call::<_, _, ()>("SetCursorRect", &(x, y, w, h)),
                Cmd::Key { id, keyval, keycode, state, release, time } => match ic.call::<_, _, bool>("ProcessKeyEvent", &(keyval, keycode, state, release, time)) {
                    Ok(handled) => {
                        poke(&msg_tx, &wake, Msg::KeyDone { id, handled });
                        Ok(())
                    }
                    Err(e) => {
                        // 答えが無ければ、アプリに渡す（キーを失わない）
                        poke(&msg_tx, &wake, Msg::KeyDone { id, handled: false });
                        Err(e)
                    }
                },
            };
            if let Err(e) = r {
                log::event("fcitx.call", "", &format!("error={e}"));
            }
        }
        let _ = ic.call::<_, _, ()>("DestroyIC", &());
    });
    if let Err(e) = spawned {
        log::event("fcitx.spawn", "", &format!("error={e}"));
    }
    (cmd_tx, msg_rx)
}
