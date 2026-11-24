//! 日本語入力の D-Bus（fcitx5 と IBus）の共通の部分。X11 の窓で使う（Wayland は text-input-v3）。
//!
//! 窓の処理（x11.rs）から見ると、IME はどちらでも同じ: キーをまず IME に渡し（Cmd::Key）、IME が使わなかった
//! キーだけをアプリに渡す。確定の文字・変換中の文字・IME が送り返したキーは Msg で届く。D-Bus の呼び出しは
//! 待つので、窓の処理は止めない: 頼みは 1 本のスレッドが順に送り、知らせは別のスレッドが受け取る。どちらも
//! 答えを列に積んで eventfd に書き、窓の処理（poll）を起こす。
//!
//! どちらを使うかは、会話のバスに今いる物で決める（D-Bus の起動で IME を立ち上げない）。両方いれば fcitx5
//! （XMODIFIERS が @im=ibus なら IBus）。fcitx5 は IBus のふりもする（org.freedesktop.IBus の名前を持つ）ので、
//! その名前の持ち主が fcitx5 と同じなら IBus とは数えない。

use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, channel};

use zbus::blocking::{Connection, Proxy};

use crate::ime::Clauses;
use crate::{fcitx, ibus, log};

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
    Ready(Engine),
    /// IME が無い・つなげない（日本語入力なしで続ける）
    Failed(String),
    /// キーを IME が使ったか（使わなければアプリに渡す）
    KeyDone { id: u64, handled: bool },
    Commit(String),
    /// 変換中の文字と文節（文字が空なら、変換中の文字が無くなった）
    Preedit(String, Clauses),
    /// IME が送り返したキー（アプリに渡す）
    Forward { keyval: u32, state: u32, release: bool },
}

/// 日本語入力の仕組み
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Engine {
    Fcitx5,
    IBus,
}

impl Engine {
    pub fn name(self) -> &'static str {
        match self {
            Engine::Fcitx5 => "fcitx5",
            Engine::IBus => "ibus",
        }
    }
}

/// 会話のバスで fcitx5 と ibus-daemon が持つ名前
const FCITX_NAME: &str = "org.fcitx.Fcitx5";
const IBUS_NAME: &str = "org.freedesktop.IBus";

/// 試す順番。fcitx_owner と ibus_owner は、会話のバスでその名前を持つ物（一意の名前。いなければ None）。
/// xmodifiers は環境変数 XMODIFIERS（デスクトップが X の入力の仕組みを知らせる）
pub(crate) fn engine_order(fcitx_owner: Option<&str>, ibus_owner: Option<&str>, xmodifiers: &str) -> Vec<Engine> {
    let fcitx = fcitx_owner.is_some();
    // fcitx5 が IBus のふりをしている（同じ持ち主）なら、本物の IBus はいない
    let ibus = ibus_owner.is_some() && ibus_owner != fcitx_owner;
    let ibus_first = xmodifiers.to_ascii_lowercase().contains("@im=ibus");
    let mut order = Vec::new();
    if ibus && ibus_first {
        order.push(Engine::IBus);
    }
    if fcitx {
        order.push(Engine::Fcitx5);
    }
    if ibus && !ibus_first {
        order.push(Engine::IBus);
    }
    order
}

/// 知らせを積んで、窓の処理を起こす
#[derive(Clone)]
pub(crate) struct Poke {
    tx: Sender<Msg>,
    wake: Arc<OwnedFd>,
}

impl Poke {
    pub fn send(&self, m: Msg) {
        let _ = self.tx.send(m);
        let _ = rustix::io::write(&*self.wake, &1u64.to_ne_bytes());
    }
}

/// 会話のバスで name を持つ物（一意の名前）
fn owner(conn: &Connection, name: &str) -> Option<String> {
    let bus = Proxy::new(conn, "org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus").ok()?;
    bus.call::<_, _, String>("GetNameOwner", &(name,)).ok()
}

/// IME の入力の文脈を、別のスレッドで作り始める（program はアプリの名前、display は「x11:DISPLAY」）。
/// 頼みは返す Sender に送り、知らせは Receiver で受ける
pub(crate) fn spawn(program: String, display: String, wake: Arc<OwnedFd>) -> (Sender<Cmd>, Receiver<Msg>) {
    let (cmd_tx, cmd_rx) = channel::<Cmd>();
    let (msg_tx, msg_rx) = channel::<Msg>();
    let poke = Poke { tx: msg_tx, wake };
    let spawned = std::thread::Builder::new().name("kargui-ime".into()).spawn(move || {
        let t = std::time::Instant::now();
        let conn = match Connection::session() {
            Ok(c) => c,
            Err(e) => return poke.send(Msg::Failed(format!("session bus: {e}"))),
        };
        let (f, i) = (owner(&conn, FCITX_NAME), owner(&conn, IBUS_NAME));
        let xmodifiers = std::env::var("XMODIFIERS").unwrap_or_default();
        let order = engine_order(f.as_deref(), i.as_deref(), &xmodifiers);
        log::event("ime.choose", &format!("fcitx5={f:?} ibus={i:?} XMODIFIERS={xmodifiers:?}"), &format!("order={order:?}"));
        let mut errors = Vec::new();
        for engine in order {
            let r = match engine {
                Engine::Fcitx5 => fcitx::connect(&conn, &program, &display),
                Engine::IBus => ibus::connect(&conn, &program),
            };
            match r {
                Ok(ic) => {
                    log::event("ime.connect", &format!("engine={} display={display} path={}", engine.name(), ic.path()), &format!("ms={:.1}", t.elapsed().as_secs_f64() * 1000.0));
                    match engine {
                        Engine::Fcitx5 => fcitx::run(ic, cmd_rx, poke),
                        Engine::IBus => ibus::run(ic, cmd_rx, poke),
                    }
                    return;
                }
                Err(e) => {
                    log::event("ime.connect", &format!("engine={}", engine.name()), &format!("error={e}"));
                    errors.push(format!("{}: {e}", engine.name()));
                }
            }
        }
        let why = if errors.is_empty() { "neither fcitx5 nor IBus is running".to_string() } else { errors.join("; ") };
        poke.send(Msg::Failed(why));
    });
    if let Err(e) = spawned {
        log::event("ime.spawn", "", &format!("error={e}"));
    }
    (cmd_tx, msg_rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_order_prefers_fcitx5_and_skips_its_ibus_stand_in() {
        // fcitx5 だけ（fcitx5 は IBus の名前も持つ。検証機で見た形）
        assert_eq!(engine_order(Some(":1.5"), Some(":1.5"), "@im=fcitx"), vec![Engine::Fcitx5]);
        // 本物の IBus だけ
        assert_eq!(engine_order(None, Some(":1.9"), "@im=ibus"), vec![Engine::IBus]);
        // 両方いれば fcitx5 が先。XMODIFIERS が IBus を指していれば IBus が先
        assert_eq!(engine_order(Some(":1.5"), Some(":1.9"), ""), vec![Engine::Fcitx5, Engine::IBus]);
        assert_eq!(engine_order(Some(":1.5"), Some(":1.9"), "@im=ibus"), vec![Engine::IBus, Engine::Fcitx5]);
        // どちらもいない（D-Bus の起動で立ち上げない）
        assert_eq!(engine_order(None, None, "@im=ibus"), Vec::<Engine>::new());
    }
}
