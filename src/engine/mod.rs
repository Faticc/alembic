//! Крошечный 2D-движок: программный рендер в кадровый буфер, растровый шрифт,
//! синтезатор звука, запись PNG и генератор случайных чисел.

pub mod audio;
pub mod canvas;
pub mod font;
pub mod png;

pub use canvas::*;
/// Клавиши, которые понимает игра. Платформенный слой (minifb на ПК, сенсорные кнопки на
/// телефоне) переводит свой ввод в них.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Key0,
    Key1,
    Key2,
    Key3,
    Key4,
    Key5,
    Key6,
    Key7,
    Key8,
    Key9,
    B,
    C,
    E,
    F,
    G,
    H,
    L,
    M,
    P,
    Q,
    R,
    T,
    U,
    V,
    X,
    Z,
    F1,
    F2,
    F3,
    F5,
    F11,
    Escape,
    Enter,
    NumPadEnter,
    Backspace,
    Delete,
    Tab,
}

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
#[derive(Default, Clone)]
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
    pub alt: bool,
    pub wheel: f32,
    pub pressed: Vec<Key>,
    /// Набранные за кадр знаки (с учётом раскладки) - для полей с текстом.
    pub typed: Vec<char>,
    pub pan_x: f32,
    pub pan_y: f32,
    /// Геймпад: что зажато сейчас. Нажатия и курсор из него делает сама игра.
    pub pad: Pad,
}

/// Кнопки геймпада, биты `Pad::held`. Раскладка как у пульта Xbox.
pub mod btn {
    pub const A: u32 = 1;
    pub const B: u32 = 1 << 1;
    pub const X: u32 = 1 << 2;
    pub const Y: u32 = 1 << 3;
    pub const LB: u32 = 1 << 4;
    pub const RB: u32 = 1 << 5;
    pub const LT: u32 = 1 << 6;
    pub const RT: u32 = 1 << 7;
    pub const BACK: u32 = 1 << 8;
    pub const START: u32 = 1 << 9;
    pub const L3: u32 = 1 << 10;
    pub const R3: u32 = 1 << 11;
    pub const UP: u32 = 1 << 12;
    pub const DOWN: u32 = 1 << 13;
    pub const LEFT: u32 = 1 << 14;
    pub const RIGHT: u32 = 1 << 15;
}

/// Состояние геймпада за кадр. Оси стиков -1..1 (y вниз), курки 0..1.
#[derive(Default, Clone, Copy, Debug)]
pub struct Pad {
    pub connected: bool,
    pub held: u32,
    pub lx: f32,
    pub ly: f32,
    pub rx: f32,
    pub ry: f32,
    pub lt: f32,
    pub rt: f32,
}
