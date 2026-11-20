//! 日本語入力の候補の窓が、文字を打っている位置に出たかを記録に残す（確かめのための道具）。
//! 変換中に手が止まったとき、文字の位置のまわりにある他の窓の位置と大きさを記録し、
//! 同じ範囲の画面を BMP に保存する。記録には、文字の位置からのずれも書く。

use std::path::{Path, PathBuf};

use taskcore::log;
use windows::Win32::Foundation::{HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::BOOL;

/// 撮る範囲: 文字の位置（左下）から左に 60、上に 80、幅 640、高さ 480（画素）
const LEFT: i32 = 60;
const UP: i32 = 80;
const WIDTH: i32 = 640;
const HEIGHT: i32 = 480;

struct Found {
    own: HWND,
    area: RECT,
    list: Vec<(String, String, RECT, bool)>,
}

fn overlaps(a: &RECT, b: &RECT) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

unsafe extern "system" fn each_window(hwnd: HWND, lp: LPARAM) -> BOOL {
    unsafe {
        let found = &mut *(lp.0 as *mut Found);
        if hwnd == found.own || !IsWindowVisible(hwnd).as_bool() {
            return true.into();
        }
        let mut r = RECT::default();
        if GetWindowRect(hwnd, &mut r).is_err() || !overlaps(&r, &found.area) {
            return true.into();
        }
        // 画面全体を覆うような大きな窓（ブラウザーなど）は候補の窓ではないので除く
        if r.right - r.left > 1200 || r.bottom - r.top > 900 {
            return true.into();
        }
        let mut class = [0u16; 128];
        let n = GetClassNameW(hwnd, &mut class).max(0) as usize;
        let mut title = [0u16; 128];
        let t = GetWindowTextW(hwnd, &mut title).max(0) as usize;
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, (&mut cloaked as *mut u32).cast(), 4);
        found.list.push((String::from_utf16_lossy(&class[..n]), String::from_utf16_lossy(&title[..t]), r, cloaked != 0));
        true.into()
    }
}

/// caret_left / caret_bottom は画面の座標。n は何枚目か
pub fn shot(own: HWND, caret_left: i32, caret_bottom: i32, dir: &Path, n: u32) {
    let area = RECT {
        left: caret_left - LEFT,
        top: caret_bottom - UP,
        right: caret_left - LEFT + WIDTH,
        bottom: caret_bottom - UP + HEIGHT,
    };
    let mut found = Found { own, area, list: Vec::new() };
    unsafe {
        let _ = EnumWindows(Some(each_window), LPARAM(&mut found as *mut Found as isize));
    }
    for (class, title, r, cloaked) in &found.list {
        log::event(
            "ime.window",
            &format!("n={n} class={class} title={title} cloaked={cloaked}"),
            &format!(
                "rect={},{},{},{} from_caret_dx={} dy={}",
                r.left,
                r.top,
                r.right,
                r.bottom,
                r.left - caret_left,
                r.top - caret_bottom
            ),
        );
    }
    let path = dir.join(format!("ime-shot-{n}.bmp"));
    match save_bmp(&area, &path) {
        Ok(()) => log::event(
            "ime.shot",
            &format!("n={n} caret_screen={caret_left},{caret_bottom} windows={}", found.list.len()),
            &format!("path={} area={},{},{},{}", path.display(), area.left, area.top, area.right, area.bottom),
        ),
        Err(e) => log::event("ime.shot", &format!("n={n}"), &format!("error={e}")),
    }
}

/// 画面の area の範囲を 32 ビットの BMP に保存する
fn save_bmp(area: &RECT, path: &PathBuf) -> std::io::Result<()> {
    let (w, h) = (area.right - area.left, area.bottom - area.top);
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        let old = SelectObject(mem, bmp.into());
        let _ = BitBlt(mem, 0, 0, w, h, Some(screen), area.left, area.top, SRCCOPY);
        SelectObject(mem, old);
        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        GetDIBits(mem, bmp, 0, h as u32, Some(pixels.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
    }
    let mut out = Vec::with_capacity(54 + pixels.len());
    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(54 + pixels.len() as u32).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&54u32.to_le_bytes());
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&w.to_le_bytes());
    out.extend_from_slice(&h.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&[0u8; 24]);
    out.extend_from_slice(&pixels);
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    std::fs::write(path, out)
}
