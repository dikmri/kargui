//! IBus の D-Bus で日本語入力を受ける。X11 の窓で、fcitx5 がいない時に使う（ime_dbus.rs が選ぶ）。
//!
//! ibus-daemon は自分専用のバスで話すが、会話のバスのポータル（org.freedesktop.portal.IBus、ibus-portal）が
//! 入力の文脈を取り次ぐ。GTK の IBus の部品と同じく、キーを渡し（ProcessKeyEvent）、IME が使わなかったキーだけを
//! アプリに渡す。変換中の文字は属性付きの文字（IBusText）で届き、位置は文字の数。確定の文字（CommitText）は
//! キーの答えより先に届く。候補の窓は IBus の画面の部品（ibus-ui-gtk3）が出すので、文字を打つ位置
//! （SetCursorLocation）だけを知らせる。

use std::sync::mpsc::Receiver;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use crate::ime;
use crate::ime_dbus::{Cmd, Engine, Msg, Poke};
use crate::log;

const PORTAL: &str = "org.freedesktop.portal.IBus";
const PORTAL_PATH: &str = "/org/freedesktop/IBus";
const PORTAL_IFACE: &str = "org.freedesktop.IBus.Portal";
const IC_IFACE: &str = "org.freedesktop.IBus.InputContext";
/// 頼む機能（IBusCapabilite）: 変換中の文字をアプリが描く（PREEDIT_TEXT 1）・焦点を知らせる（FOCUS 8）。
/// 候補の一覧（LOOKUP_TABLE）は頼まない（IBus の画面の部品が出す）
const CAPS: u32 = 1 | 8;
/// 修飾キーの旗の「離した」（IBUS_RELEASE_MASK）
const RELEASE: u32 = 1 << 30;

/// ポータルに入力の文脈を作ってもらう（program はアプリの名前）
pub(crate) fn connect(conn: &Connection, program: &str) -> zbus::Result<Proxy<'static>> {
    let portal = Proxy::new(conn, PORTAL, PORTAL_PATH, PORTAL_IFACE)?;
    let path: OwnedObjectPath = portal.call("CreateInputContext", &(program,))?;
    let ic = Proxy::new(conn, PORTAL, path.as_str().to_string(), IC_IFACE)?;
    ic.call::<_, _, ()>("SetCapabilities", &(CAPS,))?;
    Ok(ic)
}

/// 中に入れ子になった値（v の中の v）を外す
fn inner<'b, 'a>(v: &'b Value<'a>) -> &'b Value<'a> {
    match v {
        Value::Value(b) => inner(b),
        v => v,
    }
}

/// IBusText（("IBusText", a{sv}, 文字, v 属性の並び)）から、文字と属性 [種類, 値, 始まり, 終わり] を取り出す。
/// 属性の並びは ("IBusAttrList", a{sv}, av)、属性は ("IBusAttribute", a{sv}, u 種類, u 値, u 始まり, u 終わり)
pub(crate) fn ibus_text(v: &Value<'_>) -> Option<(String, Vec<[u32; 4]>)> {
    let Value::Structure(s) = inner(v) else { return None };
    let f = s.fields();
    let (Some(Value::Str(name)), Some(Value::Str(text))) = (f.first().map(inner), f.get(2).map(inner)) else { return None };
    if name.as_str() != "IBusText" {
        return None;
    }
    let mut attrs = Vec::new();
    if let Some(Value::Structure(list)) = f.get(3).map(inner)
        && let Some(Value::Array(items)) = list.fields().get(2).map(inner)
    {
        for item in items.inner() {
            let Value::Structure(a) = inner(item) else { continue };
            let u = |k: usize| match a.fields().get(k).map(inner) {
                Some(Value::U32(x)) => Some(*x),
                _ => None,
            };
            if let (Some(kind), Some(value), Some(start), Some(end)) = (u(2), u(3), u(4), u(5)) {
                attrs.push([kind, value, start, end]);
            }
        }
    }
    Some((text.to_string(), attrs))
}

/// 今の変換中の文字（IBus は文字と、見せるかどうかを別々に知らせる）
#[derive(Default)]
struct Preedit {
    text: String,
    attrs: Vec<[u32; 4]>,
    cursor: u32,
    visible: bool,
}

impl Preedit {
    fn msg(&self) -> Msg {
        let text = if self.visible { self.text.clone() } else { String::new() };
        let clauses = ime::ibus_clauses(&text, &self.attrs, self.cursor);
        Msg::Preedit(text, clauses)
    }
}

/// 知らせを受けるスレッドを立て、頼みを順に送る（窓の処理が Sender を捨てたら終わる）
pub(crate) fn run(ic: Proxy<'static>, cmds: Receiver<Cmd>, poke: Poke) {
    // 知らせの登録は「できた」より前に済ませる（登録の前に届いた知らせは受け取れない）
    match ic.receive_all_signals() {
        Ok(signals) => {
            let p = poke.clone();
            let _ = std::thread::Builder::new().name("kargui-ibus-signals".into()).spawn(move || {
                let mut pre = Preedit::default();
                for m in signals {
                    let member = m.header().member().map(|n| n.to_string()).unwrap_or_default();
                    let body = m.body();
                    let msg = match member.as_str() {
                        "CommitText" => body.deserialize::<(OwnedValue,)>().ok().and_then(|(v,)| ibus_text(&v)).map(|(text, _)| Msg::Commit(text)),
                        "UpdatePreeditText" => body.deserialize::<(OwnedValue, u32, bool)>().ok().and_then(|(v, cursor, visible)| {
                            let (text, attrs) = ibus_text(&v)?;
                            pre = Preedit { text, attrs, cursor, visible };
                            Some(pre.msg())
                        }),
                        "ShowPreeditText" => {
                            pre.visible = true;
                            Some(pre.msg())
                        }
                        "HidePreeditText" => {
                            pre.visible = false;
                            Some(pre.msg())
                        }
                        "ForwardKeyEvent" => body.deserialize::<(u32, u32, u32)>().ok().map(|(keyval, _keycode, state)| Msg::Forward { keyval, state: state & !RELEASE, release: state & RELEASE != 0 }),
                        _ => None,
                    };
                    if let Some(msg) = msg {
                        p.send(msg);
                    }
                }
            });
        }
        Err(e) => log::event("ibus.signals", "", &format!("error={e}")),
    }
    poke.send(Msg::Ready(Engine::IBus));
    for c in cmds {
        let r = match c {
            Cmd::FocusIn => ic.call::<_, _, ()>("FocusIn", &()),
            Cmd::FocusOut => ic.call::<_, _, ()>("FocusOut", &()),
            Cmd::Reset => ic.call::<_, _, ()>("Reset", &()),
            Cmd::Rect(x, y, w, h) => ic.call::<_, _, ()>("SetCursorLocation", &(x, y, w, h)),
            // IBus のキーの番号は X の番号から 8 を引いた物（evdev の番号）。離したことは旗で知らせる
            Cmd::Key { id, keyval, keycode, state, release, time: _ } => {
                let state = if release { state | RELEASE } else { state };
                match ic.call::<_, _, bool>("ProcessKeyEvent", &(keyval, keycode.saturating_sub(8), state)) {
                    Ok(handled) => {
                        poke.send(Msg::KeyDone { id, handled });
                        Ok(())
                    }
                    Err(e) => {
                        // 答えが無ければ、アプリに渡す（キーを失わない）
                        poke.send(Msg::KeyDone { id, handled: false });
                        Err(e)
                    }
                }
            }
        };
        if let Err(e) = r {
            log::event("ibus.call", "", &format!("error={e}"));
        }
    }
    let _ = ic.call::<_, _, ()>("Destroy", &());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use zbus::zvariant::{LE, Structure, serialized::Context, to_bytes};

    /// D-Bus の形に書き出して読み直す（知らせの本体と同じ「v」で届く形にする）
    fn over_the_wire(v: Value<'static>) -> OwnedValue {
        let data = to_bytes(Context::new_dbus(LE, 0), &v).unwrap();
        data.deserialize::<OwnedValue>().unwrap().0
    }

    fn text(s: &str, attrs: &[[u32; 4]]) -> Value<'static> {
        let none = || HashMap::<String, Value<'static>>::new();
        let list: Vec<Value<'static>> = attrs.iter().map(|&[t, v, a, b]| Value::Structure(Structure::from(("IBusAttribute".to_string(), none(), t, v, a, b)))).collect();
        let list = Value::Structure(Structure::from(("IBusAttrList".to_string(), none(), list)));
        Value::Structure(Structure::from(("IBusText".to_string(), none(), s.to_string(), list)))
    }

    #[test]
    fn ibus_text_reads_the_text_and_the_attributes() {
        // ibus-mozc が「私は|学生です」の「私は」を変換中に送った形（検証機で受けた物）
        let attrs = [[1, 2, 0, 2], [3, 13757183, 0, 2], [2, 0, 0, 2], [1, 1, 2, 6]];
        let v = over_the_wire(text("私は学生です", &attrs));
        assert_eq!(ibus_text(&v), Some(("私は学生です".to_string(), attrs.to_vec())));
        // 確定の知らせ（属性なし）
        let v = over_the_wire(text("私は学生です", &[]));
        assert_eq!(ibus_text(&v), Some(("私は学生です".to_string(), vec![])));
        // IBusText でない物は読まない
        let v = over_the_wire(Value::Structure(Structure::from(("IBusAttrList".to_string(), HashMap::<String, Value<'static>>::new(), Vec::<Value<'static>>::new()))));
        assert_eq!(ibus_text(&v), None);
    }
}
