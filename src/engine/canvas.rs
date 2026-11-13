//! Изображение в памяти (0xAARRGGBB) и примитивы рисования.

pub type Color = u32;

pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    0xFF00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | b as u32
}

pub const fn hex(v: u32) -> Color {
    0xFF00_0000 | v
}

pub fn with_alpha(c: Color, a: u8) -> Color {
    (c & 0x00FF_FFFF) | ((a as u32) << 24)
}

pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    let ch = |s: u32| {
        let x = ((a >> s) & 255) as f32;
        let y = ((b >> s) & 255) as f32;
        (x + (y - x) * t) as u32
    };
    0xFF00_0000 | (ch(16) << 16) | (ch(8) << 8) | ch(0)
}

/// f < 1 затемняет, f > 1 осветляет к белому.
pub fn shade(c: Color, f: f32) -> Color {
    if f <= 1.0 {
        mix(hex(0), c, f)
    } else {
        mix(c, hex(0xFFFFFF), f - 1.0)
    }
}

#[inline]
pub fn blend_px(dst: u32, src: u32) -> u32 {
    let a = src >> 24;
    if a == 255 {
        return src;
    }
    if a == 0 {
        return dst;
    }
    let ia = 255 - a;
    let rb = (((src & 0xFF00FF) * a + (dst & 0xFF00FF) * ia) >> 8) & 0xFF00FF;
    let g = (((src & 0xFF00) * a + (dst & 0xFF00) * ia) >> 8) & 0xFF00;
    0xFF00_0000 | rb | g
}

#[derive(Clone)]
pub struct Image {
    pub w: i32,
    pub h: i32,
    pub px: Vec<u32>,
}

impl Image {
    pub fn new(w: i32, h: i32) -> Self {
        Image { w, h, px: vec![0; (w * h) as usize] }
    }

    pub fn clear(&mut self, c: Color) {
        self.px.fill(c);
    }

    /// Записать пиксель без смешивания (для рисования спрайтов).
    pub fn set(&mut self, x: i32, y: i32, c: Color) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            self.px[(y * self.w + x) as usize] = c;
        }
    }

    /// Пиксель со смешиванием по альфе.
    pub fn put(&mut self, x: i32, y: i32, c: Color) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            let p = &mut self.px[(y * self.w + x) as usize];
            *p = blend_px(*p, c);
        }
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: Color) {
        let x0 = x.max(0);
        let y0 = y.max(0);
        let x1 = (x + w).min(self.w);
        let y1 = (y + h).min(self.h);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let a = c >> 24;
        for yy in y0..y1 {
            let row = (yy * self.w) as usize;
            let s = &mut self.px[row + x0 as usize..row + x1 as usize];
            if a == 255 {
                s.fill(c);
            } else {
                for p in s {
                    *p = blend_px(*p, c);
                }
            }
        }
    }

    pub fn rect_outline(&mut self, x: i32, y: i32, w: i32, h: i32, t: i32, c: Color) {
        self.fill_rect(x, y, w, t, c);
        self.fill_rect(x, y + h - t, w, t, c);
        self.fill_rect(x, y + t, t, h - 2 * t, c);
        self.fill_rect(x + w - t, y + t, t, h - 2 * t, c);
    }

    pub fn fill_circle(&mut self, cx: f32, cy: f32, r: f32, c: Color) {
        let x0 = (cx - r).floor() as i32;
        let x1 = (cx + r).ceil() as i32;
        let y0 = (cy - r).floor() as i32;
        let y1 = (cy + r).ceil() as i32;
        let opaque = c >> 24 == 255;
        for y in y0.max(0)..y1.min(self.h) {
            for x in x0.max(0)..x1.min(self.w) {
                let dx = x as f32 + 0.5 - cx;
                let dy = y as f32 + 0.5 - cy;
                if dx * dx + dy * dy <= r * r {
                    let p = &mut self.px[(y * self.w + x) as usize];
                    *p = if opaque { c } else { blend_px(*p, c) };
                }
            }
        }
    }

    pub fn fill_round_rect(&mut self, x: i32, y: i32, w: i32, h: i32, r: f32, c: Color) {
        let opaque = c >> 24 == 255;
        for yy in y.max(0)..(y + h).min(self.h) {
            for xx in x.max(0)..(x + w).min(self.w) {
                let fx = xx as f32 + 0.5;
                let fy = yy as f32 + 0.5;
                let cx = fx.clamp(x as f32 + r, (x + w) as f32 - r);
                let cy = fy.clamp(y as f32 + r, (y + h) as f32 - r);
                let dx = fx - cx;
                let dy = fy - cy;
                if dx * dx + dy * dy <= r * r {
                    let p = &mut self.px[(yy * self.w + xx) as usize];
                    *p = if opaque { c } else { blend_px(*p, c) };
                }
            }
        }
    }

    /// Вывод спрайта: целочисленный масштаб, поворот на rot*90° по часовой, общая альфа.
    pub fn blit_ex(&mut self, s: &Image, dx: i32, dy: i32, scale: i32, rot: u8, alpha: u32) {
        let (rw, rh) = if rot % 2 == 0 { (s.w, s.h) } else { (s.h, s.w) };
        let x0 = dx.max(0);
        let y0 = dy.max(0);
        let x1 = (dx + rw * scale).min(self.w);
        let y1 = (dy + rh * scale).min(self.h);
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        for y in y0..y1 {
            let ry = (y - dy) / scale;
            let row = (y * self.w) as usize;
            for x in x0..x1 {
                let rx = (x - dx) / scale;
                let (u, v) = match rot & 3 {
                    0 => (rx, ry),
                    1 => (ry, s.h - 1 - rx),
                    2 => (s.w - 1 - rx, s.h - 1 - ry),
                    _ => (s.w - 1 - ry, rx),
                };
                let mut c = s.px[(v * s.w + u) as usize];
                let a = c >> 24;
                if a == 0 {
                    continue;
                }
                if alpha < 255 {
                    c = (c & 0x00FF_FFFF) | ((a * alpha / 255) << 24);
                }
                let d = &mut self.px[row + x as usize];
                *d = blend_px(*d, c);
            }
        }
    }

    pub fn blit(&mut self, s: &Image, dx: i32, dy: i32, scale: i32) {
        self.blit_ex(s, dx, dy, scale, 0, 255);
    }

    pub fn mirrored_h(&self) -> Image {
        let mut o = Image::new(self.w, self.h);
        for y in 0..self.h {
            for x in 0..self.w {
                o.px[(y * self.w + x) as usize] = self.px[(y * self.w + self.w - 1 - x) as usize];
            }
        }
        o
    }
}
