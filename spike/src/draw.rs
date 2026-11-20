//! 描く物の一覧。GPU の道（gpu.rs）も CPU の道（raster.rs）も、同じ一覧を受け取る。
//! 座標はすべて窓の中の画素（物理ピクセル）。位置と大きさは整数の画素にそろえてから入れる。

/// 色（乗算していない RGBA、0〜1）
pub type Color = [f32; 4];

/// 0xRRGGBB を色にする
pub const fn rgb(hex: u32) -> Color {
    [
        ((hex >> 16) & 0xff) as f32 / 255.0,
        ((hex >> 8) & 0xff) as f32 / 255.0,
        (hex & 0xff) as f32 / 255.0,
        1.0,
    ]
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }
    /// 四辺を d だけ内側に寄せる
    pub fn inset(&self, d: f32) -> Rect {
        Rect::new(self.x + d, self.y + d, (self.w - 2.0 * d).max(0.0), (self.h - 2.0 * d).max(0.0))
    }
    /// 位置と大きさを整数の画素にそろえる
    pub fn snapped(&self) -> Rect {
        let x = self.x.round();
        let y = self.y.round();
        Rect::new(x, y, (self.right().round() - x).max(0.0), (self.bottom().round() - y).max(0.0))
    }
}

/// 整数の画素の四角。描き直す範囲に使う。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl IRect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    /// 重なる部分。重ならなければ None
    pub fn intersect(&self, o: &IRect) -> Option<IRect> {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = self.right().min(o.right());
        let y1 = self.bottom().min(o.bottom());
        (x1 > x0 && y1 > y0).then(|| IRect::new(x0, y0, x1 - x0, y1 - y0))
    }
    /// 小数の四角を、外側へ広げて整数の画素にする
    pub fn covering(r: Rect) -> IRect {
        let x0 = r.x.floor() as i32;
        let y0 = r.y.floor() as i32;
        let x1 = r.right().ceil() as i32;
        let y1 = r.bottom().ceil() as i32;
        IRect::new(x0, y0, (x1 - x0).max(0), (y1 - y0).max(0))
    }
}

/// 描く物 1 つ。GPU の頂点の入力と同じ並び（64 バイト）。
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Quad {
    /// x, y, 幅, 高さ（画素）
    pub rect: [f32; 4],
    /// 色（乗算していない RGBA）
    pub color: [f32; 4],
    /// 文字の形の表の中の位置 x, y, 幅, 高さ（画素）。幅が 0 なら塗りつぶし
    pub uv: [f32; 4],
    /// x: 角の丸み（画素）。残りは使っていない
    pub extra: [f32; 4],
}

impl Quad {
    pub fn is_glyph(&self) -> bool {
        self.uv[2] > 0.0
    }
    pub fn bounds(&self) -> Rect {
        Rect::new(self.rect[0], self.rect[1], self.rect[2], self.rect[3])
    }
}

#[derive(Default)]
pub struct DrawList {
    pub quads: Vec<Quad>,
}

impl DrawList {
    pub fn clear(&mut self) {
        self.quads.clear();
    }

    /// 塗りつぶした四角（radius > 0 なら角を丸める）
    pub fn fill(&mut self, r: Rect, color: Color, radius: f32) {
        let r = r.snapped();
        if r.w <= 0.0 || r.h <= 0.0 || color[3] <= 0.0 {
            return;
        }
        let radius = radius.min(r.w * 0.5).min(r.h * 0.5).max(0.0);
        self.quads.push(Quad {
            rect: [r.x, r.y, r.w, r.h],
            color,
            uv: [0.0; 4],
            extra: [radius, 0.0, 0.0, 0.0],
        });
    }

    /// 枠付きの四角。外側を枠の色、内側を塗りの色で 2 回描く
    pub fn boxed(&mut self, r: Rect, fill: Color, border: Color, width: f32, radius: f32) {
        self.fill(r, border, radius);
        self.fill(r.inset(width), fill, (radius - width).max(0.0));
    }

    /// 文字の形 1 つ。x, y は形の左上（整数の画素）、ax, ay は表の中の位置
    pub fn glyph(&mut self, x: f32, y: f32, w: f32, h: f32, ax: f32, ay: f32, color: Color) {
        self.quads.push(Quad {
            rect: [x, y, w, h],
            color,
            uv: [ax, ay, w, h],
            extra: [0.0; 4],
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersect_keeps_only_the_overlap() {
        let a = IRect::new(0, 0, 100, 50);
        assert_eq!(a.intersect(&IRect::new(90, 40, 30, 30)), Some(IRect::new(90, 40, 10, 10)));
        assert_eq!(a.intersect(&IRect::new(100, 0, 10, 10)), None);
        assert_eq!(a.intersect(&IRect::new(-5, -5, 5, 100)), None);
    }

    #[test]
    fn covering_grows_outward() {
        assert_eq!(IRect::covering(Rect::new(1.2, 2.8, 3.0, 1.0)), IRect::new(1, 2, 4, 2));
        assert_eq!(IRect::covering(Rect::new(4.0, 5.0, 2.0, 3.0)), IRect::new(4, 5, 2, 3));
    }

    #[test]
    fn fill_snaps_and_limits_radius() {
        let mut d = DrawList::default();
        d.fill(Rect::new(10.4, 10.6, 20.0, 8.0), [0.0, 0.0, 0.0, 1.0], 100.0);
        let q = d.quads[0];
        assert_eq!(q.rect, [10.0, 11.0, 20.0, 8.0]);
        // 角の丸みは短い辺の半分まで
        assert_eq!(q.extra[0], 4.0);
        // 透明な物と大きさ 0 の物は入れない
        d.fill(Rect::new(0.0, 0.0, 10.0, 10.0), [0.0, 0.0, 0.0, 0.0], 0.0);
        d.fill(Rect::new(0.0, 0.0, 0.2, 10.0), [0.0, 0.0, 0.0, 1.0], 0.0);
        assert_eq!(d.quads.len(), 1);
    }
}
