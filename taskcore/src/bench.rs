//! 一覧の自動スクロール（測定用）。表示からの経過秒を渡すと、その時点の位置を返す。

/// スクロールの速さ（論理ピクセル毎秒）
pub const SPEED: f32 = 3000.0;
/// 表示からスクロールを始めるまでの秒数
pub const DELAY: f32 = 1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step {
    /// まだ始めない
    Wait,
    /// この位置（論理ピクセル）までスクロールする。first は最初の 1 回だけ true
    Scroll { offset: f32, first: bool },
    /// 今終わった。描いた枚数、最初のスクロールから今までの秒数、
    /// 枚と枚のあいだで一番長かった間隔（ミリ秒）
    Finished { frames: u32, secs: f32, max_gap_ms: f32 },
    /// 終わったあと
    Idle,
}

#[derive(Debug, Clone)]
pub struct ScrollBench {
    secs: f32,
    frames: u32,
    first: Option<f32>,
    last: Option<f32>,
    max_gap: f32,
    finished: bool,
}

impl ScrollBench {
    pub fn new(secs: f32) -> Self {
        Self { secs, frames: 0, first: None, last: None, max_gap: 0.0, finished: false }
    }

    /// 測り終えたか（kargui の試作で、描き続けるかどうかを決めるのに使う）
    pub fn is_done(&self) -> bool {
        self.finished
    }

    /// t は表示からの経過秒。1 枚描くごとに 1 回呼ぶ。
    pub fn tick(&mut self, t: f32) -> Step {
        if self.finished {
            return Step::Idle;
        }
        if t < DELAY {
            return Step::Wait;
        }
        let first = *self.first.get_or_insert(t);
        if let Some(last) = self.last {
            self.max_gap = self.max_gap.max(t - last);
        }
        self.last = Some(t);
        if t - DELAY >= self.secs {
            self.finished = true;
            return Step::Finished {
                frames: self.frames,
                secs: t - first,
                max_gap_ms: self.max_gap * 1000.0,
            };
        }
        self.frames += 1;
        Step::Scroll { offset: (t - DELAY) * SPEED, first: self.frames == 1 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_scrolls_then_finishes_once() {
        let mut b = ScrollBench::new(5.0);
        assert_eq!(b.tick(0.5), Step::Wait);
        // 最初の 1 回は first が true。経過がちょうど 1 秒でなくても印が付く
        match b.tick(1.004) {
            Step::Scroll { offset, first } => {
                assert!(first);
                assert!((offset - 12.0).abs() < 1e-3);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(b.tick(1.5), Step::Scroll { offset: 1500.0, first: false });
        match b.tick(6.1) {
            Step::Finished { frames, secs, max_gap_ms } => {
                assert_eq!(frames, 2);
                assert!((secs - 5.096).abs() < 1e-4);
                // 1.5 秒から 6.1 秒のあいだが一番長い
                assert!((max_gap_ms - 4600.0).abs() < 0.5);
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(b.tick(7.0), Step::Idle);
    }
}
