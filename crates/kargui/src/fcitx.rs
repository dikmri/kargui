//! fcitx5 の D-Bus（org.fcitx.Fcitx.InputContext1）で日本語入力を受ける。X11 の窓で使う（ime_dbus.rs が選ぶ）。
//!
//! GTK・Qt・SDL の入力の部品と同じ道で、キーを IME に渡し（ProcessKeyEvent）、IME が使わなかったキーだけを
//! アプリに渡す。変換中の文字は書式付きの区切り（UpdateFormattedPreedit）、確定の文字は CommitString で届く。

use std::sync::mpsc::Receiver;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedObjectPath;

use crate::ime;
use crate::ime_dbus::{Cmd, Engine, Msg, Poke};
use crate::log;

const SERVICE: &str = "org.fcitx.Fcitx5";
const IM_PATH: &str = "/org/freedesktop/portal/inputmethod";
const IM_IFACE: &str = "org.fcitx.Fcitx.InputMethod1";
const IC_IFACE: &str = "org.fcitx.Fcitx.InputContext1";
/// 頼む機能（fcitx5 の CapabilityFlag）: 変換中の文字をアプリが描く（Preedit 1<<1）・書式付き（FormattedPreedit 1<<4）・
/// キーの答えより後に確定と送り返しを届ける（KeyEventOrderFix 1<<37）。焦点が離れた時の確定は fcitx5 に任せる
/// （ClientUnfocusCommit を立てない。Windows の IME と同じく、変換中の文字は確定する）
const CAPS: u64 = (1 << 1) | (1 << 4) | (1 << 37);

/// 入力の文脈を作る（program はアプリの名前、display は「x11:DISPLAY」。fcitx5 は表示ごとに焦点の組を分ける）
pub(crate) fn connect(conn: &Connection, program: &str, display: &str) -> zbus::Result<Proxy<'static>> {
    let im = Proxy::new(conn, SERVICE, IM_PATH, IM_IFACE)?;
    let args: Vec<(&str, &str)> = vec![("program", program), ("display", display)];
    let (path, _uuid): (OwnedObjectPath, Vec<u8>) = im.call("CreateInputContext", &(args,))?;
    let ic = Proxy::new(conn, SERVICE, path.as_str().to_string(), IC_IFACE)?;
    ic.call::<_, _, ()>("SetCapability", &(CAPS,))?;
    Ok(ic)
}

/// 知らせを受けるスレッドを立て、頼みを順に送る（窓の処理が Sender を捨てたら終わる）
pub(crate) fn run(ic: Proxy<'static>, cmds: Receiver<Cmd>, poke: Poke) {
    // 知らせの登録は「できた」より前に済ませる（登録の前に届いた知らせは受け取れない）
    match ic.receive_all_signals() {
        Ok(signals) => {
            let p = poke.clone();
            let _ = std::thread::Builder::new().name("kargui-fcitx-signals".into()).spawn(move || {
                for m in signals {
                    let member = m.header().member().map(|n| n.to_string()).unwrap_or_default();
                    let body = m.body();
                    let msg = match member.as_str() {
                        "CommitString" => body.deserialize::<(String,)>().ok().map(|(s,)| Msg::Commit(s)),
                        "UpdateFormattedPreedit" => body.deserialize::<(Vec<(String, i32)>, i32)>().ok().map(|(segs, cursor)| {
                            let (text, clauses) = ime::fcitx_clauses(&segs, cursor);
                            Msg::Preedit(text, clauses)
                        }),
                        "ForwardKey" => body.deserialize::<(u32, u32, bool)>().ok().map(|(keyval, state, release)| Msg::Forward { keyval, state, release }),
                        _ => None,
                    };
                    if let Some(msg) = msg {
                        p.send(msg);
                    }
                }
            });
        }
        Err(e) => log::event("fcitx.signals", "", &format!("error={e}")),
    }
    poke.send(Msg::Ready(Engine::Fcitx5));
    for c in cmds {
        let r = match c {
            Cmd::FocusIn => ic.call::<_, _, ()>("FocusIn", &()),
            Cmd::FocusOut => ic.call::<_, _, ()>("FocusOut", &()),
            Cmd::Reset => ic.call::<_, _, ()>("Reset", &()),
            Cmd::Rect(x, y, w, h) => ic.call::<_, _, ()>("SetCursorRect", &(x, y, w, h)),
            Cmd::Key { id, keyval, keycode, state, release, time } => match ic.call::<_, _, bool>("ProcessKeyEvent", &(keyval, keycode, state, release, time)) {
                Ok(handled) => {
                    poke.send(Msg::KeyDone { id, handled });
                    Ok(())
                }
                Err(e) => {
                    // 答えが無ければ、アプリに渡す（キーを失わない）
                    poke.send(Msg::KeyDone { id, handled: false });
                    Err(e)
                }
            },
        };
        if let Err(e) = r {
            log::event("fcitx.call", "", &format!("error={e}"));
        }
    }
    let _ = ic.call::<_, _, ()>("DestroyIC", &());
}
