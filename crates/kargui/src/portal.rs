//! xdg-desktop-portal（D-Bus のセッションバス）を使う物: OS の明暗の設定と、ファイルのダイアログ。
//!
//! どちらも別のスレッドで呼び、窓の処理を待たせない。D-Bus は zbus（読み上げの accesskit_unix が使うのと同じ物）の
//! 同期の呼び方で使う。ダイアログはポータルが出す（Hyprland では GTK の物）。答えは Request の Response の知らせで届く。

use std::collections::HashMap;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

use crate::file_dialog::{FileDialog, file_uri_bytes};
use crate::log;

const DEST: &str = "org.freedesktop.portal.Desktop";
const PATH: &str = "/org/freedesktop/portal/desktop";

/// OS の明暗の設定（org.freedesktop.appearance の color-scheme。1 暗い・2 明るい・0 好みなし）を読む。
/// 暗い方なら true。ポータルが無い時などは Err
pub(crate) fn read_dark() -> zbus::Result<bool> {
    let conn = Connection::session()?;
    let p = Proxy::new(&conn, DEST, PATH, "org.freedesktop.portal.Settings")?;
    let v: OwnedValue = match p.call("ReadOne", &("org.freedesktop.appearance", "color-scheme")) {
        Ok(v) => v,
        // 古いポータル（ReadOne が無い）は Read で、値をもう 1 枚の箱に入れて返す
        Err(_) => {
            let outer: OwnedValue = p.call("Read", &("org.freedesktop.appearance", "color-scheme"))?;
            match &*outer {
                Value::Value(inner) => inner.try_to_owned()?,
                _ => outer,
            }
        }
    };
    Ok(u32::try_from(v)? == 1)
}

/// 頼みの番号（handle_token）を作るための数
static NEXT: AtomicU32 = AtomicU32::new(1);

/// ファイルのダイアログを出し、選んだファイルを返す（取り消しは空）。終わるまで戻らないので、別のスレッドで呼ぶ
pub(crate) fn choose_files(d: &FileDialog) -> zbus::Result<Vec<PathBuf>> {
    let conn = Connection::session()?;
    // 答えを取りこぼさないよう、頼む前に Request の知らせを待ち始める（場所は自分の名前と handle_token で決まる）
    let token = format!("kargui{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let sender = conn.unique_name().map(|n| n.trim_start_matches(':').replace('.', "_")).unwrap_or_default();
    let request_path = format!("{PATH}/request/{sender}/{token}");
    let request = Proxy::new(&conn, DEST, request_path.as_str(), "org.freedesktop.portal.Request")?;
    let mut responses = request.receive_signal("Response")?;

    let mut opts: HashMap<&str, Value> = HashMap::new();
    opts.insert("handle_token", Value::from(token.as_str()));
    opts.insert("modal", Value::from(true));
    if d.allows_multiple() {
        opts.insert("multiple", Value::from(true));
    }
    if !d.filters.is_empty() {
        // a(sa(us)): 種類の名前と、（0 = 「*.txt」の形, 形）の並び。拡張子の「*」は全部のファイル（Windows の「*.*」と
        // 違い、GTK の「*.*」は点の無い名前に合わないので「*」にする）
        let glob = |e: &String| if e == "*" { "*".to_string() } else { format!("*.{e}") };
        let filters: Vec<(String, Vec<(u32, String)>)> = d.filters.iter().map(|(name, exts)| (name.clone(), exts.iter().map(|e| (0u32, glob(e))).collect())).collect();
        opts.insert("filters", Value::from(filters));
    }
    if d.save
        && let Some(n) = &d.file_name
    {
        opts.insert("current_name", Value::from(n.as_str()));
    }
    if let Some(f) = &d.folder {
        // ay: 終わりに 0 を付けたバイト
        let mut b = f.as_os_str().to_owned().into_vec();
        b.push(0);
        opts.insert("current_folder", Value::from(b));
    }
    let chooser = Proxy::new(&conn, DEST, PATH, "org.freedesktop.portal.FileChooser")?;
    let title = d.title.clone().unwrap_or_default();
    let method = if d.save { "SaveFile" } else { "OpenFile" };
    let handle: OwnedObjectPath = chooser.call(method, &("", title.as_str(), opts))?;
    // 古いポータルは別の場所を返すことがある。その時はそちらで待ち直す
    if handle.as_str() != request_path {
        log::event("portal.request", &format!("expected={request_path}"), &format!("got={}", handle.as_str()));
        let request = Proxy::new(&conn, DEST, handle.as_str(), "org.freedesktop.portal.Request")?;
        responses = request.receive_signal("Response")?;
    }
    let msg = responses.next().ok_or_else(|| zbus::Error::Failure("no Response from the portal".into()))?;
    let (code, results): (u32, HashMap<String, OwnedValue>) = msg.body().deserialize()?;
    // 0 は選んだ、1 は取り消し、2 はほかの理由で終わった
    if code != 0 {
        log::event("portal.file_chooser", &format!("method={method}"), &format!("response={code}"));
        return Ok(Vec::new());
    }
    let uris: Vec<String> = match results.get("uris") {
        Some(v) => match &**v {
            Value::Array(a) => a.iter().filter_map(|u| if let Value::Str(s) = u { Some(s.to_string()) } else { None }).collect(),
            _ => Vec::new(),
        },
        None => Vec::new(),
    };
    Ok(uris.iter().filter_map(|u| file_uri_bytes(u)).map(|b| PathBuf::from(OsString::from_vec(b))).collect())
}
