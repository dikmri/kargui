//! 画像。画素の並び（乗算済みの RGBA）を持ち、複製は軽い（Arc）。同じかどうかは作った時の番号で比べる。
//! ファイルの形式（PNG・JPEG など）を読むのは OS の道（Windows は wic.rs）。

use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::Error;

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) struct ImageData {
    /// 作った時の番号（描く物の一覧と、変わった所を見つける仕組みが使う）
    pub id: u64,
    pub width: u32,
    pub height: u32,
    /// 乗算済みの RGBA（1 画素 4 バイト、左上から行ごと）
    pub pixels: Vec<u8>,
}

/// A picture to show with [`Ui::image`](crate::Ui::image). Cloning is cheap (the pixels are
/// shared). Create it once (for example when the app starts) and keep it in your state, not in
/// `view`, so that it is not decoded every frame.
///
/// ```
/// use kargui::Image;
///
/// // a 2 × 1 picture: one red pixel and one half-transparent blue pixel
/// let image = Image::from_rgba(2, 1, vec![255, 0, 0, 255, 0, 0, 255, 128]).unwrap();
/// assert_eq!((image.width(), image.height()), (2, 1));
/// ```
#[derive(Clone)]
pub struct Image(pub(crate) Arc<ImageData>);

impl Image {
    /// A picture from RGBA pixels (4 bytes per pixel, not premultiplied, rows from the top).
    /// Fails if `rgba` does not hold exactly `width × height` pixels or a side is 0.
    ///
    /// ```
    /// use kargui::Image;
    ///
    /// let gray = Image::from_rgba(1, 1, vec![128, 128, 128, 255]).unwrap();
    /// assert_eq!(gray.width(), 1);
    /// assert!(Image::from_rgba(2, 2, vec![0; 4]).is_err());
    /// ```
    pub fn from_rgba(width: u32, height: u32, mut rgba: Vec<u8>) -> Result<Image, Error> {
        if width == 0 || height == 0 || rgba.len() != width as usize * height as usize * 4 {
            return Err(Error(format!("Image::from_rgba: {width} x {height} needs {} bytes, got {}", width as usize * height as usize * 4, rgba.len())));
        }
        for p in rgba.as_chunks_mut::<4>().0 {
            let a = p[3] as u32;
            if a < 255 {
                for c in &mut p[..3] {
                    *c = ((*c as u32 * a + 127) / 255) as u8;
                }
            }
        }
        Ok(Self::premultiplied(width, height, rgba))
    }

    /// 乗算済みの画素から作る（OS の読み込みの結果に使う）
    pub(crate) fn premultiplied(width: u32, height: u32, pixels: Vec<u8>) -> Image {
        Image(Arc::new(ImageData { id: NEXT_ID.fetch_add(1, Ordering::Relaxed), width, height, pixels }))
    }

    /// A picture from the bytes of an image file (PNG, JPEG, GIF, BMP, TIFF and the other
    /// formats Windows can read). On other systems this returns an error for now; use
    /// [`Image::from_rgba`] there.
    ///
    /// ```no_run
    /// let logo = kargui::Image::from_bytes(include_bytes!("../../../docs/shots/button-light.png")).unwrap();
    /// assert!(logo.width() > 0);
    /// ```
    pub fn from_bytes(bytes: &[u8]) -> Result<Image, Error> {
        #[cfg(windows)]
        {
            let (w, h, px) = crate::wic::decode(bytes)?;
            Ok(Self::premultiplied(w, h, px))
        }
        #[cfg(not(windows))]
        {
            let _ = bytes;
            Err(Error("Image::from_bytes is only available on Windows yet; use Image::from_rgba".into()))
        }
    }

    /// Reads an image file. See [`Image::from_bytes`] for the formats.
    ///
    /// ```no_run
    /// let photo = kargui::Image::open("photo.jpg").unwrap();
    /// println!("{} x {}", photo.width(), photo.height());
    /// ```
    pub fn open(path: impl AsRef<Path>) -> Result<Image, Error> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|e| Error(format!("could not read {}: {e}", path.display())))?;
        Self::from_bytes(&bytes)
    }

    /// Width in pixels.
    ///
    /// ```
    /// let image = kargui::Image::from_rgba(3, 2, vec![0; 24]).unwrap();
    /// assert_eq!(image.width(), 3);
    /// ```
    pub fn width(&self) -> u32 {
        self.0.width
    }

    /// Height in pixels.
    ///
    /// ```
    /// let image = kargui::Image::from_rgba(3, 2, vec![0; 24]).unwrap();
    /// assert_eq!(image.height(), 2);
    /// ```
    pub fn height(&self) -> u32 {
        self.0.height
    }

    /// 大きさを変えた画像（トレイのアイコンなど、OS が決まった大きさで求める時）。縮める時は元の範囲の平均、
    /// 大きくする時は近い画素を繰り返す。乗算済みのまま計算するので、透明な縁が暗くならない。同じ大きさならそのまま
    pub(crate) fn resized(&self, width: u32, height: u32) -> Image {
        let (sw, sh) = (self.0.width as usize, self.0.height as usize);
        let (tw, th) = (width.max(1) as usize, height.max(1) as usize);
        if (sw, sh) == (tw, th) {
            return self.clone();
        }
        // 目の画素 t が使う元の範囲 [a, b)（少なくとも 1 画素）
        let span = |t: usize, s: usize, n: usize| {
            let a = t * s / n;
            (a, ((t + 1) * s / n).max(a + 1))
        };
        let src = &self.0.pixels;
        let mut out = Vec::with_capacity(tw * th * 4);
        for ty in 0..th {
            let (y0, y1) = span(ty, sh, th);
            for tx in 0..tw {
                let (x0, x1) = span(tx, sw, tw);
                let mut sum = [0u32; 4];
                for y in y0..y1 {
                    for x in x0..x1 {
                        let i = (y * sw + x) * 4;
                        for c in 0..4 {
                            sum[c] += src[i + c] as u32;
                        }
                    }
                }
                let n = ((y1 - y0) * (x1 - x0)) as u32;
                out.extend(sum.map(|v| ((v + n / 2) / n) as u8));
            }
        }
        Image::premultiplied(tw as u32, th as u32, out)
    }
}

impl PartialEq for Image {
    fn eq(&self, other: &Image) -> bool {
        self.0.id == other.0.id
    }
}

impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Image({} x {}, #{})", self.0.width, self.0.height, self.0.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_is_checked_and_premultiplied() {
        assert!(Image::from_rgba(2, 2, vec![0; 15]).is_err());
        assert!(Image::from_rgba(0, 1, vec![]).is_err());
        let img = Image::from_rgba(2, 1, vec![255, 0, 0, 255, 200, 100, 50, 128]).unwrap();
        assert_eq!(&img.0.pixels[..4], &[255, 0, 0, 255]);
        assert_eq!(&img.0.pixels[4..], &[100, 50, 25, 128]);
        // 作るたびに別の画像
        let again = Image::from_rgba(2, 1, vec![255, 0, 0, 255, 200, 100, 50, 128]).unwrap();
        assert_ne!(img, again);
        assert_eq!(img, img.clone());
    }

    #[test]
    fn resizing_averages_when_smaller_and_repeats_when_larger() {
        // 左半分が白・右半分が透明の 4 × 2 を 2 × 1 に: 左は白、右は透明（乗算済みのまま平均する）
        let mut px = Vec::new();
        for _ in 0..2 {
            px.extend([255, 255, 255, 255, 255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0]);
        }
        let img = Image::from_rgba(4, 2, px).unwrap();
        let small = img.resized(2, 1);
        assert_eq!((small.width(), small.height()), (2, 1));
        assert_eq!(small.0.pixels, [255, 255, 255, 255, 0, 0, 0, 0]);
        // 2 × 1 を 1 × 1 に: 白と透明の平均は、乗算済みで半分（赤・緑・青・不透明度とも 128）
        assert_eq!(small.resized(1, 1).0.pixels, [128, 128, 128, 128]);
        // 1 × 1 を 2 × 2 に: 同じ画素を繰り返す
        let dot = Image::from_rgba(1, 1, vec![10, 20, 30, 255]).unwrap();
        assert_eq!(dot.resized(2, 2).0.pixels, [10, 20, 30, 255].repeat(4));
        // 同じ大きさなら同じ画像（作り直さない）
        assert_eq!(dot.resized(1, 1), dot);
    }
}
