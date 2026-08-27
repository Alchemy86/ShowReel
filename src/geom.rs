//! Rectangles, anchors and fit modes.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Rect { x, y, w, h }
    }

    pub fn from_size(w: f64, h: f64) -> Self {
        Rect { x: 0.0, y: 0.0, w, h }
    }

    /// A rect of `w`x`h` centred on (`cx`, `cy`).
    pub fn centred(cx: f64, cy: f64, w: f64, h: f64) -> Self {
        Rect { x: cx - w / 2.0, y: cy - h / 2.0, w, h }
    }

    pub fn centre(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    pub fn aspect(&self) -> f64 {
        if self.h == 0.0 { 1.0 } else { self.w / self.h }
    }

    /// Grow (or, with a negative value, shrink) on every side.
    pub fn inset(&self, d: f64) -> Rect {
        Rect { x: self.x + d, y: self.y + d, w: self.w - 2.0 * d, h: self.h - 2.0 * d }
    }

    /// Widen this rect to `aspect`, keeping its centre. Never shrinks a side,
    /// so the original content always stays inside.
    pub fn to_aspect(&self, aspect: f64) -> Rect {
        let (cx, cy) = self.centre();
        if self.aspect() < aspect {
            let w = self.h * aspect;
            Rect::centred(cx, cy, w, self.h)
        } else {
            let h = self.w / aspect;
            Rect::centred(cx, cy, self.w, h)
        }
    }

    /// The largest rect of `aspect` that fits *inside* this one, centred.
    ///
    /// The counterpart to [`Rect::to_aspect`], which grows. This one crops,
    /// which is what `Fit::Cover` needs: the region of a source that will be
    /// shown when it fills a differently-shaped box.
    pub fn inscribed_aspect(&self, aspect: f64) -> Rect {
        let (cx, cy) = self.centre();
        if self.aspect() > aspect {
            Rect::centred(cx, cy, self.h * aspect, self.h)
        } else {
            Rect::centred(cx, cy, self.w, self.w / aspect)
        }
    }

    /// Linear blend between two rects.
    pub fn lerp(&self, other: &Rect, t: f64) -> Rect {
        Rect {
            x: self.x + (other.x - self.x) * t,
            y: self.y + (other.y - self.y) * t,
            w: self.w + (other.w - self.w) * t,
            h: self.h + (other.h - self.h) * t,
        }
    }

    /// Keep this rect inside `bounds` where it is smaller, and centre it on
    /// the axes where it is larger. Stops a camera drifting off the map.
    pub fn clamp_within(&self, bounds: &Rect) -> Rect {
        let mut r = *self;
        if r.w >= bounds.w {
            r.x = bounds.x + (bounds.w - r.w) / 2.0;
        } else {
            r.x = r.x.clamp(bounds.x, bounds.right() - r.w);
        }
        if r.h >= bounds.h {
            r.y = bounds.y + (bounds.h - r.h) / 2.0;
        } else {
            r.y = r.y.clamp(bounds.y, bounds.bottom() - r.h);
        }
        r
    }
}

/// Where a thing is pinned, relative to its container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    #[default]
    Centre,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Anchor {
    /// Fractions of the container, (0..1, 0..1).
    pub fn fractions(self) -> (f64, f64) {
        use Anchor::*;
        let fx = match self {
            TopLeft | Left | BottomLeft => 0.0,
            Top | Centre | Bottom => 0.5,
            TopRight | Right | BottomRight => 1.0,
        };
        let fy = match self {
            TopLeft | Top | TopRight => 0.0,
            Left | Centre | Right => 0.5,
            BottomLeft | Bottom | BottomRight => 1.0,
        };
        (fx, fy)
    }

    /// Place a `w`x`h` box in `container`, `pad` in from the edges it touches.
    pub fn place(self, container: &Rect, w: f64, h: f64, pad: f64) -> Rect {
        let (fx, fy) = self.fractions();
        let inner = container.inset(pad);
        Rect { x: inner.x + (inner.w - w) * fx, y: inner.y + (inner.h - h) * fy, w, h }
    }
}

/// How a source of one aspect ratio fills a box of another.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Fit {
    /// Fill the box, cropping the overflow.
    #[default]
    Cover,
    /// Fit entirely inside the box, leaving bars.
    Contain,
    /// Ignore the source aspect ratio.
    Stretch,
    /// Draw at native size.
    None,
}

impl Fit {
    /// The destination rect for a `src_w`x`src_h` source drawn into `box_`.
    pub fn apply(self, src_w: f64, src_h: f64, box_: &Rect) -> Rect {
        if src_w <= 0.0 || src_h <= 0.0 {
            return *box_;
        }
        let (cx, cy) = box_.centre();
        match self {
            Fit::Stretch => *box_,
            Fit::None => Rect::centred(cx, cy, src_w, src_h),
            Fit::Cover => {
                let k = (box_.w / src_w).max(box_.h / src_h);
                Rect::centred(cx, cy, src_w * k, src_h * k)
            }
            Fit::Contain => {
                let k = (box_.w / src_w).min(box_.h / src_h);
                Rect::centred(cx, cy, src_w * k, src_h * k)
            }
        }
    }
}

/// A direction, for wipes, slides and pushes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Direction {
    #[default]
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    /// Unit vector in screen space (x right, y down).
    pub fn vector(self) -> (f64, f64) {
        match self {
            Direction::Left => (-1.0, 0.0),
            Direction::Right => (1.0, 0.0),
            Direction::Up => (0.0, -1.0),
            Direction::Down => (0.0, 1.0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_fills_and_contain_fits() {
        let box_ = Rect::from_size(1920.0, 1080.0);
        let c = Fit::Cover.apply(160.0, 144.0, &box_);
        assert!(c.w >= 1920.0 - 1e-9 && c.h >= 1080.0 - 1e-9);
        let k = Fit::Contain.apply(160.0, 144.0, &box_);
        assert!(k.w <= 1920.0 + 1e-9 && k.h <= 1080.0 + 1e-9);
        // Both keep the source aspect ratio.
        assert!((c.aspect() - 160.0 / 144.0).abs() < 1e-9);
        assert!((k.aspect() - 160.0 / 144.0).abs() < 1e-9);
    }

    #[test]
    fn to_aspect_grows_and_inscribed_aspect_crops() {
        let square = Rect::from_size(5000.0, 5000.0);
        let wide = 16.0 / 9.0;
        let grown = square.to_aspect(wide);
        let cropped = square.inscribed_aspect(wide);
        assert!((grown.aspect() - wide).abs() < 1e-9);
        assert!((cropped.aspect() - wide).abs() < 1e-9);
        // Growing leaves the source entirely inside; cropping stays inside the
        // source. That difference is Contain versus Cover.
        assert!(grown.w > square.w && grown.h >= square.h - 1e-9);
        assert!(cropped.w <= square.w + 1e-9 && cropped.h < square.h);
        assert!((cropped.w - 5000.0).abs() < 1e-6, "width is the limit here");
    }

    #[test]
    fn clamp_keeps_viewport_on_the_map() {
        let map = Rect::from_size(1000.0, 1000.0);
        // Off the left edge -> pushed back on.
        assert_eq!(Rect::new(-50.0, 10.0, 100.0, 100.0).clamp_within(&map).x, 0.0);
        // Wider than the map -> centred, showing the whole width.
        let wide = Rect::new(-500.0, 0.0, 2000.0, 100.0).clamp_within(&map);
        assert_eq!(wide.x, -500.0);
    }

    #[test]
    fn anchor_places_with_padding() {
        let c = Rect::from_size(1000.0, 1000.0);
        let r = Anchor::BottomLeft.place(&c, 100.0, 50.0, 40.0);
        assert_eq!((r.x, r.y), (40.0, 910.0));
    }
}
