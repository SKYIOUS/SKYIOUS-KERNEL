use alloc::vec::Vec;

#[derive(Clone, Copy, Debug)]
pub struct DirtyRect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

impl DirtyRect {
    pub fn from_xywh(x: usize, y: usize, w: usize, h: usize) -> Self {
        DirtyRect { x, y, w, h }
    }

    pub fn right(&self) -> usize { self.x + self.w }
    pub fn bottom(&self) -> usize { self.y + self.h }

    fn overlaps(&self, other: &DirtyRect) -> bool {
        self.x < other.right() && self.right() > other.x
            && self.y < other.bottom() && self.bottom() > other.y
    }

    fn merged(&self, other: &DirtyRect) -> DirtyRect {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        DirtyRect {
            x,
            y,
            w: self.right().max(other.right()) - x,
            h: self.bottom().max(other.bottom()) - y,
        }
    }
}

pub struct DamageTracker {
    rects: Vec<DirtyRect>,
    screen_w: usize,
    screen_h: usize,
}

impl DamageTracker {
    pub fn new(screen_w: usize, screen_h: usize) -> Self {
        DamageTracker { rects: Vec::new(), screen_w, screen_h }
    }

    pub fn mark(&mut self, x: usize, y: usize, w: usize, h: usize) {
        if w == 0 || h == 0 { return; }
        let x = x.min(self.screen_w.saturating_sub(1));
        let y = y.min(self.screen_h.saturating_sub(1));
        let w = w.min(self.screen_w - x);
        let h = h.min(self.screen_h - y);
        if w == 0 || h == 0 { return; }
        let new = DirtyRect::from_xywh(x, y, w, h);
        for r in &mut self.rects {
            if r.overlaps(&new) {
                *r = r.merged(&new);
                return;
            }
        }
        self.rects.push(new);
    }

    pub fn mark_full(&mut self) {
        self.rects.clear();
        self.rects.push(DirtyRect::from_xywh(0, 0, self.screen_w, self.screen_h));
    }

    pub fn drain(&mut self) -> Vec<DirtyRect> {
        core::mem::take(&mut self.rects)
    }

    pub fn clear(&mut self) {
        self.rects.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }
}
