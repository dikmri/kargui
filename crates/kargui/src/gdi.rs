//! CPU の道の画面への出し方（GDI）。描いた画素（DIB セクション）を GDI の BitBlt で窓へ写す。
//! 写すのは描き直した範囲だけ。GPU のドライバーは読み込まない。
//! 垂直同期は、窓のあるモニターの DXGI の出力で待つ（GPU の装置は作らない）。取れなければ DwmFlush で待つ。

use std::time::Instant;

use crate::log;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::DwmFlush;
use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, IDXGIOutput};
use windows::Win32::Graphics::Gdi::*;
use windows::core::Result;

use crate::geom::IRect;

pub(crate) struct Gdi {
    hwnd: HWND,
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u32,
    pub width: i32,
    pub height: i32,
    output: Option<IDXGIOutput>,
    output_tried: bool,
}

impl Gdi {
    pub fn new(hwnd: HWND, width: i32, height: i32) -> Result<Self> {
        let t = Instant::now();
        let dc = unsafe { CreateCompatibleDC(None) };
        let mut gdi = Self {
            hwnd,
            dc,
            bitmap: HBITMAP::default(),
            old: HGDIOBJ::default(),
            bits: std::ptr::null_mut(),
            width: 0,
            height: 0,
            output: None,
            output_tried: false,
        };
        gdi.make(width, height)?;
        log::event("cpu.init", &format!("size={width}x{height}"), &format!("ms={:.1}", t.elapsed().as_secs_f64() * 1000.0));
        Ok(gdi)
    }

    fn make(&mut self, width: i32, height: i32) -> Result<()> {
        let (width, height) = (width.max(1), height.max(1));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                // 負の高さ = 上の行から並ぶ
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = unsafe { CreateDIBSection(Some(self.dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)? };
        let old = unsafe { SelectObject(self.dc, bitmap.into()) };
        if !self.bitmap.is_invalid() {
            unsafe {
                let _ = DeleteObject(self.bitmap.into());
            }
        } else {
            self.old = old;
        }
        self.bitmap = bitmap;
        self.bits = bits.cast();
        self.width = width;
        self.height = height;
        Ok(())
    }

    pub fn resize(&mut self, width: i32, height: i32) -> Result<()> {
        if width == self.width && height == self.height {
            return Ok(());
        }
        self.output = None;
        self.output_tried = false;
        self.make(width, height)
    }

    pub fn pixels(&mut self) -> &mut [u32] {
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.width * self.height) as usize) }
    }

    /// 描き直した範囲を窓へ写す
    pub fn present(&self, rects: &[IRect]) {
        unsafe {
            let wdc = GetDC(Some(self.hwnd));
            for r in rects {
                if let Err(e) = BitBlt(wdc, r.x, r.y, r.w, r.h, Some(self.dc), r.x, r.y, SRCCOPY) {
                    log::event("cpu.present", &format!("rect={r:?}"), &format!("error={e:?}"));
                }
            }
            ReleaseDC(Some(self.hwnd), wdc);
        }
    }

    /// WM_PAINT: 隠れていた部分を描いた画素から写し直す
    pub fn paint(&self) {
        unsafe {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(self.hwnd, &mut ps);
            let r = ps.rcPaint;
            let _ = BitBlt(hdc, r.left, r.top, r.right - r.left, r.bottom - r.top, Some(self.dc), r.left, r.top, SRCCOPY);
            let _ = EndPaint(self.hwnd, &ps);
        }
    }

    /// 窓のあるモニターの垂直同期を待つ
    pub fn wait_vblank(&mut self) {
        if self.output.is_none() && !self.output_tried {
            self.output_tried = true;
            self.output = find_output(self.hwnd);
            log::event("cpu.vblank", "find output", if self.output.is_some() { "dxgi" } else { "dwmflush" });
        }
        let ok = self.output.as_ref().is_some_and(|o| unsafe { o.WaitForVBlank() }.is_ok());
        if !ok {
            let _ = unsafe { DwmFlush() };
        }
    }

    pub fn forget_output(&mut self) {
        self.output = None;
        self.output_tried = false;
    }
}

/// 窓のあるモニターに当たる DXGI の出力を探す
fn find_output(hwnd: HWND) -> Option<IDXGIOutput> {
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let factory: IDXGIFactory1 = CreateDXGIFactory1().ok()?;
        for a in 0.. {
            let adapter = factory.EnumAdapters1(a).ok()?;
            for o in 0.. {
                let Ok(output) = adapter.EnumOutputs(o) else { break };
                if output.GetDesc().is_ok_and(|d| d.Monitor == monitor) {
                    return Some(output);
                }
            }
        }
        None
    }
}

impl Drop for Gdi {
    fn drop(&mut self) {
        unsafe {
            if !self.old.is_invalid() {
                SelectObject(self.dc, self.old);
            }
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}
