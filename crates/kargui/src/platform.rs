//! 窓（Win32 を直接使う）、メッセージの輪、日本語入力（IMM32）、読み上げ（AccessKit）、描く道。
//!
//! 段階 0 の試作（spike/src/win.rs）で確かめた決まりを引き継ぐ:
//! - 変化が無い間は GetMessage で眠り、自分から起きない（待機中の CPU を 0 にする）
//! - アニメーションの間だけ、窓のあるモニターの垂直同期に合わせて描く。前面にない窓は full_rate が無ければ約 30 fps
//! - 変換を確定する Enter は IME が受け取り、アプリには VK_PROCESSKEY として届くので、送信しない
//! - 状態を借りたまま、窓へメッセージを送り返す関数（SetWindowPos・DestroyWindow・Imm*）を呼ばない。Post に入れて借りの外で行う
//! - 閉じる時は、GPU の物と読み上げを片付けてから窓を壊す

use std::cell::RefCell;
use std::rc::Rc;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use accesskit::{ActionHandler, ActionRequest, ActivationHandler, TreeUpdate};
use accesskit_windows::{Adapter, QueuedEvents};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmFlush, DwmSetWindowAttribute};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::DataExchange::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::System::Threading::{
    AvRevertMmThreadCharacteristics, AvSetMmThreadCharacteristicsW, GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_ABOVE_NORMAL,
    THREAD_PRIORITY_NORMAL,
};
use windows::Win32::UI::HiDpi::*;
use windows::Win32::UI::Input::Ime::*;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{BOOL, HSTRING, w};

use crate::app::{App, Frame, Options, Renderer};
use crate::cpu_frame;
use crate::damage::Summary;
use crate::gdi::Gdi;
use crate::geom::{Color, IRect};
use crate::gpu::Gpu;
use crate::ime;
use crate::input::{A11yAction, Event, Key, Mods};
use crate::log;

use crate::runtime::Runtime;
use crate::text::{ATLAS_SIZE, Text};
use crate::ui::Id;
use crate::ui::context::Clipboard;

const WM_APP_A11Y: u32 = WM_APP + 1;
/// ほかのスレッドが Waker::wake で窓を起こした
const WM_APP_WAKE: u32 = WM_APP + 2;
/// Ui::request_frame_after とツールチップの時刻に起こすタイマーの番号
const WAKE_TIMER: usize = 1;
/// windows crate の読み込んだ範囲に無いので自分で定義する。
/// match の中で読み込まれていない名前を書くと、何にでも当てはまる変数になり、
/// それより後のメッセージを全部受け止めてしまう（2026-11-21 に窓の枠が消えた原因）
const WM_MOUSELEAVE: u32 = 0x02A3;
/// WM_MOUSEMOVE の wParam で、左のボタンが押されている印（SystemServices の機能を読み込まずに済ませる）
const MK_LBUTTON: usize = 0x0001;
/// これより長くフレームの間が空いたら、内訳を記録する（60 Hz の 1.5 フレーム）
const SLOW_FRAME_MS: f64 = 25.0;
/// 変換ごとに記録する IME の問い合わせの数
const IME_REQUEST_LOG_MAX: u32 = 3;
/// マウスのホイール 1 目盛りで動かす量（論理ピクセル。行の高さ 32 の 3 行分）
const WHEEL_STEP: f32 = 96.0;
/// WM_GETOBJECT（読み上げソフトなどからの問い合わせ）を受けた回数
static GETOBJECT_COUNT: AtomicU32 = AtomicU32::new(0);

enum Surface {
    Gpu(Gpu),
    Cpu(Gdi),
}

/// 状態の借りを返してから行うこと
enum Post {
    SetWindowPos(HWND, RECT),
    Destroy(HWND),
    Raise(QueuedEvents),
    AssociateIme(HWND, bool),
    /// IME の変換を取り消す（変換中の文字をこちらで入力欄に確定した時）
    ImeCancel(HWND),
    ImePosition(HWND, IRect),
    DarkTitle(HWND, bool),
    /// マウスを捕まえる（true）か放す（false）か
    Capture(HWND, bool),
    /// Ui::window で頼まれた窓を開く・閉じる
    OpenWindow(crate::ui::context::WindowRequest),
    CloseWindow(String),
    /// 利用者がほかの窓の閉じるボタンを押した（主の窓に伝え、アプリの open を false にしてもらう）
    ClosedByUser(String),
    /// 入力でこの窓を描いた。ほかの窓も描き直す
    FrameOthers(HWND),
    /// システムのファイルのダイアログを出す（モーダル。借りの外で出し、選んだファイルを窓に渡す）
    FileDialog(HWND, String, crate::file_dialog::FileDialog),
    /// アプリを終える（Ui::quit。主の窓を壊す）
    Quit,
    /// ほかのアプリからファイルを落とせる窓だと OS に届け出る（OLE の準備を借りの外で行う）
    AcceptFiles(HWND),
}

/// 読み上げの操作を、窓の持ち主のスレッドへ渡す係（AccessKit は別のスレッドから呼ぶことがある）
struct ActionSink {
    hwnd: isize,
    queue: Arc<Mutex<VecDeque<ActionRequest>>>,
}

impl ActionHandler for ActionSink {
    fn do_action(&mut self, request: ActionRequest) {
        log::event("a11y.action", &format!("action={:?} node={:?}", request.action, request.target_node), "queued");
        if let Ok(mut q) = self.queue.lock() {
            q.push_back(request);
        }
        unsafe {
            let _ = PostMessageW(Some(HWND(self.hwnd as *mut _)), WM_APP_A11Y, WPARAM(0), LPARAM(0));
        }
    }
}

/// 最初の読み上げの木を渡す係。借りの外へ結果を持ち出せるよう、木を自分で持つ
struct Initial(Option<TreeUpdate>);

impl ActivationHandler for Initial {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        log::event("a11y.activate", "", "initial tree");
        self.0.take()
    }
}

/// OS のクリップボード
struct SystemClipboard {
    hwnd: isize,
}

const CF_UNICODETEXT: u32 = 13;

impl Clipboard for SystemClipboard {
    fn get(&mut self) -> Option<String> {
        unsafe {
            OpenClipboard(Some(HWND(self.hwnd as *mut _))).ok()?;
            let text = GetClipboardData(CF_UNICODETEXT).ok().and_then(|h| {
                let g = HGLOBAL(h.0);
                let p = GlobalLock(g) as *const u16;
                if p.is_null() {
                    return None;
                }
                let mut n = 0;
                while *p.add(n) != 0 {
                    n += 1;
                }
                let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
                let _ = GlobalUnlock(g);
                Some(s)
            });
            let _ = CloseClipboard();
            log::event("clipboard.get", "", &format!("chars={}", text.as_ref().map_or(0, |t| t.chars().count())));
            text
        }
    }

    fn set(&mut self, text: &str) {
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            if OpenClipboard(Some(HWND(self.hwnd as *mut _))).is_err() {
                log::event("clipboard.set", "", "error=open");
                return;
            }
            let _ = EmptyClipboard();
            let ok = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2).ok().map(|g| {
                let p = GlobalLock(g) as *mut u16;
                if !p.is_null() {
                    std::ptr::copy_nonoverlapping(wide.as_ptr(), p, wide.len());
                    let _ = GlobalUnlock(g);
                }
                SetClipboardData(CF_UNICODETEXT, Some(HANDLE(g.0))).is_ok()
            });
            let _ = CloseClipboard();
            log::event("clipboard.set", &format!("chars={}", text.chars().count()), &format!("ok={ok:?}"));
        }
    }
}

/// OS の「アニメーション効果」が切られているか（設定 > アクセシビリティ > 視覚効果）。読めなければ切られていない扱い
fn system_reduce_motion() -> bool {
    let mut on = BOOL(1);
    let r = unsafe {
        SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, Some((&mut on as *mut BOOL).cast()), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0))
    };
    r.is_ok() && !on.as_bool()
}

/// OS が暗いテーマか（設定 > 個人用設定 > 色 > アプリ モード）
fn system_dark() -> bool {
    let mut value = 1u32;
    let mut size = 4u32;
    let r = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&mut value as *mut u32).cast()),
            Some(&mut size),
        )
    };
    r.is_ok() && value == 0
}

/// システムのファイルのダイアログを出して、選んだファイルを返す（取り消したら空）。モーダルなので借りの外で呼ぶ
/// 記録にはパスを残さない（利用者のフォルダーの名前が入るため）。数と取り消しだけを残す
fn show_file_dialog(owner: HWND, d: &crate::file_dialog::FileDialog) -> Vec<std::path::PathBuf> {
    let args = format!("save={} multiple={} filters={} title={:?}", d.save, d.multiple, d.filters.len(), d.title.as_deref().unwrap_or(""));
    log::event("file_dialog.show", &args, "start");
    match file_dialog_inner(owner, d) {
        Ok(files) => {
            // file_dialog_inner が空を返すのは取り消した時だけ
            log::event("file_dialog.result", &args, &if files.is_empty() { "cancelled".to_string() } else { format!("files={}", files.len()) });
            files
        }
        Err(e) => {
            log::event("file_dialog.error", &args, &format!("{e:?}"));
            Vec::new()
        }
    }
}

/// パスから IShellItem を作る（始めのフォルダー）。shell32.dll は大きく、ふつうに読み込むと起動の時に読み込まれて
/// 起動が十数 ms 遅くなったので、使う時に探して呼ぶ
fn shell_item_from_path(path: &std::path::Path) -> Option<windows::Win32::UI::Shell::IShellItem> {
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::Win32::UI::Shell::IShellItem;
    use windows::core::{GUID, HRESULT, Interface, PCWSTR, s};
    type Create = unsafe extern "system" fn(PCWSTR, *mut core::ffi::c_void, *const GUID, *mut *mut core::ffi::c_void) -> HRESULT;
    unsafe {
        let module = LoadLibraryW(w!("shell32.dll")).ok()?;
        let f = GetProcAddress(module, s!("SHCreateItemFromParsingName"))?;
        let create: Create = std::mem::transmute(f);
        let wide = HSTRING::from(path.as_os_str());
        let mut out = std::ptr::null_mut();
        create(PCWSTR(wide.as_ptr()), std::ptr::null_mut(), &IShellItem::IID, &mut out).ok().ok()?;
        (!out.is_null()).then(|| IShellItem::from_raw(out))
    }
}

/// ファイルのダイアログの前に 1 回だけ、窓のスレッドを COM の STA にする（起動の時にはしない。起動を遅くしないため）
fn ensure_com() {
    thread_local! {
        static DONE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    if !DONE.with(|d| d.replace(true)) {
        let r = unsafe { windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED) };
        log::event("com.init", "apartment=sta", &format!("{r:?}"));
    }
}

/// ほかのアプリからファイルを落とせる窓にする（OS に届け出る）。Vec<PathBuf> を受け取る部品が初めて現れた時に 1 回だけ
/// 呼ぶ。OLE の準備はここまで遅らせ、使わないアプリの起動を遅くしない
fn accept_files(hwnd: HWND) {
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
    use windows::Win32::System::Ole::{IDropTarget, OleInitialize, RegisterDragDrop};
    use windows::Win32::UI::Shell::{CLSID_DragDropHelper, IDropTargetHelper};
    let init = unsafe { OleInitialize(None) };
    // 引っ張っている物の絵（エクスプローラーのファイルの絵）を、この窓の上でも OS に描いてもらう係
    let helper: Option<IDropTargetHelper> = unsafe { CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER) }.ok();
    let has_helper = helper.is_some();
    let target: IDropTarget = DropTarget { hwnd, helper, files: std::cell::Cell::new(false), last_effect: std::cell::Cell::new(u32::MAX) }.into();
    let res = unsafe { RegisterDragDrop(hwnd, &target) };
    log::event("files.accept", &format!("ole={init:?} helper={has_helper}"), &format!("{res:?}"));
    if res.is_ok() {
        with_window(hwnd, |st| st.file_drop = Some(target));
    }
}

/// 落とされた物（IDataObject）の中のファイルのパス（CF_HDROP）。ファイルでなければ空
fn dropped_paths(data: &windows::Win32::System::Com::IDataObject) -> Vec<std::path::PathBuf> {
    use windows::Win32::System::Com::{DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL};
    use windows::Win32::System::Ole::{CF_HDROP, ReleaseStgMedium};
    let fmt = FORMATETC { cfFormat: CF_HDROP.0, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 };
    let Ok(mut medium) = (unsafe { data.GetData(&fmt) }) else { return Vec::new() };
    let hdrop = windows::Win32::UI::Shell::HDROP(unsafe { medium.u.hGlobal.0 });
    let paths = drag_query_files(hdrop);
    unsafe { ReleaseStgMedium(&mut medium) };
    paths
}

/// HDROP の中のパスを全部読む。DragQueryFileW は shell32.dll にあり、ふつうに読み込むと起動が遅くなるので、使う時に探して呼ぶ
fn drag_query_files(hdrop: windows::Win32::UI::Shell::HDROP) -> Vec<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows::core::s;
    type Query = unsafe extern "system" fn(windows::Win32::UI::Shell::HDROP, u32, *mut u16, u32) -> u32;
    let Some(f) = (unsafe { LoadLibraryW(w!("shell32.dll")).ok().and_then(|m| GetProcAddress(m, s!("DragQueryFileW"))) }) else {
        return Vec::new();
    };
    let query: Query = unsafe { std::mem::transmute(f) };
    let n = unsafe { query(hdrop, u32::MAX, std::ptr::null_mut(), 0) };
    (0..n)
        .filter_map(|i| {
            let len = unsafe { query(hdrop, i, std::ptr::null_mut(), 0) };
            let mut buf = vec![0u16; len as usize + 1];
            let got = unsafe { query(hdrop, i, buf.as_mut_ptr(), buf.len() as u32) };
            (got > 0).then(|| std::path::PathBuf::from(std::ffi::OsString::from_wide(&buf[..got as usize])))
        })
        .collect()
}

/// OS からの「ファイルを引っ張って窓に入った・動いた・出た・落とした」を、kargui の入力に変える係（IDropTarget）。
/// 受け取れるか（ポインターの形）は、前のフレームの配置からその場で答える
#[windows::core::implement(windows::Win32::System::Ole::IDropTarget)]
struct DropTarget {
    hwnd: HWND,
    helper: Option<windows::Win32::UI::Shell::IDropTargetHelper>,
    /// 今の引っ張りがファイルを運んでいる（DragEnter で決め、出たか落としたら戻す）
    files: std::cell::Cell<bool>,
    /// 最後に OS に返したポインターの形（DROPEFFECT。変わった時だけ記録する）
    last_effect: std::cell::Cell<u32>,
}

impl DropTarget {
    fn client(&self, pt: &windows::Win32::Foundation::POINTL) -> (f32, f32) {
        let mut p = POINT { x: pt.x, y: pt.y };
        unsafe {
            let _ = ScreenToClient(self.hwnd, &mut p);
        }
        (p.x as f32, p.y as f32)
    }

    /// (x, y) で受け取れればコピーの形、受け取れなければ禁止の形を OS に返す
    fn answer(&self, x: f32, y: f32, effect: *mut windows::Win32::System::Ole::DROPEFFECT) -> windows::Win32::System::Ole::DROPEFFECT {
        use windows::Win32::System::Ole::{DROPEFFECT_COPY, DROPEFFECT_NONE};
        let ok = self.files.get() && with_window(self.hwnd, |st| st.rt.cx.accepts_files_at(x, y)).unwrap_or(false);
        let e = if ok { DROPEFFECT_COPY } else { DROPEFFECT_NONE };
        if !effect.is_null() {
            unsafe { *effect = e };
        }
        if self.last_effect.replace(e.0) != e.0 {
            log::event("files.effect", &format!("x={x} y={y}"), if ok { "copy" } else { "none (not allowed)" });
        }
        e
    }

    fn push(&self, e: Event) {
        with_window(self.hwnd, |st| st.push(e));
    }
}

impl windows::Win32::System::Ole::IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(
        &self,
        data: windows::core::Ref<windows::Win32::System::Com::IDataObject>,
        _keys: windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS,
        pt: &windows::Win32::Foundation::POINTL,
        effect: *mut windows::Win32::System::Ole::DROPEFFECT,
    ) -> windows::core::Result<()> {
        let paths = data.ok().map(dropped_paths).unwrap_or_default();
        let (x, y) = self.client(pt);
        let count = paths.len();
        self.files.set(count > 0);
        if count > 0 {
            self.push(Event::FileDragEnter { x, y, paths });
        }
        let e = self.answer(x, y, effect);
        log::event("files.drag_enter", &format!("files={count} x={x} y={y}"), &format!("effect={}", e.0));
        if let (Some(h), Ok(d)) = (&self.helper, data.ok()) {
            let p = POINT { x: pt.x, y: pt.y };
            let _ = unsafe { h.DragEnter(self.hwnd, d, &p, e) };
        }
        Ok(())
    }

    fn DragOver(
        &self,
        _keys: windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS,
        pt: &windows::Win32::Foundation::POINTL,
        effect: *mut windows::Win32::System::Ole::DROPEFFECT,
    ) -> windows::core::Result<()> {
        let (x, y) = self.client(pt);
        if self.files.get() {
            self.push(Event::FileDragOver { x, y });
        }
        let e = self.answer(x, y, effect);
        if let Some(h) = &self.helper {
            let p = POINT { x: pt.x, y: pt.y };
            let _ = unsafe { h.DragOver(&p, e) };
        }
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        if self.files.replace(false) {
            self.push(Event::FileDragLeave);
        }
        log::event("files.drag_leave", "", "ok");
        if let Some(h) = &self.helper {
            let _ = unsafe { h.DragLeave() };
        }
        Ok(())
    }

    fn Drop(
        &self,
        data: windows::core::Ref<windows::Win32::System::Com::IDataObject>,
        _keys: windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS,
        pt: &windows::Win32::Foundation::POINTL,
        effect: *mut windows::Win32::System::Ole::DROPEFFECT,
    ) -> windows::core::Result<()> {
        let (x, y) = self.client(pt);
        let e = self.answer(x, y, effect);
        if self.files.replace(false) {
            self.push(Event::FileDrop { x, y });
        }
        log::event("files.drop", &format!("x={x} y={y}"), &format!("effect={}", e.0));
        if let (Some(h), Ok(d)) = (&self.helper, data.ok()) {
            let p = POINT { x: pt.x, y: pt.y };
            let _ = unsafe { h.Drop(d, &p, e) };
        }
        Ok(())
    }
}

fn file_dialog_inner(owner: HWND, d: &crate::file_dialog::FileDialog) -> windows::core::Result<Vec<std::path::PathBuf>> {
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
    use windows::Win32::UI::Shell::Common::COMDLG_FILTERSPEC;
    use windows::Win32::UI::Shell::{FOS_ALLOWMULTISELECT, FOS_FORCEFILESYSTEM, FileOpenDialog, FileSaveDialog, IFileDialog, IFileOpenDialog, IShellItem, SIGDN_FILESYSPATH};
    use windows::core::{Interface, PCWSTR};
    ensure_com();
    unsafe {
        let dialog: IFileDialog = if d.save {
            CoCreateInstance::<_, IFileDialog>(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)?
        } else {
            CoCreateInstance::<_, IFileDialog>(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?
        };
        // 種類の名前と「*.txt;*.md」の形の文字。COMDLG_FILTERSPEC が指す間は持っておく
        let texts: Vec<(HSTRING, HSTRING)> = d
            .filters
            .iter()
            .map(|(name, exts)| (HSTRING::from(name.as_str()), HSTRING::from(exts.iter().map(|e| format!("*.{e}")).collect::<Vec<_>>().join(";"))))
            .collect();
        let specs: Vec<COMDLG_FILTERSPEC> = texts.iter().map(|(n, s)| COMDLG_FILTERSPEC { pszName: PCWSTR(n.as_ptr()), pszSpec: PCWSTR(s.as_ptr()) }).collect();
        if !specs.is_empty() {
            dialog.SetFileTypes(&specs)?;
        }
        if let Some(t) = &d.title {
            dialog.SetTitle(&HSTRING::from(t.as_str()))?;
        }
        if let Some(n) = &d.file_name {
            dialog.SetFileName(&HSTRING::from(n.as_str()))?;
        }
        if let Some(ext) = d.filters.first().and_then(|(_, e)| e.first()).filter(|_| d.save) {
            dialog.SetDefaultExtension(&HSTRING::from(ext.as_str()))?;
        }
        if let Some(item) = d.folder.as_deref().and_then(shell_item_from_path) {
            dialog.SetFolder(&item)?;
        }
        let mut options = dialog.GetOptions()?;
        options |= FOS_FORCEFILESYSTEM;
        if d.multiple && !d.save {
            options |= FOS_ALLOWMULTISELECT;
        }
        dialog.SetOptions(options)?;
        if let Err(e) = dialog.Show(Some(owner)) {
            // 取り消しは失敗ではない
            if e.code() == windows::core::HRESULT::from_win32(ERROR_CANCELLED.0) {
                return Ok(Vec::new());
            }
            return Err(e);
        }
        let items: Vec<IShellItem> = if d.multiple && !d.save {
            let all = dialog.cast::<IFileOpenDialog>()?.GetResults()?;
            (0..all.GetCount()?).map(|k| all.GetItemAt(k)).collect::<windows::core::Result<_>>()?
        } else {
            vec![dialog.GetResult()?]
        };
        let mut files = Vec::with_capacity(items.len());
        for item in items {
            let p = item.GetDisplayName(SIGDN_FILESYSPATH)?;
            let path = p.to_string();
            CoTaskMemFree(Some(p.0 as *const _));
            files.push(std::path::PathBuf::from(path.map_err(|_| windows::core::Error::from(E_FAIL))?));
        }
        Ok(files)
    }
}

/// 画像から窓のアイコンを作る（乗算済みの RGBA を、乗算を戻した BGRA にして 32 bit のアイコンにする）
fn make_icon(image: &crate::Image) -> Option<HICON> {
    let d = &image.0;
    let (w, h) = (d.width as i32, d.height as i32);
    let mut bgra = Vec::with_capacity(d.pixels.len());
    for px in d.pixels.chunks_exact(4) {
        let a = px[3] as u32;
        let un = |c: u8| if a == 0 { 0 } else { ((c as u32 * 255 + a / 2) / a).min(255) as u8 };
        bgra.extend([un(px[2]), un(px[1]), un(px[0]), px[3]]);
    }
    unsafe {
        let mut info = BITMAPINFO::default();
        info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
        info.bmiHeader.biWidth = w;
        // 負の高さは上の行から
        info.bmiHeader.biHeight = -h;
        info.bmiHeader.biPlanes = 1;
        info.bmiHeader.biBitCount = 32;
        info.bmiHeader.biCompression = BI_RGB.0;
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let color = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        if bits.is_null() {
            let _ = DeleteObject(color.into());
            return None;
        }
        std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits as *mut u8, bgra.len());
        let mask = CreateBitmap(w, h, 1, 1, None);
        let icon = CreateIconIndirect(&ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color });
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
        icon.ok()
    }
}

/// 今日の日付（地元の時刻の年・月・日）。Date::today が使う
pub(crate) fn local_date() -> Option<(i32, u32, u32)> {
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    Some((t.wYear as i32, t.wMonth as u32, t.wDay as u32))
}

fn key_of(vk: VIRTUAL_KEY) -> Option<Key> {
    const LETTERS: [Key; 26] = [Key::A, Key::B, Key::C, Key::D, Key::E, Key::F, Key::G, Key::H, Key::I, Key::J, Key::K, Key::L, Key::M, Key::N, Key::O, Key::P, Key::Q, Key::R, Key::S, Key::T, Key::U, Key::V, Key::W, Key::X, Key::Y, Key::Z];
    const DIGITS: [Key; 10] = [Key::Num0, Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
    const FKEYS: [Key; 12] = [Key::F1, Key::F2, Key::F3, Key::F4, Key::F5, Key::F6, Key::F7, Key::F8, Key::F9, Key::F10, Key::F11, Key::F12];
    let v = vk.0;
    if (0x41..=0x5A).contains(&v) {
        return Some(LETTERS[(v - 0x41) as usize]);
    }
    if (0x30..=0x39).contains(&v) {
        return Some(DIGITS[(v - 0x30) as usize]);
    }
    if (VK_F1.0..=VK_F12.0).contains(&v) {
        return Some(FKEYS[(v - VK_F1.0) as usize]);
    }
    Some(match vk {
        VK_RETURN => Key::Enter,
        VK_TAB => Key::Tab,
        VK_ESCAPE => Key::Escape,
        VK_SPACE => Key::Space,
        VK_BACK => Key::Backspace,
        VK_DELETE => Key::Delete,
        VK_LEFT => Key::Left,
        VK_RIGHT => Key::Right,
        VK_UP => Key::Up,
        VK_DOWN => Key::Down,
        VK_HOME => Key::Home,
        VK_END => Key::End,
        VK_APPS => Key::ContextMenu,
        VK_PRIOR => Key::PageUp,
        VK_NEXT => Key::PageDown,
        VK_INSERT => Key::Insert,
        _ => return None,
    })
}

fn mods_now() -> Mods {
    let down = |vk: VIRTUAL_KEY| unsafe { GetKeyState(vk.0 as i32) } < 0;
    Mods { shift: down(VK_SHIFT), ctrl: down(VK_CONTROL), alt: down(VK_MENU) }
}

fn lo(v: isize) -> i32 {
    (v & 0xffff) as i16 as i32
}
fn hi(v: isize) -> i32 {
    ((v >> 16) & 0xffff) as i16 as i32
}

/// ポインターの形を OS の決まった形にする（IDC_ARROW・IDC_NO など）
fn set_cursor(shape: windows::core::PCWSTR) {
    if let Ok(c) = unsafe { LoadCursorW(None, shape) } {
        unsafe { SetCursor(Some(c)) };
    }
}

struct State {
    hwnd: HWND,
    /// Ui::window のキー（主の窓は None）
    key: Option<String>,
    /// どの窓も同じアプリを使う（同じスレッドで、1 度に 1 つの窓だけが描く）
    app: Rc<RefCell<Box<dyn App>>>,
    renderer: Renderer,
    /// 主の窓だけ: 開いているほかの窓のキー
    open_windows: Vec<String>,
    /// ほかの窓の入力のあとの描き直し（ほかの窓へは広げない）
    synced: bool,
    /// 窓のアイコン（壊す時に片付ける）と、その元の画像（ほかの窓が引き継ぐ）
    icon: Option<HICON>,
    icon_image: Option<crate::Image>,
    /// 中の大きさの下限（論理ピクセル）
    min_size: Option<(f32, f32)>,
    rt: Runtime,
    surface: Option<Surface>,
    adapter: Option<Adapter>,
    actions: Arc<Mutex<VecDeque<ActionRequest>>>,
    post: Vec<Post>,
    events: Vec<Event>,
    /// 次のメッセージの区切りで描く
    need_frame: bool,
    /// 起こすタイマー（WAKE_TIMER）を掛けている
    wake_timer: bool,
    /// アプリがアニメーションを求めている
    animating: bool,
    started: Instant,
    shown: bool,
    full_rate: bool,
    window_focused: bool,
    composing: bool,
    high_surrogate: Option<u16>,
    last_caret: Option<IRect>,
    /// IME の問い合わせの数（種類ごと: 文字の位置・前後の文字・再変換・再変換の範囲・その他）。変換の始めと終わりで数え直す
    ime_requests: [u32; 5],
    /// CPU の道: 前のフレームの描く物の要約と背景の色
    summary: Summary,
    background: Color,
    dark_title: Option<bool>,
    tracking_leave: bool,
    /// 引っ張っている間に最後に決めたポインターの形（true は矢印、false は禁止の形）。変わった時だけ記録する
    drag_cursor: Option<bool>,
    /// ほかのアプリからファイルを受け取る係（OS に届け出た物。窓を壊す時に取り消す）と、届け出を頼んだか
    file_drop: Option<windows::Win32::System::Ole::IDropTarget>,
    file_drop_asked: bool,
    // 遅いフレームの内訳
    last_tick: Option<Instant>,
    t_build: f64,
    t_draw: f64,
    t_wait: f64,
    slow_frames: u32,
    /// アニメーション 1 回分の、作る・描くの時間の合計（終わった時に anim.summary として記録する）
    anim: AnimStats,
}

thread_local! {
    /// 窓の状態（0 番が主の窓、後ろは Ui::window で開いた窓）
    static STATE: RefCell<Vec<State>> = const { RefCell::new(Vec::new()) };
}

/// 窓の一覧を借りて f を呼ぶ。すでに借りている（メッセージの入れ子）ときは None
fn with_windows<R>(f: impl FnOnce(&mut Vec<State>) -> R) -> Option<R> {
    STATE.with(|s| {
        let Ok(mut guard) = s.try_borrow_mut() else { return None };
        Some(f(&mut guard))
    })
}

/// 主の窓の状態を借りて f を呼ぶ
fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    with_windows(|ws| ws.first_mut().map(f)).flatten()
}

/// hwnd の窓の状態を借りて f を呼ぶ
fn with_window<R>(hwnd: HWND, f: impl FnOnce(&mut State) -> R) -> Option<R> {
    with_windows(|ws| ws.iter_mut().find(|w| w.hwnd == hwnd).map(f)).flatten()
}

/// 借りを返したあとで、Post に入れたことを行う（行ったことがまた Post を足すので、無くなるまで繰り返す）
fn drain_post() {
    loop {
        let posts = with_windows(|ws| ws.iter_mut().flat_map(|w| std::mem::take(&mut w.post)).collect::<Vec<_>>()).unwrap_or_default();
        if posts.is_empty() {
            break;
        }
        for p in posts {
            do_post(p);
        }
    }
}

fn do_post(p: Post) {
    {
        match p {
            Post::SetWindowPos(hwnd, r) => unsafe {
                let res = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, SWP_NOZORDER | SWP_NOACTIVATE);
                log::event("window.setpos", &format!("rect={},{},{},{}", r.left, r.top, r.right, r.bottom), &format!("{res:?}"));
            },
            Post::Destroy(hwnd) => unsafe {
                let res = DestroyWindow(hwnd);
                log::event("window.destroy_call", "", &format!("{res:?}"));
            },
            Post::Raise(events) => events.raise(),
            Post::AssociateIme(hwnd, on) => unsafe {
                let ok = ImmAssociateContextEx(hwnd, HIMC::default(), if on { IACE_DEFAULT } else { 0 });
                log::event("ime.associate", &format!("on={on}"), &format!("ok={}", ok.as_bool()));
            },
            Post::ImePosition(hwnd, caret) => set_ime_position(hwnd, caret),
            Post::ImeCancel(hwnd) => unsafe {
                let himc = ImmGetContext(hwnd);
                let ok = !himc.is_invalid() && ImmNotifyIME(himc, NI_COMPOSITIONSTR, CPS_CANCEL, 0).as_bool();
                if !himc.is_invalid() {
                    let _ = ImmReleaseContext(hwnd, himc);
                }
                log::event("ime.cancel", "", &format!("ok={ok}"));
            },
            Post::Capture(hwnd, on) => unsafe {
                if on {
                    SetCapture(hwnd);
                } else {
                    let _ = ReleaseCapture();
                }
            },
            Post::DarkTitle(hwnd, dark) => unsafe {
                let v = BOOL::from(dark);
                let res = DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, (&v as *const BOOL).cast(), 4);
                log::event("window.dark_title", &format!("dark={dark}"), &format!("{res:?}"));
            },
            Post::OpenWindow(r) => open_window(r),
            Post::CloseWindow(key) => {
                let hwnd = with_windows(|ws| ws.iter().find(|w| w.key.as_deref() == Some(key.as_str())).map(|w| w.hwnd)).flatten();
                if let Some(h) = hwnd {
                    // WM_DESTROY が入れ子で届くので、借りの外で壊してから一覧から外す
                    let res = unsafe { DestroyWindow(h) };
                    log::event("window.close_other", &format!("key={key}"), &format!("{res:?}"));
                    with_windows(|ws| ws.retain(|w| w.hwnd != h));
                }
                with_state(|st| st.open_windows.retain(|k| *k != key));
            }
            Post::ClosedByUser(key) => {
                with_state(|st| {
                    st.rt.cx.closed_windows.insert(key);
                    st.need_frame = true;
                });
            }
            Post::FileDialog(hwnd, key, d) => {
                let files = show_file_dialog(hwnd, &d);
                log::event("file_dialog.answer", &format!("key={key} save={}", d.save), &format!("files={}", files.len()));
                // 取り消しは空の答え（Ui::file_dialog_cancelled が読む）
                with_window(hwnd, |st| {
                    st.rt.cx.file_results.insert(key, files);
                    st.need_frame = true;
                });
            }
            Post::AcceptFiles(hwnd) => accept_files(hwnd),
            Post::Quit => {
                if let Some(h) = with_state(|st| st.hwnd) {
                    let res = unsafe { DestroyWindow(h) };
                    log::event("app.quit", "", &format!("{res:?}"));
                }
            }
            Post::FrameOthers(hwnd) => {
                with_windows(|ws| {
                    for w in ws.iter_mut().filter(|w| w.hwnd != hwnd) {
                        w.need_frame = true;
                        w.synced = true;
                    }
                });
            }
        }
    }
}

/// Ui::window で頼まれた窓を作って見せ、1 枚目を描く（描き方・明暗は主の窓と同じ）
fn open_window(r: crate::ui::context::WindowRequest) {
    let Some((app, renderer, dark, full_rate, started, main_icon)) =
        with_state(|st| (st.app.clone(), st.renderer, st.rt.cx.dark, st.full_rate, st.started, st.icon_image.clone()))
    else {
        return;
    };
    // アイコンが無ければ主の窓のアイコン
    let icon = r.icon.clone().or(main_icon);
    match create_window(Some(r.key.clone()), &r.title, r.size, r.position, renderer, dark, full_rate, icon, r.min_size, app, started) {
        Ok(w) => {
            let hwnd = w.hwnd;
            with_windows(|ws| ws.push(w));
            with_state(|st| st.open_windows.push(r.key.clone()));
            unsafe {
                let _ = ShowWindow(hwnd, if r.activate { SW_SHOW } else { SW_SHOWNOACTIVATE });
            }
            with_window(hwnd, |st| {
                st.frame("first");
                st.shown = true;
            });
            log::event("window.open_other", &format!("key={}", r.key), "ok");
        }
        Err(e) => log::event("window.open_other", &format!("key={}", r.key), &format!("error={e:?}")),
    }
}

/// 文字を打つ位置を IME に知らせる（候補の窓をそこに出すため）
fn set_ime_position(hwnd: HWND, caret: IRect) {
    unsafe {
        let himc = ImmGetContext(hwnd);
        if himc.is_invalid() {
            return;
        }
        let comp = COMPOSITIONFORM { dwStyle: CFS_POINT, ptCurrentPos: POINT { x: caret.x, y: caret.y }, rcArea: RECT::default() };
        let cand = CANDIDATEFORM {
            dwIndex: 0,
            dwStyle: CFS_EXCLUDE,
            ptCurrentPos: POINT { x: caret.x, y: caret.bottom() },
            rcArea: RECT { left: caret.x, top: caret.y, right: caret.right(), bottom: caret.bottom() },
        };
        let a = ImmSetCompositionWindow(himc, &comp);
        let b = ImmSetCandidateWindow(himc, &cand);
        let _ = ImmReleaseContext(hwnd, himc);
        log::event("ime.caret", &format!("client={},{} h={}", caret.x, caret.y, caret.h), &format!("comp={} cand={}", a.as_bool(), b.as_bool()));
    }
}

impl State {
    fn size(&self) -> (i32, i32) {
        (self.rt.cx.width as i32, self.rt.cx.height as i32)
    }

    fn resize(&mut self, width: i32, height: i32) {
        if width <= 0 || height <= 0 {
            return;
        }
        let scale = self.rt.cx.scale;
        self.rt.resize(width as f32, height as f32, scale);
        let res = match self.surface.as_mut() {
            Some(Surface::Gpu(g)) => g.resize(width as u32, height as u32),
            Some(Surface::Cpu(c)) => c.resize(width, height),
            None => Ok(()),
        };
        if let Err(e) = res {
            log::event("resize", &format!("size={width}x{height}"), &format!("error={e:?}"));
        }
        self.summary = Summary::default();
        self.need_frame = true;
    }

    /// 起こしてほしい時間があればタイマーを掛け、無ければ外す（掛けている時だけ外す）
    fn set_wake(&mut self, after: Option<std::time::Duration>) {
        match after {
            Some(d) => {
                let ms = (d.as_millis().min(u32::MAX as u128) as u32).max(1);
                unsafe {
                    SetTimer(Some(self.hwnd), WAKE_TIMER, ms, None);
                }
                self.wake_timer = true;
            }
            None if self.wake_timer => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), WAKE_TIMER);
                }
                self.wake_timer = false;
            }
            None => {}
        }
    }

    /// 1 フレーム: 入力を渡してビューを回し、描いて画面に出す
    fn frame(&mut self, why: &str) {
        let t = Instant::now();
        let events = std::mem::take(&mut self.events);
        let focus_before = self.rt.cx.focus;
        let out = {
            let mut app = self.app.borrow_mut();
            match &self.key {
                None => self.rt.frame(&mut **app, events),
                Some(key) => self.rt.frame_with(&mut |ui: &mut crate::ui::Ui| app.window(key, ui), events),
            }
        };
        // 主の窓: 頼まれた窓と開いている窓をそろえる（作る・壊すのは借りの外）
        if self.key.is_none() {
            let requested = self.rt.cx.windows.clone();
            for r in requested.iter().filter(|r| !self.open_windows.contains(&r.key)) {
                self.post.push(Post::OpenWindow(r.clone()));
            }
            for k in self.open_windows.iter().filter(|k| !requested.iter().any(|r| &r.key == *k)) {
                self.post.push(Post::CloseWindow(k.clone()));
            }
        }
        // Ui::quit: 主の窓を壊してアプリを終える
        if std::mem::take(&mut self.rt.cx.quit) {
            self.post.push(Post::Quit);
        }
        // ファイルのダイアログは、描いた後で借りの外で出す
        if let Some((key, d)) = self.rt.cx.file_dialog_request.take() {
            self.post.push(Post::FileDialog(self.hwnd, key, d));
        }
        // ファイルを受け取る部品が初めて現れたら、OS にファイルを落とせる窓だと届け出る（1 回だけ）
        if self.rt.cx.wants_files && !self.file_drop_asked {
            self.file_drop_asked = true;
            self.post.push(Post::AcceptFiles(self.hwnd));
        }
        // 入力で描いたら、ほかの窓も描き直す（アプリを共有するため。描き直しはそれ以上広げない）
        if why == "input" {
            self.post.push(Post::FrameOthers(self.hwnd));
        }
        self.set_wake(out.wake_after);
        self.animating = out.want_frame;
        self.need_frame = false;
        self.t_build = t.elapsed().as_secs_f64() * 1000.0;
        let t_draw = Instant::now();
        let bg = self.rt.cx.theme.background;
        let (w, h) = self.size();
        let mut mode = String::from("full");
        match self.surface.as_mut() {
            Some(Surface::Gpu(g)) => {
                if let Err(e) = g.render(&self.rt.draw, &mut self.rt.cx.text.atlas, bg, 0) {
                    log::event("render", &format!("why={why}"), &format!("error={e:?}"));
                }
            }
            Some(Surface::Cpu(c)) => {
                // 背景の色が変わったとき（明暗の切り替え）は、要約に背景が入らないので全部描き直す
                let full = bg != self.background;
                self.background = bg;
                let atlas = &self.rt.cx.text.atlas;
                let out = cpu_frame::draw(c.pixels(), w, h, &self.rt.draw, atlas, bg, &self.summary, full);
                c.present(&out.present);
                self.summary = out.summary;
                mode = out.mode;
            }
            None => return,
        }
        self.t_draw = t_draw.elapsed().as_secs_f64() * 1000.0;
        if !self.animating {
            log::event(
                "render",
                &format!("why={why} {mode} quads={}", self.rt.draw.quads.len()),
                &format!("ms={:.2} draw_ms={:.2}", self.t_build + self.t_draw, self.t_draw),
            );
        }
        // 読み上げ・日本語入力・タイトルバーの色
        if let Some(adapter) = self.adapter.as_mut() {
            let rt = &mut self.rt;
            if let Some(events) = adapter.update_if_active(|| rt.a11y().0) {
                self.post.push(Post::Raise(events));
            }
        }
        // 変換中の文字をこちらで確定した時は、関連付けを変える前に IME の変換を取り消す
        if std::mem::take(&mut self.rt.cx.ime_cancel) {
            self.post.push(Post::ImeCancel(self.hwnd));
        }
        let was_input = focus_before.is_some_and(|f| self.rt.cx.prev.inputs.contains_key(&f));
        let is_input = self.rt.cx.focus.is_some_and(|f| self.rt.cx.prev.inputs.contains_key(&f));
        if was_input != is_input || self.rt.frames == 1 {
            self.post.push(Post::AssociateIme(self.hwnd, is_input));
        }
        if let Some(c) = self.rt.cx.caret {
            let caret = IRect::new(c.x as i32, c.y as i32, c.w as i32, c.h as i32);
            if self.last_caret != Some(caret) {
                self.last_caret = Some(caret);
                self.post.push(Post::ImePosition(self.hwnd, caret));
            }
        }
        // 明るいテーマで始まるときは呼ばない（OS の既定のまま）
        let want = self.rt.cx.dark;
        if self.dark_title.is_some_and(|d| d != want) || (self.dark_title.is_none() && want) {
            self.post.push(Post::DarkTitle(self.hwnd, want));
        }
        self.dark_title = Some(want);
        if self.shown && self.key.is_none() {
            let number = self.rt.frames - 1;
            self.app.borrow_mut().presented(&Frame { number, since_start: self.started.elapsed() });
        }
    }

    /// アニメーションの 1 回分。遅いフレームの内訳も記録する
    fn tick(&mut self) {
        let now = Instant::now();
        if let Some(prev) = self.last_tick {
            let gap = (now - prev).as_secs_f64() * 1000.0;
            if gap > SLOW_FRAME_MS {
                self.slow_frames += 1;
                let rest = gap - self.t_build - self.t_draw - self.t_wait;
                log::event(
                    "frame.slow",
                    &format!("n={} gap_ms={gap:.1}", self.slow_frames),
                    &format!("build_ms={:.2} draw_ms={:.2} wait_ms={:.2} other_ms={rest:.1}", self.t_build, self.t_draw, self.t_wait),
                );
            }
        }
        self.last_tick = Some(now);
        self.frame("animation");
        let work = self.t_build + self.t_draw;
        let a = &mut self.anim;
        a.frames += 1;
        a.build_ms += self.t_build;
        a.draw_ms += self.t_draw;
        a.max_ms = a.max_ms.max(work);
    }

    /// アニメーションが止まったら、その間の作る・描くの時間をまとめて記録する。
    /// 待ちを含まない、このスレッドの仕事の時間（測定の道具の CPU の時間は 15.6 ms 単位で粗いため）
    fn end_animation(&mut self) {
        let a = std::mem::take(&mut self.anim);
        if a.frames == 0 {
            return;
        }
        let n = a.frames as f64;
        log::event(
            "anim.summary",
            &format!("frames={}", a.frames),
            &format!(
                "build_ms_per_frame={:.3} draw_ms_per_frame={:.3} work_ms_per_frame={:.3} work_ms_max={:.2}",
                a.build_ms / n,
                a.draw_ms / n,
                (a.build_ms + a.draw_ms) / n,
                a.max_ms
            ),
        );
    }

    fn wait_vblank(&mut self) {
        // 前面にない窓は、full_rate が無ければ 2 回に 1 回だけ描く（約 30 fps）
        let waits = if self.window_focused || self.full_rate { 1 } else { 2 };
        let t = Instant::now();
        for _ in 0..waits {
            match self.surface.as_mut() {
                Some(Surface::Gpu(g)) => g.wait_vblank(),
                Some(Surface::Cpu(c)) => c.wait_vblank(),
                None => {}
            }
        }
        self.t_wait = t.elapsed().as_secs_f64() * 1000.0;
    }

    fn forget_output(&mut self) {
        match self.surface.as_mut() {
            Some(Surface::Gpu(g)) => g.forget_output(),
            Some(Surface::Cpu(c)) => c.forget_output(),
            None => {}
        }
    }

    fn push(&mut self, e: Event) {
        self.events.push(e);
        self.need_frame = true;
    }

    fn ime_string(&self, kind: IME_COMPOSITION_STRING) -> Option<String> {
        let bytes = self.ime_raw(kind)?;
        let units: Vec<u16> = bytes.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
        Some(String::from_utf16_lossy(&units))
    }

    /// 変換中の文字の情報（文字・文字ごとの属性・文節の境目）をそのまま読む。IME が答えなければ None
    fn ime_raw(&self, kind: IME_COMPOSITION_STRING) -> Option<Vec<u8>> {
        unsafe {
            let himc = ImmGetContext(self.hwnd);
            if himc.is_invalid() {
                return None;
            }
            let bytes = ImmGetCompositionStringW(himc, kind, None, 0);
            let result = if bytes > 0 {
                let mut buf = vec![0u8; bytes as usize];
                ImmGetCompositionStringW(himc, kind, Some(buf.as_mut_ptr().cast()), bytes as u32);
                Some(buf)
            } else if bytes == 0 {
                Some(Vec::new())
            } else {
                None
            };
            let _ = ImmReleaseContext(self.hwnd, himc);
            result
        }
    }

    /// 変換中の文字の中のキャレットの位置（UTF-16 の位置）。IME が答えなければ None
    fn ime_cursor(&self) -> Option<usize> {
        unsafe {
            let himc = ImmGetContext(self.hwnd);
            if himc.is_invalid() {
                return None;
            }
            let pos = ImmGetCompositionStringW(himc, GCS_CURSORPOS, None, 0);
            let _ = ImmReleaseContext(self.hwnd, himc);
            usize::try_from(pos).ok()
        }
    }

    /// IME の「変換中の何文字目は画面のどこか」の問い合わせに答える。描いた時に覚えた文字の境目を使う
    fn answer_char_position(&self, info: &mut IMECHARPOSITION) -> bool {
        let (Some(p), Some(r)) = (self.rt.cx.preedit_rect, self.rt.cx.input_rect) else { return false };
        let x = self.rt.cx.ime_field.as_ref().and_then(|f| ime::char_x(f, info.dwCharPos as usize)).unwrap_or(p.x);
        let mut pt = POINT { x: x.round() as i32, y: p.y as i32 };
        let mut lt = POINT { x: r.x as i32, y: r.y as i32 };
        let mut rb = POINT { x: r.right() as i32, y: r.bottom() as i32 };
        unsafe {
            let _ = ClientToScreen(self.hwnd, &mut pt);
            let _ = ClientToScreen(self.hwnd, &mut lt);
            let _ = ClientToScreen(self.hwnd, &mut rb);
        }
        info.pt = pt;
        info.cLineHeight = p.h.max(1.0) as u32;
        info.rcDocument = RECT { left: lt.x, top: lt.y, right: rb.x, bottom: rb.y };
        true
    }

    fn apply_a11y_actions(&mut self) {
        let requests: Vec<ActionRequest> = self.actions.lock().map(|mut q| q.drain(..).collect()).unwrap_or_default();
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

    /// メッセージを受ける。None を返したら DefWindowProc に任せる
    fn handle(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_CLOSE => {
                log::event("window.close", &format!("key={:?}", self.key), "requested");
                match &self.key {
                    // ほかの窓はすぐには壊さない。アプリの open を false にしてもらい、次のフレームで閉じる
                    Some(key) => self.post.push(Post::ClosedByUser(key.clone())),
                    // 主の窓: アプリが認めたら閉じる（認めなければ、アプリが聞いてから Ui::quit で閉じる）
                    None => {
                        let ok = self.app.try_borrow_mut().map(|mut a| a.close_requested()).unwrap_or(false);
                        log::event("window.close_requested", "", &format!("allowed={ok}"));
                        if ok {
                            self.post.push(Post::Destroy(self.hwnd));
                        } else {
                            self.need_frame = true;
                        }
                    }
                }
                Some(LRESULT(0))
            }
            WM_DESTROY => {
                // 窓が残っているうちに、GPU の物と読み上げを片付け、ファイルを落とせる窓の届け出を取り消す
                if self.file_drop.take().is_some() {
                    let res = unsafe { windows::Win32::System::Ole::RevokeDragDrop(self.hwnd) };
                    log::event("files.revoke", "", &format!("{res:?}"));
                }
                self.adapter = None;
                let had = self.surface.take().is_some();
                if let Some(i) = self.icon.take() {
                    let _ = unsafe { DestroyIcon(i) };
                }
                log::event("window.destroy", &format!("key={:?}", self.key), &format!("surface_dropped={had}"));
                // 主の窓を閉じたらアプリを終える
                if self.key.is_none() {
                    unsafe { PostQuitMessage(0) };
                }
                Some(LRESULT(0))
            }
            // 中の大きさの下限を、今の拡大率の窓の大きさにして伝える
            WM_GETMINMAXINFO => {
                let (w, h) = self.min_size?;
                let dpi = unsafe { GetDpiForWindow(self.hwnd) }.max(96);
                let s = dpi as f32 / 96.0;
                let mut r = RECT { left: 0, top: 0, right: (w * s).round() as i32, bottom: (h * s).round() as i32 };
                unsafe {
                    let _ = AdjustWindowRectExForDpi(&mut r, WS_OVERLAPPEDWINDOW, false, WINDOW_EX_STYLE(0), dpi);
                    let info = &mut *(lp.0 as *mut MINMAXINFO);
                    info.ptMinTrackSize = POINT { x: r.right - r.left, y: r.bottom - r.top };
                }
                Some(LRESULT(0))
            }
            WM_SIZE => {
                let (w, h) = (lo(lp.0), hi(lp.0));
                log::event("window.size", &format!("size={w}x{h} kind={}", wp.0), "ok");
                self.resize(w, h);
                if self.shown && w > 0 && h > 0 {
                    self.frame("size");
                }
                Some(LRESULT(0))
            }
            WM_PAINT => {
                match self.surface.as_ref() {
                    Some(Surface::Cpu(c)) => c.paint(),
                    _ => unsafe {
                        let _ = ValidateRect(Some(self.hwnd), None);
                        self.need_frame = true;
                    },
                }
                Some(LRESULT(0))
            }

            WM_DPICHANGED => {
                let dpi = (wp.0 & 0xffff) as f32;
                let (w, h) = (self.rt.cx.width, self.rt.cx.height);
                self.rt.resize(w, h, dpi / 96.0);
                self.last_caret = None;
                let r = unsafe { *(lp.0 as *const RECT) };
                log::event("window.dpi", &format!("dpi={dpi}"), "ok");
                self.post.push(Post::SetWindowPos(self.hwnd, r));
                self.summary = Summary::default();
                self.need_frame = true;
                Some(LRESULT(0))
            }
            // OS の設定が変わった（アニメーション効果の入り切りもここに来る）
            WM_SETTINGCHANGE => {
                let reduce = system_reduce_motion();
                if reduce != self.rt.cx.reduce_motion {
                    self.rt.cx.reduce_motion = reduce;
                    log::event("motion.setting", &format!("key={:?} spi={:#x}", self.key, wp.0), &format!("reduce={reduce}"));
                    self.need_frame = true;
                }
                None
            }
            WM_MOVE | WM_EXITSIZEMOVE | WM_DISPLAYCHANGE => {
                self.forget_output();
                self.last_caret = None;
                None
            }
            WM_SETFOCUS | WM_KILLFOCUS => {
                let focused = msg == WM_SETFOCUS;
                self.window_focused = focused;
                log::event("window.focus", &format!("focused={focused}"), "ok");
                if let Some(events) = self.adapter.as_mut().and_then(|a| a.update_window_focus_state(focused)) {
                    self.post.push(Post::Raise(events));
                }
                self.last_caret = None;
                self.need_frame = true;
                None
            }
            WM_MOUSEMOVE => {
                let (x, y) = (lo(lp.0) as f32, hi(lp.0) as f32);
                if !self.tracking_leave {
                    let mut t =
                        TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: self.hwnd, dwHoverTime: 0 };
                    self.tracking_leave = unsafe { TrackMouseEvent(&mut t) }.is_ok();
                }
                // Esc で引っ張りをやめた後もボタンを押したままなら、つかんだままで WM_SETCURSOR が来ないので、ここで矢印に戻す
                if self.drag_cursor.is_some() && self.rt.cx.dnd.is_none() {
                    set_cursor(IDC_ARROW);
                    log::event("drag.cursor", &format!("x={x} y={y}"), "arrow (drag ended)");
                    self.drag_cursor = None;
                }
                let cx = &self.rt.cx;
                // ほかのアプリからの引っ張りは、マウスをつかんでいるのがそのアプリなので、ここでは扱わない（OLE の
                // IDropTarget が扱う）。落とす直前にボタンの離れた動きが届いても、取り消さない
                let inner_drag = cx.dnd.as_ref().is_some_and(|d| !d.external);
                if cx.drag.is_some() || cx.pressed.is_some() || cx.press_source.is_some() || inner_drag {
                    // つまみをつかんでいる間・部品を押している間・引っ張り出せる部品を押している間と引っ張っている間は、
                    // 移動を渡す。左のボタンが離れていたら（捕まえを失って WM_LBUTTONUP が届かなかった）、押すのを終える。
                    // 引っ張っている途中なら、そこへは落とさずに取り消す
                    if wp.0 & MK_LBUTTON != 0 {
                        self.push(Event::PointerMove { x, y });
                        // つかんでいる間は WM_SETCURSOR が来ないので、ここで形を決める（受け取れない所は禁止の形）
                        let shape = self.rt.cx.drag_cursor(x, y);
                        if let Some(ok) = shape {
                            set_cursor(if ok { IDC_ARROW } else { IDC_NO });
                        }
                        if shape != self.drag_cursor {
                            log::event("drag.cursor", &format!("x={x} y={y}"), match shape {
                                Some(true) => "arrow",
                                Some(false) => "not-allowed",
                                None => "none",
                            });
                            self.drag_cursor = shape;
                        }
                    } else if inner_drag || self.rt.cx.press_source.is_some() {
                        log::event("drag.lost", &format!("x={x} y={y}"), "cancel");
                        self.push(Event::PointerCancel);
                        set_cursor(IDC_ARROW);
                        self.drag_cursor = None;
                    } else {
                        let what = if self.rt.cx.drag.is_some() { "scrollbar.lost" } else { "pointer.lost" };
                        log::event(what, &format!("x={x} y={y}"), "released");
                        self.push(Event::PointerUp { x, y });
                    }
                    return Some(LRESULT(0));
                }
                let hover = self.rt.hover_at(x, y);
                let bar = self.rt.cx.scrollbar_at(x, y).map(|l| l.id);
                if bar != self.rt.cx.bar_hover {
                    log::event("scrollbar.hover", &format!("x={x} y={y}"), &format!("on={}", bar.is_some()));
                }
                if hover != self.rt.cx.hover {
                    log::event("hover", &format!("x={x} y={y}"), &format!("id={}", hover.map_or("none".to_string(), |h| format!("{:x}", h.0))));
                }
                if hover != self.rt.cx.hover || bar != self.rt.cx.bar_hover {
                    self.rt.cx.set_hover(hover);
                    self.rt.cx.bar_hover = bar;
                    self.need_frame = true;
                }
                Some(LRESULT(0))
            }
            WM_MOUSELEAVE => {
                self.tracking_leave = false;
                let bar = self.rt.cx.bar_hover.take().is_some();
                if bar {
                    log::event("scrollbar.hover", "leave", "on=false");
                }
                let had = self.rt.cx.hover.is_some();
                log::event("pointer.leave", "", &format!("had_hover={had}"));
                self.rt.cx.set_hover(None);
                if had | bar {
                    self.need_frame = true;
                }
                Some(LRESULT(0))
            }
            WM_SETCURSOR if (lp.0 & 0xffff) as u32 == HTCLIENT => {
                let mut p = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut p);
                    let _ = ScreenToClient(self.hwnd, &mut p);
                }
                let hit = self.rt.cx.hit(p.x as f32, p.y as f32).cloned();
                // 仕切りの上は左右か上下の矢印、入力欄の上は I の形
                let cursor = match hit.as_ref().and_then(|h| self.rt.cx.prev.split_bars.get(&h.id).copied()) {
                    Some(true) => IDC_SIZENS,
                    Some(false) => IDC_SIZEWE,
                    None if hit.is_some_and(|h| h.text_input) => IDC_IBEAM,
                    None => IDC_ARROW,
                };
                if let Ok(c) = unsafe { LoadCursorW(None, cursor) } {
                    unsafe { SetCursor(Some(c)) };
                }
                Some(LRESULT(1))
            }
            WM_LBUTTONDOWN => {
                let (x, y) = (lo(lp.0) as f32, hi(lp.0) as f32);
                // マウスの捕まえは WM_CAPTURECHANGED を送り返すので、借りの外で行う
                self.post.push(Post::Capture(self.hwnd, true));
                let target = self.rt.cx.hit(x, y).map(|h| h.id);
                log::event("click", &format!("x={x} y={y}"), &format!("target={target:?}"));
                self.push(Event::PointerDown { x, y, shift: mods_now().shift });
                Some(LRESULT(0))
            }
            WM_LBUTTONUP => {
                let (x, y) = (lo(lp.0) as f32, hi(lp.0) as f32);
                self.post.push(Post::Capture(self.hwnd, false));
                // 引っ張りの禁止の形を戻す（離した後、ポインターを動かすまで WM_SETCURSOR が来ない）
                if self.rt.cx.dnd.is_some() || self.drag_cursor.is_some() {
                    set_cursor(IDC_ARROW);
                }
                self.drag_cursor = None;
                self.push(Event::PointerUp { x, y });
                Some(LRESULT(0))
            }
            // 右クリック。キーボードから来た物（位置が -1）は、Shift+F10 とアプリケーションキーをキーとして受けたので捨てる
            WM_CONTEXTMENU => {
                if lo(lp.0) == -1 && hi(lp.0) == -1 {
                    return Some(LRESULT(0));
                }
                let mut p = POINT { x: lo(lp.0), y: hi(lp.0) };
                unsafe {
                    let _ = ScreenToClient(self.hwnd, &mut p);
                }
                let (x, y) = (p.x as f32, p.y as f32);
                // 窓の枠（題の帯）の上なら、OS の窓のメニューに任せる
                if x < 0.0 || y < 0.0 || x >= self.rt.cx.width || y >= self.rt.cx.height {
                    return None;
                }
                log::event("context_menu", &format!("x={x} y={y}"), "sent");
                self.push(Event::ContextMenu { x, y });
                Some(LRESULT(0))
            }
            WM_MOUSEWHEEL => {
                let delta = hi(wp.0 as isize) as f32;
                let mut p = POINT { x: lo(lp.0), y: hi(lp.0) };
                unsafe {
                    let _ = ScreenToClient(self.hwnd, &mut p);
                }
                log::event("wheel", &format!("delta={delta} x={} y={}", p.x, p.y), "ok");
                self.push(Event::Wheel { x: p.x as f32, y: p.y as f32, dy: -delta / 120.0 * WHEEL_STEP });
                Some(LRESULT(0))
            }
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let vk = VIRTUAL_KEY(wp.0 as u16);
                if vk == VK_PROCESSKEY {
                    log::event("key", &format!("vk=0x{:02X} composing={}", vk.0, self.composing), "ime (ignored)");
                    return Some(LRESULT(0));
                }
                match key_of(vk) {
                    Some(key) => {
                        let mods = mods_now();
                        log::event("key", &format!("key={key:?} mods={mods:?} composing={}", self.composing), "sent");
                        self.push(Event::Key { key, mods });
                        // Alt+F4（閉じる）と、メニューバーの無い窓の F10（窓のメニュー）は、アプリに知らせた上で OS の
                        // 既定の動きも残す。Shift+F10 はキーとして右クリックのメニューを開くので、OS には渡さない
                        let f10 = key == Key::F10 && !mods.shift && self.rt.cx.prev.menubar.is_empty();
                        if msg == WM_SYSKEYDOWN && (key == Key::F4 || f10) { None } else { Some(LRESULT(0)) }
                    }
                    None => None,
                }
            }
            // Alt と文字はアプリのショートカットとして WM_SYSKEYDOWN で受けた。メニューが無いので OS が鳴らす
            // 警告音を止める（Alt+Space の窓のメニューは残す）
            WM_SYSCHAR if wp.0 != 0x20 => Some(LRESULT(0)),
            WM_TIMER if wp.0 == WAKE_TIMER => {
                unsafe {
                    let _ = KillTimer(Some(self.hwnd), WAKE_TIMER);
                }
                self.wake_timer = false;
                log::event("timer", "", "frame");
                self.need_frame = true;
                Some(LRESULT(0))
            }
            WM_APP_WAKE => {
                self.rt.cx.waker.take_woken();
                log::event("wake", "from=waker", "frame");
                self.need_frame = true;
                Some(LRESULT(0))
            }
            WM_CHAR => {
                let unit = wp.0 as u16;
                if (0xD800..0xDC00).contains(&unit) {
                    self.high_surrogate = Some(unit);
                    return Some(LRESULT(0));
                }
                let units: Vec<u16> = match self.high_surrogate.take() {
                    Some(h) if (0xDC00..0xE000).contains(&unit) => vec![h, unit],
                    _ => vec![unit],
                };
                // 制御文字（Backspace・Enter・Tab・Ctrl+文字）は WM_KEYDOWN で受ける
                if let Some(c) = String::from_utf16_lossy(&units).chars().next().filter(|c| !c.is_control()) {
                    log::event("char", &format!("u=0x{unit:04X}"), "insert");
                    self.push(Event::Char(c));
                }
                Some(LRESULT(0))
            }
            WM_IME_STARTCOMPOSITION => {
                self.composing = true;
                self.last_caret = None;
                // 変換の前の問い合わせ（再変換など）の数を記録して、変換の中の数え直しを始める
                let [pos, feed, reconv, confirm, other] = std::mem::take(&mut self.ime_requests);
                log::event("ime.start", &format!("before charpos={pos} feed={feed} reconvert={reconv} confirm={confirm} other={other}"), "ok");
                self.need_frame = true;
                // DefWindowProc に渡さない = IME の既定の変換中の窓を出さない（入力欄の中に描く）
                Some(LRESULT(0))
            }
            WM_IME_COMPOSITION => {
                let flags = lp.0 as u32;
                if flags & GCS_RESULTSTR.0 != 0 {
                    if let Some(s) = self.ime_string(GCS_RESULTSTR) {
                        log::event("ime.commit", &format!("text={s}"), "sent");
                        self.push(Event::ImePreedit(String::new()));
                        self.push(Event::ImeCommit(s));
                    }
                }
                if flags & GCS_COMPSTR.0 != 0 {
                    if let Some(s) = self.ime_string(GCS_COMPSTR) {
                        // 文節の境目（u32 の並び）・文字ごとの属性・キャレットの位置も読む（文節ごとの下線に使う）
                        let len16 = s.encode_utf16().count();
                        let raw: Vec<u32> = self
                            .ime_raw(GCS_COMPCLAUSE)
                            .unwrap_or_default()
                            .chunks_exact(4)
                            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                            .collect();
                        let bounds = ime::clause_bounds(&raw, len16);
                        let target = ime::target_range(&self.ime_raw(GCS_COMPATTR).unwrap_or_default());
                        let cursor = self.ime_cursor();
                        log::event("ime.preedit", &format!("text={s}"), &format!("shown clauses={bounds:?} target={target:?} cursor={cursor:?}"));
                        self.push(Event::ImePreedit(s));
                        self.push(Event::ImeClauses { bounds, target, cursor });
                    }
                }
                Some(LRESULT(0))
            }
            WM_IME_ENDCOMPOSITION => {
                self.composing = false;
                let [pos, feed, reconv, confirm, other] = std::mem::take(&mut self.ime_requests);
                log::event("ime.end", &format!("requests charpos={pos} feed={feed} reconvert={reconv} confirm={confirm} other={other}"), "ok");
                self.push(Event::ImePreedit(String::new()));
                Some(LRESULT(0))
            }
            WM_IME_CHAR => Some(LRESULT(0)),
            WM_IME_REQUEST => {
                let imr = wp.0 as u32;
                // 種類ごとに数える。再変換の 2 つは必ず記録し、ほかは種類ごとに最初の数件だけ記録する
                let kind = match imr {
                    IMR_QUERYCHARPOSITION => 0,
                    IMR_DOCUMENTFEED => 1,
                    IMR_RECONVERTSTRING => 2,
                    IMR_CONFIRMRECONVERTSTRING => 3,
                    _ => 4,
                };
                self.ime_requests[kind] += 1;
                let log_this = kind == 2 || kind == 3 || self.ime_requests[kind] <= IME_REQUEST_LOG_MAX;
                if imr == IMR_QUERYCHARPOSITION && lp.0 != 0 {
                    let info = unsafe { &mut *(lp.0 as *mut IMECHARPOSITION) };
                    let ok = self.answer_char_position(info);
                    if log_this {
                        log::event(
                            "ime.request",
                            &format!("imr=QUERYCHARPOSITION char={}", info.dwCharPos),
                            &format!("answered={ok} pt={},{}", info.pt.x, info.pt.y),
                        );
                    }
                    return ok.then_some(LRESULT(1));
                }
                if imr == IMR_DOCUMENTFEED || imr == IMR_RECONVERTSTRING {
                    let name = if imr == IMR_DOCUMENTFEED { "DOCUMENTFEED" } else { "RECONVERTSTRING" };
                    let preedit = &self.rt.cx.preedit;
                    let answer = self.rt.cx.ime_field.as_ref().and_then(|f| {
                        if imr == IMR_DOCUMENTFEED { Some(ime::document_feed(f, preedit)) } else { ime::reconvert(f, preedit) }
                    });
                    let Some(r) = answer else {
                        if log_this {
                            log::event("ime.request", &format!("imr={name}"), "answered=false");
                        }
                        return Some(LRESULT(0));
                    };
                    let need = r.size();
                    // 1 回目は大きさだけを聞かれる（lParam が 0）。2 回目に IME が用意した場所へ書く
                    let wrote = lp.0 != 0 && unsafe { write_reconvert(lp.0 as *mut RECONVERTSTRING, &r) };
                    if log_this {
                        log::event(
                            "ime.request",
                            &format!("imr={name} buffer={}", lp.0 != 0),
                            &format!("size={need} wrote={wrote} len={} comp={:?}", r.text.len(), r.comp),
                        );
                    }
                    if lp.0 != 0 && !wrote {
                        return Some(LRESULT(0));
                    }
                    return Some(LRESULT(need as isize));
                }
                if imr == IMR_CONFIRMRECONVERTSTRING && lp.0 != 0 {
                    let rs = unsafe { &*(lp.0 as *const RECONVERTSTRING) };
                    let Some(f) = self.rt.cx.ime_field.as_ref() else { return Some(LRESULT(0)) };
                    let (start, end) = ime::confirmed_range(&f.value, rs.dwCompStrOffset, rs.dwCompStrLen);
                    log::event("ime.request", "imr=CONFIRMRECONVERTSTRING", &format!("select={start}..{end}"));
                    self.push(Event::ImeSelect { start, end });
                    return Some(LRESULT(1));
                }
                if log_this {
                    log::event("ime.request", &format!("imr=0x{imr:02X}"), "default");
                }
                None
            }
            WM_APP_A11Y => {
                self.apply_a11y_actions();
                Some(LRESULT(0))
            }
            _ => None,
        }
    }
}

/// 環境変数 KARGUI_TRACE=1 のとき、届いたメッセージを最初の 400 件だけ記録する（不具合を調べるため）
static TRACE_LEFT: AtomicU32 = AtomicU32::new(0);

extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if TRACE_LEFT.load(Ordering::Relaxed) > 0 {
        TRACE_LEFT.fetch_sub(1, Ordering::Relaxed);
        let borrowed = STATE.with(|s| s.try_borrow().is_err());
        log::event("msg", &format!("msg=0x{msg:04X} wp=0x{:X} lp=0x{:X}", wp.0, lp.0), &format!("borrowed={borrowed}"));
        if msg == WM_WINDOWPOSCHANGING {
            let pos = unsafe { &*(lp.0 as *const WINDOWPOS) };
            let bt = std::backtrace::Backtrace::force_capture().to_string();
            let frames: Vec<&str> = bt.lines().filter(|l| !l.trim_start().starts_with("at ")).take(24).collect();
            log::event("msg.windowpos", &format!("flags=0x{:X} x={} y={} cx={} cy={}", pos.flags.0, pos.x, pos.y, pos.cx, pos.cy), &frames.join(" | "));
        }
    }
    match msg {
        // 状態を使わない物は、借りる前に答える（BeginPaint などの中から入れ子で届くため）
        WM_ERASEBKGND => return LRESULT(1),
        WM_NCPAINT | WM_CAPTURECHANGED => return unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
        WM_IME_SETCONTEXT => {
            // IME の既定の変換中の窓を出さない（変換中の文字は入力欄の中に描く）
            let lp = LPARAM(((lp.0 as u32) & !ISC_SHOWUICOMPOSITIONWINDOW) as i32 as isize);
            return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
        }
        WM_GETOBJECT => {
            let n = GETOBJECT_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            if n <= 5 || n % 100 == 0 {
                log::event("a11y.getobject", &format!("objid={} n={n}", lp.0 as i32), "received");
            }
            // 借りの中で木を作って渡し、UIA への受け渡し（入れ子のメッセージが起きうる）は借りの外で行う
            let reply = with_window(hwnd, |st| {
                let mut initial = Initial(Some(st.rt.a11y().0));
                st.adapter.as_mut().and_then(|a| a.handle_wm_getobject(wp, lp, &mut initial))
            })
            .flatten();
            if let Some(r) = reply {
                return r.into();
            }
            return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
        }
        _ => {}
    }
    let handled = with_window(hwnd, |st| st.handle(msg, wp, lp));
    drain_post();
    match handled {
        Some(Some(r)) => r,
        Some(None) => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
        None => {
            if STATE.with(|s| s.try_borrow().is_err()) {
                log::event("wndproc.reentrant", &format!("msg=0x{msg:04X}"), "default");
            }
            unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
        }
    }
}

fn dispatch_pending(msg: &mut MSG) -> bool {
    while unsafe { PeekMessageW(msg, None, 0, 0, PM_REMOVE) }.as_bool() {
        if msg.message == WM_QUIT {
            return false;
        }
        unsafe {
            let _ = TranslateMessage(msg);
            DispatchMessageW(msg);
        }
    }
    true
}

/// 窓を 1 つ作る（主の窓は key が None、Ui::window の窓はそのキー）。描く物・読み上げ・起こし役を用意した状態を返す。
/// 見せるのは呼んだ側（ShowWindow）
#[allow(clippy::too_many_arguments)]
fn create_window(
    key: Option<String>,
    title: &str,
    size: (f32, f32),
    position: Option<(f32, f32)>,
    renderer: Renderer,
    dark: bool,
    full_rate: bool,
    icon_image: Option<crate::Image>,
    min_size: Option<(f32, f32)>,
    app: Rc<RefCell<Box<dyn App>>>,
    started: Instant,
) -> Result<State, crate::Error> {
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None)? }.into();
    // 位置（論理ピクセル）と大きさを、そのモニターの拡大率で物理ピクセルにする
    let (lx, ly) = position.map_or((CW_USEDEFAULT, CW_USEDEFAULT), |(x, y)| (x as i32, y as i32));
    let monitor = unsafe { MonitorFromPoint(POINT { x: lx.max(-32000), y: ly.max(-32000) }, MONITOR_DEFAULTTOPRIMARY) };
    let (mut dx, mut dy) = (96u32, 96u32);
    unsafe {
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
    }
    let scale = dx as f32 / 96.0;
    let style = WS_OVERLAPPEDWINDOW;
    let mut r = RECT { left: 0, top: 0, right: (size.0 * scale) as i32, bottom: (size.1 * scale) as i32 };
    unsafe {
        let _ = AdjustWindowRectExForDpi(&mut r, style, false, WINDOW_EX_STYLE(0), dx);
    }
    let wide = HSTRING::from(title);
    let hwnd = unsafe {
        CreateWindowExW(WINDOW_EX_STYLE(0), w!("kargui-window"), &wide, style, lx, ly, r.right - r.left, r.bottom - r.top, None, None, Some(instance), None)?
    };
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
    let mut client = RECT::default();
    unsafe { GetClientRect(hwnd, &mut client)? };
    let (cw, ch) = (client.right - client.left, client.bottom - client.top);
    // 窓のアイコン（題の帯とタスクバー）
    let icon = icon_image.as_ref().and_then(make_icon);
    if let Some(i) = icon {
        unsafe {
            SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_BIG as usize)), Some(LPARAM(i.0 as isize)));
            SendMessageW(hwnd, WM_SETICON, Some(WPARAM(ICON_SMALL as usize)), Some(LPARAM(i.0 as isize)));
        }
    }
    log::event("window.icon", &format!("key={key:?}"), &format!("set={}", icon.is_some()));
    log::event(
        "window.create",
        &format!("key={key:?} pos={position:?} dpi={dpi} client={cw}x{ch} renderer={renderer:?}"),
        &format!("ms={:.1}", started.elapsed().as_secs_f64() * 1000.0),
    );
    let text = Text::new()?;
    let t_surface = Instant::now();
    let surface = match renderer {
        Renderer::Gpu => Surface::Gpu(Gpu::new(hwnd, cw as u32, ch as u32, ATLAS_SIZE)?),
        Renderer::Cpu => Surface::Cpu(Gdi::new(hwnd, cw, ch)?),
    };
    log::event("surface.init", &format!("renderer={renderer:?}"), &format!("ms={:.1}", t_surface.elapsed().as_secs_f64() * 1000.0));
    let mut rt = Runtime::new(text, dark, Box::new(SystemClipboard { hwnd: hwnd.0 as isize }), title.to_string());
    // ほかのスレッドから起こされたら、窓へメッセージを送る（窓の番号は数として渡す）
    let target = hwnd.0 as isize;
    rt.cx.waker = crate::wake::Waker::new(move || unsafe {
        let _ = PostMessageW(Some(HWND(target as *mut _)), WM_APP_WAKE, WPARAM(0), LPARAM(0));
    });
    rt.resize(cw as f32, ch as f32, dpi as f32 / 96.0);
    rt.cx.reduce_motion = system_reduce_motion();
    log::event("motion.setting", &format!("key={key:?}"), &format!("reduce={}", rt.cx.reduce_motion));
    let actions = Arc::new(Mutex::new(VecDeque::new()));
    let focused = unsafe { GetForegroundWindow() } == hwnd;
    let adapter = Adapter::new(hwnd, focused, ActionSink { hwnd: hwnd.0 as isize, queue: actions.clone() });
    Ok(State {
        hwnd,
        key,
        app,
        renderer,
        open_windows: Vec::new(),
        synced: false,
        icon,
        icon_image,
        min_size,
        rt,
        surface: Some(surface),
        adapter: Some(adapter),
        actions,
        post: Vec::new(),
        events: Vec::new(),
        need_frame: true,
        wake_timer: false,
        animating: false,
        started,
        shown: false,
        full_rate,
        window_focused: focused,
        composing: false,
        high_surrogate: None,
        last_caret: None,
        ime_requests: [0; 5],
        summary: Summary::default(),
        background: [0.0; 4],
        dark_title: None,
        tracking_leave: false,
        drag_cursor: None,
        file_drop: None,
        file_drop_asked: false,
        last_tick: None,
        t_build: 0.0,
        t_draw: 0.0,
        t_wait: 0.0,
        slow_frames: 0,
        anim: AnimStats::default(),
    })
}

pub(crate) fn run(options: Options, app: Box<dyn App>) -> Result<(), crate::Error> {
    let started = Instant::now();
    if std::env::var_os("KARGUI_TRACE").is_some() {
        TRACE_LEFT.store(400, Ordering::Relaxed);
    }
    unsafe {
        if let Err(e) = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) {
            log::event("dpi.awareness", "", &format!("error={e:?}"));
        }
    }
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None)? }.into();
    let class = w!("kargui-window");
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(wndproc),
        hInstance: instance,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW)? },
        lpszClassName: class,
        ..Default::default()
    };
    if unsafe { RegisterClassExW(&wc) } == 0 {
        log::event("window.class", "", "error=RegisterClassExW");
    }
    let app = Rc::new(RefCell::new(app));
    let dark = options.dark.unwrap_or_else(system_dark);
    let main = create_window(None, &options.title, options.size, options.position, options.renderer, dark, options.full_rate, options.icon.clone(), options.min_size, app, started)?;
    let hwnd = main.hwnd;
    STATE.with(|s| s.borrow_mut().push(main));

    unsafe {
        let _ = ShowWindow(hwnd, if options.activate { SW_SHOW } else { SW_SHOWNOACTIVATE });
    }
    drain_post();
    // 1 枚目を描いて出し、画面の合成が終わるのを待ってから、アプリに「表示できた」と知らせる
    with_state(|st| st.frame("first"));
    drain_post();
    unsafe {
        let _ = DwmFlush();
    }
    with_state(|st| {
        st.shown = true;
        st.app.borrow_mut().presented(&Frame { number: 0, since_start: st.started.elapsed() });
    });

    let mut msg = MSG::default();
    // 測定で比べるため、環境変数 KARGUI_MMCSS=0 で登録しないようにできる
    let mut boost = Boost(None, std::env::var("KARGUI_MMCSS").as_deref() != Ok("0"));
    loop {
        let animating = with_windows(|ws| ws.iter().any(|w| w.animating)).unwrap_or(false);
        boost.set(animating);
        if animating {
            if !dispatch_pending(&mut msg) {
                break;
            }
            with_windows(|ws| {
                for w in ws.iter_mut().filter(|w| w.animating) {
                    w.tick();
                }
            });
            drain_post();
            with_state(|st| st.wait_vblank());
        } else {
            with_windows(|ws| {
                for w in ws.iter_mut() {
                    w.last_tick = None;
                    w.end_animation();
                }
            });
            let r = unsafe { GetMessageW(&mut msg, None, 0, 0) };
            if r.0 == 0 || r.0 == -1 {
                break;
            }
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if !dispatch_pending(&mut msg) {
                break;
            }
            frame_pending();
        }
    }
    boost.set(false);
    STATE.with(|s| s.borrow_mut().clear());
    log::event("loop.end", "", "ok");
    Ok(())
}

/// 描き直しを頼まれた窓を描く。入力で描いた窓があれば、ほかの窓も 1 回描き直す（アプリを共有するため）
fn frame_pending() {
    loop {
        with_windows(|ws| {
            for w in ws.iter_mut().filter(|w| w.need_frame) {
                let why = if std::mem::take(&mut w.synced) { "sync" } else { "input" };
                w.frame(why);
            }
        });
        drain_post();
        if !with_windows(|ws| ws.iter().any(|w| w.need_frame)).unwrap_or(false) {
            break;
        }
    }
}

/// アニメーションの間だけ、このスレッドを MMCSS（動画・ゲーム向けに優先度を上げる Windows の仕組み）に登録する。
/// 垂直同期の待ちから起こされた時に、同じコアのほかのスレッドより先に動けるようにする
/// （1 コアの条件で、待ちから起こされるのが 1 フレーム以上遅れて途切れていた）
struct Boost(Option<HANDLE>, bool);

impl Boost {
    fn set(&mut self, on: bool) {
        match (on && self.1, self.0) {
            (true, None) => {
                let mut task = 0u32;
                match unsafe { AvSetMmThreadCharacteristicsW(w!("Games"), &mut task) } {
                    Ok(h) => {
                        self.0 = Some(h);
                        log::event("mmcss.on", "task=Games", &format!("index={task}"));
                    }
                    Err(e) => {
                        // 登録できない時は、待ちから起こされたら先に動けるよう普通の優先度より 1 つ上げる
                        let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL) };
                        self.0 = Some(HANDLE::default());
                        log::event("mmcss.on", "task=Games", &format!("failed={e:?} fallback=above_normal"));
                    }
                }
            }
            (false, Some(h)) => {
                if h.is_invalid() {
                    let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_NORMAL) };
                } else {
                    let _ = unsafe { AvRevertMmThreadCharacteristics(h) };
                }
                self.0 = None;
                log::event("mmcss.off", "", "ok");
            }
            _ => {}
        }
    }
}

/// IME が用意した場所に RECONVERTSTRING を書く。場所が足りなければ書かずに false
///
/// # Safety
/// `rs` は IME が渡した RECONVERTSTRING を指し、その dwSize バイトを書けること
unsafe fn write_reconvert(rs: *mut RECONVERTSTRING, r: &ime::Reconvert) -> bool {
    let need = r.size();
    let rs = unsafe { &mut *rs };
    if (rs.dwSize as usize) < need {
        return false;
    }
    rs.dwVersion = 0;
    rs.dwStrLen = r.text.len() as u32;
    rs.dwStrOffset = ime::HEADER as u32;
    rs.dwCompStrLen = r.comp.1 as u32;
    rs.dwCompStrOffset = (r.comp.0 * 2) as u32;
    rs.dwTargetStrLen = r.target.1 as u32;
    rs.dwTargetStrOffset = (r.target.0 * 2) as u32;
    unsafe {
        let text = (rs as *mut RECONVERTSTRING as *mut u8).add(ime::HEADER) as *mut u16;
        std::ptr::copy_nonoverlapping(r.text.as_ptr(), text, r.text.len());
        *text.add(r.text.len()) = 0;
    }
    true
}

/// アニメーション 1 回分の時間の合計
#[derive(Default)]
struct AnimStats {
    frames: u32,
    build_ms: f64,
    draw_ms: f64,
    max_ms: f64,
}
