//! Крошечный 2D-движок: программный рендер в кадровый буфер, растровый шрифт,
//! синтезатор звука, запись PNG и генератор случайных чисел.

pub mod audio;
pub mod canvas;
pub mod font;
pub mod png;

pub use canvas::*;
pub use minifb::Key;

/// xorshift64* - маленький детерминированный ГСЧ.
pub struct Rng(pub u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        (x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 32) as u32
    }
    /// Целое из [lo, hi).
    pub fn range(&mut self, lo: i32, hi: i32) -> i32 {
        if hi <= lo {
            return lo;
        }
        lo + (self.next_u32() % (hi - lo) as u32) as i32
    }
    pub fn f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }
}

/// Снимок ввода за кадр.
#[derive(Default)]
pub struct Input {
    pub mx: i32,
    pub my: i32,
    pub dmx: i32,
    pub dmy: i32,
    pub lmb: bool,
    pub rmb: bool,
    pub mmb: bool,
    pub lmb_pressed: bool,
    pub rmb_pressed: bool,
    /// Боковые кнопки мыши ("назад" и "вперёд"), только момент нажатия.
    pub x1_pressed: bool,
    pub x2_pressed: bool,
    pub shift: bool,
    pub ctrl: bool,
    pub wheel: f32,
    pub pressed: Vec<Key>,
    pub pan_x: f32,
    pub pan_y: f32,
}
