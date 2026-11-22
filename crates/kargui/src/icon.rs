//! kargui が持つアイコンの図。24 マスの格子の上に、太さ 2 マスの丸い端の線と円で描く（自分で決めた座標で、
//! 既存のアイコン集は写していない）。表示する大きさに拡げてから vector.rs で塗り、文字と同じ形の表に入れる。

use crate::vector::{Pt, Raster, circle, hole, stroke};

/// A built-in icon, drawn as a simple vector shape at any size and in any color. Show one with
/// [`Ui::icon`](crate::Ui::icon) or put one on a button with [`Button::icon`](crate::ui::Button::icon).
/// For other pictures, use an [`Image`](crate::Image).
///
/// ```
/// use kargui::Icon;
/// # kargui::test::example(|ui| {
/// ui.icon(Icon::Search);
/// ui.button("Add").icon(Icon::Plus);
/// # });
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Icon {
    /// A check mark.
    Check,
    /// An X, for closing and removing.
    Close,
    /// A chevron pointing down (drop-downs, expanding).
    ChevronDown,
    /// A chevron pointing up.
    ChevronUp,
    /// A chevron pointing left (going back).
    ChevronLeft,
    /// A chevron pointing right (going forward, submenus).
    ChevronRight,
    /// A plus sign.
    Plus,
    /// A minus sign.
    Minus,
    /// A magnifying glass.
    Search,
    /// Three horizontal lines (a menu).
    Menu,
    /// Three dots (more actions).
    More,
    /// The letter i in a circle.
    Info,
    /// An exclamation mark in a triangle.
    Warning,
    /// An arrow pointing up.
    ArrowUp,
    /// An arrow pointing down.
    ArrowDown,
    /// An arrow pointing left.
    ArrowLeft,
    /// An arrow pointing right.
    ArrowRight,
    /// A calendar page.
    Calendar,
}

impl Icon {
    /// Every icon, for example to show them all in a picker.
    ///
    /// ```
    /// # kargui::test::example(|ui| {
    /// ui.row(|ui| {
    ///     for icon in kargui::Icon::ALL {
    ///         ui.icon(icon);
    ///     }
    /// });
    /// # });
    /// ```
    pub const ALL: [Icon; 18] = [
        Icon::Check,
        Icon::Close,
        Icon::ChevronDown,
        Icon::ChevronUp,
        Icon::ChevronLeft,
        Icon::ChevronRight,
        Icon::Plus,
        Icon::Minus,
        Icon::Search,
        Icon::Menu,
        Icon::More,
        Icon::Info,
        Icon::Warning,
        Icon::ArrowUp,
        Icon::ArrowDown,
        Icon::ArrowLeft,
        Icon::ArrowRight,
        Icon::Calendar,
    ];
}

/// 線の太さ（マス）
const W: f32 = 2.0;

/// 24 マスの格子の上の形（塗る形と穴の形）
fn shapes(icon: Icon) -> Vec<Vec<Pt>> {
    let line = |pts: &[Pt]| stroke(pts, W);
    let ring = |c: Pt, r: f32| vec![circle(c, r + W / 2.0), hole(circle(c, r - W / 2.0))];
    let join = |parts: Vec<Vec<Vec<Pt>>>| parts.into_iter().flatten().collect::<Vec<_>>();
    match icon {
        Icon::Check => line(&[(5.0, 12.5), (10.0, 17.5), (19.0, 7.0)]),
        Icon::Close => join(vec![line(&[(6.0, 6.0), (18.0, 18.0)]), line(&[(18.0, 6.0), (6.0, 18.0)])]),
        Icon::ChevronDown => line(&[(6.0, 9.0), (12.0, 15.0), (18.0, 9.0)]),
        Icon::ChevronUp => line(&[(6.0, 15.0), (12.0, 9.0), (18.0, 15.0)]),
        Icon::ChevronLeft => line(&[(15.0, 6.0), (9.0, 12.0), (15.0, 18.0)]),
        Icon::ChevronRight => line(&[(9.0, 6.0), (15.0, 12.0), (9.0, 18.0)]),
        Icon::Plus => join(vec![line(&[(12.0, 5.0), (12.0, 19.0)]), line(&[(5.0, 12.0), (19.0, 12.0)])]),
        Icon::Minus => line(&[(5.0, 12.0), (19.0, 12.0)]),
        Icon::Search => join(vec![ring((10.5, 10.5), 6.0), line(&[(15.0, 15.0), (19.5, 19.5)])]),
        Icon::Menu => join(vec![line(&[(4.0, 7.0), (20.0, 7.0)]), line(&[(4.0, 12.0), (20.0, 12.0)]), line(&[(4.0, 17.0), (20.0, 17.0)])]),
        Icon::More => vec![circle((5.5, 12.0), 1.75), circle((12.0, 12.0), 1.75), circle((18.5, 12.0), 1.75)],
        Icon::Info => join(vec![ring((12.0, 12.0), 9.0), vec![circle((12.0, 7.75), 1.25)], line(&[(12.0, 11.0), (12.0, 16.5)])]),
        Icon::Warning => join(vec![
            line(&[(12.0, 3.5), (21.0, 19.5), (3.0, 19.5), (12.0, 3.5)]),
            line(&[(12.0, 9.5), (12.0, 13.5)]),
            vec![circle((12.0, 16.75), 1.25)],
        ]),
        Icon::ArrowUp => join(vec![line(&[(12.0, 19.0), (12.0, 5.0)]), line(&[(6.0, 11.0), (12.0, 5.0), (18.0, 11.0)])]),
        Icon::ArrowDown => join(vec![line(&[(12.0, 5.0), (12.0, 19.0)]), line(&[(6.0, 13.0), (12.0, 19.0), (18.0, 13.0)])]),
        Icon::ArrowLeft => join(vec![line(&[(19.0, 12.0), (5.0, 12.0)]), line(&[(11.0, 6.0), (5.0, 12.0), (11.0, 18.0)])]),
        Icon::ArrowRight => join(vec![line(&[(5.0, 12.0), (19.0, 12.0)]), line(&[(13.0, 6.0), (19.0, 12.0), (13.0, 18.0)])]),
        Icon::Calendar => join(vec![
            line(&[(4.0, 6.0), (20.0, 6.0), (20.0, 20.0), (4.0, 20.0), (4.0, 6.0)]),
            line(&[(4.0, 10.0), (20.0, 10.0)]),
            line(&[(8.0, 3.5), (8.0, 7.0)]),
            line(&[(16.0, 3.5), (16.0, 7.0)]),
        ]),
    }
}

/// size × size 画素の濃さの表（0〜255）
pub(crate) fn rasterize(icon: Icon, size: u32) -> Vec<u8> {
    let k = size as f32 / 24.0;
    let mut r = Raster::new(size as usize, size as usize);
    for poly in shapes(icon) {
        let scaled: Vec<Pt> = poly.iter().map(|&(x, y)| (x * k, y * k)).collect();
        r.polygon(&scaled);
    }
    r.coverage()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_draws_something_inside_its_box() {
        for icon in Icon::ALL {
            let c = rasterize(icon, 24);
            let ink: u32 = c.iter().map(|&v| v as u32).sum();
            assert!(ink > 255 * 10, "{icon:?} は塗る所がある: {ink}");
            // 端の 1 画素の枠には、ほとんど塗らない（はみ出していない）
            let edge: u32 = (0..24).map(|i| c[i] as u32 + c[23 * 24 + i] as u32 + c[i * 24] as u32 + c[i * 24 + 23] as u32).sum();
            assert!(edge < 255 * 8, "{icon:?} の端: {edge}");
        }
    }

    #[test]
    fn icons_differ_and_scale() {
        let a = rasterize(Icon::ChevronDown, 16);
        let b = rasterize(Icon::ChevronUp, 16);
        assert_ne!(a, b);
        // 大きくすると塗る量は面積に比べて増える
        let small: u32 = rasterize(Icon::Plus, 16).iter().map(|&v| v as u32).sum();
        let big: u32 = rasterize(Icon::Plus, 32).iter().map(|&v| v as u32).sum();
        assert!(big as f32 > small as f32 * 3.0, "{small} {big}");
        // 検索の輪の真ん中は穴
        let s = rasterize(Icon::Search, 24);
        assert_eq!(s[10 * 24 + 10], 0);
    }
}
