//! 画像のファイルを読む Windows の道（WIC: Windows Imaging Component）。依存の crate を増やさずに、
//! Windows が読める形式（PNG・JPEG・GIF・BMP・TIFF・HEIF など、入っている物）を読む。
//!
//! WIC は COM を使う。COM の初期化の種類（STA・MTA）は、アプリのほかの部分（読み上げの UI オートメーションなど）と
//! ぶつかることがあるので、読む時だけ使う別のスレッドで初期化して読む。

use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;

use crate::Error;

/// 画像のファイルの中身を、乗算済みの RGBA の画素にする（幅, 高さ, 画素）
pub(crate) fn decode(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), Error> {
    std::thread::scope(|s| s.spawn(|| decode_here(bytes)).join()).unwrap_or_else(|_| Err(Error("image decoding thread panicked".into())))
}

fn decode_here(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), Error> {
    unsafe {
        let init = CoInitializeEx(None, COINIT_MULTITHREADED);
        let result = (|| -> windows::core::Result<(u32, u32, Vec<u8>)> {
            let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
            let stream = factory.CreateStream()?;
            stream.InitializeFromMemory(bytes)?;
            let decoder = factory.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand)?;
            let frame = decoder.GetFrame(0)?;
            let converter = factory.CreateFormatConverter()?;
            converter.Initialize(&frame, &GUID_WICPixelFormat32bppPRGBA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
            let (mut w, mut h) = (0u32, 0u32);
            converter.GetSize(&mut w, &mut h)?;
            let mut px = vec![0u8; w as usize * h as usize * 4];
            converter.CopyPixels(std::ptr::null(), w * 4, &mut px)?;
            Ok((w, h, px))
        })();
        if init.is_ok() {
            CoUninitialize();
        }
        result.map_err(|e| Error(format!("could not decode the image: {e:?}")))
    }
}
