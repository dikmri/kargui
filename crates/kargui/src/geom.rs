//! Rectangles and colors.
//!
//! 座標は窓の中の物理ピクセル。部品の大きさを決めるときは論理ピクセル × 拡大率で計算する。

/// A color as straight (not premultiplied) RGBA in the range 0.0 to 1.0.
pub type Color = [f32; 4];

/// Builds an opaque [`Color`] from `0xRRGGBB`.
///
/// ```
/// let red = kargui::rgb(0xff0000);
/// assert_eq!(red, [1.0, 0.0, 0.0, 1.0]);
/// ```
pub const fn rgb(hex: u32) -> Color {
    [
        ((hex >> 16) & 0xff) as f32 / 255.0,
        ((hex >> 8) & 0xff) as f32 / 255.0,
        (hex & 0xff) as f32 / 255.0,
        1.0,
    ]
}

/// A rectangle in physical pixels: position of the top-left corner and size.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl Rect {
    /// Creates a rectangle.
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    /// Right edge (`x + w`).
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    /// Bottom edge (`y + h`).
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    /// Whether the point is inside (right and bottom edges excluded).
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }
    /// Moves every edge inward by `d` (outward when `d` is negative).
    pub fn inset(&self, d: f32) -> Rect {
        Rect::new(self.x + d, self.y + d, (self.w - 2.0 * d).max(0.0), (self.h - 2.0 * d).max(0.0))
    }
    /// Rounds the edges to whole pixels.
    pub fn snapped(&self) -> Rect {
        let x = self.x.round();
        let y = self.y.round();
        Rect::new(x, y, (self.right().round() - x).max(0.0), (self.bottom().round() - y).max(0.0))
    }
    /// Moves the rectangle.
    pub fn translate(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }
}

/// A rectangle in whole pixels. Used for clipping and for the areas to redraw.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct IRect {
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// Width.
    pub w: i32,
    /// Height.
    pub h: i32,
}

impl IRect {
    /// Creates a rectangle.
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
    /// Right edge (`x + w`).
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    /// Bottom edge (`y + h`).
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    /// Whether the rectangle has no area.
    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }
    /// The overlapping part, or `None` when the rectangles do not overlap.
    pub fn intersect(&self, o: &IRect) -> Option<IRect> {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = self.right().min(o.right());
        let y1 = self.bottom().min(o.bottom());
        (x1 > x0 && y1 > y0).then(|| IRect::new(x0, y0, x1 - x0, y1 - y0))
    }
    /// The smallest rectangle that contains both.
    pub fn union(&self, o: &IRect) -> IRect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        let x0 = self.x.min(o.x);
        let y0 = self.y.min(o.y);
        let x1 = self.right().max(o.right());
        let y1 = self.bottom().max(o.bottom());
        IRect::new(x0, y0, x1 - x0, y1 - y0)
    }
    /// The smallest whole-pixel rectangle that covers `r`.
    pub fn covering(r: Rect) -> IRect {
        let x0 = r.x.floor() as i32;
        let y0 = r.y.floor() as i32;
        let x1 = r.right().ceil() as i32;
        let y1 = r.bottom().ceil() as i32;
        IRect::new(x0, y0, (x1 - x0).max(0), (y1 - y0).max(0))
    }
    /// Converts to a [`Rect`].
    pub fn to_rect(&self) -> Rect {
        Rect::new(self.x as f32, self.y as f32, self.w as f32, self.h as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersect_and_union() {
        let a = IRect::new(0, 0, 100, 50);
        assert_eq!(a.intersect(&IRect::new(90, 40, 30, 30)), Some(IRect::new(90, 40, 10, 10)));
        assert_eq!(a.intersect(&IRect::new(100, 0, 10, 10)), None);
        assert_eq!(a.union(&IRect::new(90, 40, 30, 30)), IRect::new(0, 0, 120, 70));
        assert_eq!(IRect::default().union(&a), a);
    }

    #[test]
    fn covering_grows_outward() {
        assert_eq!(IRect::covering(Rect::new(1.2, 2.8, 3.0, 1.0)), IRect::new(1, 2, 4, 2));
    }
}
