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
        for p in rgba.chunks_exact_mut(4) {
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
}
