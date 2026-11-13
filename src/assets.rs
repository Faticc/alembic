//! Все ассеты игры рисуются кодом при запуске: тайлы, конвейеры, машины, предметы.
//! Базовый спрайт смотрит на север (выход вверху), остальные направления - поворотом.

use crate::engine::*;
use crate::world::{Role, KINDS, N_FORMS};

pub const TILE: i32 = 24;
pub const ITEM: i32 = 10;

pub const OUTLINE: Color = hex(0x2B2433);
pub const ELEM_COLORS: [Color; 4] = [hex(0xE8553A), hex(0x3A8FE8), hex(0x5DAA4A), hex(0xB58CF0)];

const GROUND: Color = hex(0xE9DFC6);
const BELT: Color = hex(0x4A4E5A);
const BELT_HI: [Color; 2] = [hex(0x7A8192), hex(0x58B4F0)];
const BELT_RAIL: Color = hex(0x2F323B);
const PORT: Color = hex(0x14121A);
const NOZZLE: Color = hex(0xD8DCE6);
const GOLD: Color = hex(0xF0C24B);

pub struct Assets {
    pub ground: Vec<Image>,
    /// Индекс: элемент * 2 + вариант.
    pub veins: Vec<Image>,
    /// Ленты по стилям (0 обычная, 1 экспресс), по 8 кадров: прямая, поворот слева, поворот справа.
    pub belt: [Vec<Image>; 2],
    pub belt_l: [Vec<Image>; 2],
    pub belt_r: [Vec<Image>; 2],
    /// Стыки: боковое примыкание слева/справа и заглушка торца ленты.
    pub belt_stub_l: Image,
    pub belt_stub_r: Image,
    pub belt_cap: Image,
    /// Кадры спрайта каждой постройки, индекс - Kind.0.
    pub kinds: Vec<Vec<Image>>,
    /// Индекс: Item::id().
    pub items: Vec<Image>,
}

impl Assets {
    pub fn generate() -> Assets {
        let mut rng = Rng::new(20261113);
        let ground: Vec<Image> = (0..4).map(|_| ground(&mut rng)).collect();
        let mut veins = Vec::new();
        for e in 0..4 {
            for _ in 0..2 {
                veins.push(vein(e, &mut rng));
            }
        }
        let belt = [0, 1].map(|s| (0..8).map(|f| belt_straight(f, s)).collect::<Vec<_>>());
        let belt_l = [0, 1].map(|s| (0..8).map(|f| belt_curve(f, s)).collect::<Vec<_>>());
        let belt_r = [0, 1].map(|s| belt_l[s].iter().map(|i| i.mirrored_h()).collect::<Vec<_>>());
        let mut items = Vec::new();
        for form in 0..N_FORMS as u8 {
            for purity in 1..=3u8 {
                for mask in 1..=15u8 {
                    items.push(item_sprite(mask, purity, form));
                }
            }
        }
        let kinds = KINDS
            .iter()
            .map(|d| {
                let body = hex(d.color);
                match d.role {
                    Role::Belt(m) => belt[(m > 1.0) as usize].clone(),
                    Role::Extractor(m) => (0..4).map(|f| extractor(body, m > 1.0, f)).collect(),
                    Role::Mixer => (0..4).map(mixer).collect(),
                    Role::Distiller => (0..4).map(distiller).collect(),
                    Role::Splitter => vec![splitter(body, false)],
                    Role::Overflow => vec![splitter(body, true)],
                    Role::Filter => vec![filter(body)],
                    Role::Bridge => vec![bridge()],
                    Role::Storage => vec![storage(body)],
                    Role::Trash => vec![trash(body)],
                    Role::Trade => vec![trade(body)],
                    Role::Convert(_, to, _) | Role::Assemble(_, _, to, _) => {
                        (0..4).map(|f| production(body, to, f)).collect()
                    }
                    Role::Hub => vec![hub()],
                }
            })
            .collect();
        Assets {
            ground,
            veins,
            belt,
            belt_l,
            belt_r,
            belt_stub_r: belt_stub().mirrored_h(),
            belt_stub_l: belt_stub(),
            belt_cap: belt_cap(),
            kinds,
            items,
        }
    }

    /// Собирает все спрайты в один атлас (для `--export-assets`).
    pub fn atlas(&self) -> Image {
        let mut all: Vec<&Image> = Vec::new();
        all.extend(self.ground.iter());
        all.extend(self.veins.iter());
        for s in 0..2 {
            all.extend(self.belt[s].iter());
            all.extend(self.belt_l[s].iter());
        }
        all.extend([&self.belt_stub_l, &self.belt_stub_r, &self.belt_cap]);
        for k in &self.kinds {
            all.extend(k.iter());
        }
        all.extend(self.items.iter());
        let mut a = Image::new(548, 1100);
        a.clear(hex(0x3A3346));
        let (mut x, mut y, mut row_h) = (2, 2, 0);
        for im in all {
            if x + im.w + 2 > a.w {
                x = 2;
                y += row_h + 2;
                row_h = 0;
            }
            a.blit(im, x, y, 1);
            x += im.w + 2;
            row_h = row_h.max(im.h);
        }
        a
    }
}

fn ground(rng: &mut Rng) -> Image {
    let mut im = Image::new(TILE, TILE);
    im.clear(GROUND);
    for i in 0..TILE {
        im.set(i, 0, hex(0xDDD2B6));
        im.set(0, i, hex(0xDDD2B6));
    }
    for _ in 0..9 {
        let c = if rng.range(0, 2) == 0 { hex(0xDFD4B8) } else { hex(0xEFE7D2) };
        im.set(rng.range(1, TILE), rng.range(1, TILE), c);
    }
    im
}

fn vein(e: usize, rng: &mut Rng) -> Image {
    let mut im = ground(rng);
    let col = ELEM_COLORS[e];
    im.fill_round_rect(1, 1, 22, 22, 6.0, mix(GROUND, col, 0.28));
    for _ in 0..6 {
        let x = rng.range(5, 19) as f32 + 0.5;
        let y = rng.range(5, 19) as f32 + 0.5;
        let r = 2.2 + rng.f32() * 1.4;
        im.fill_circle(x, y, r + 1.0, shade(col, 0.6));
        im.fill_circle(x, y, r, col);
        im.set(x as i32 - 1, y as i32 - 1, shade(col, 1.5));
    }
    im
}

// ---------- конвейеры ----------

fn belt_pixel(across: f32, travel: f32, frame: i32, style: usize) -> Color {
    // across: расстояние от осевой линии; travel: пройденный путь в пикселях (0..24).
    if across >= 8.0 {
        return BELT_RAIL;
    }
    let ph = (24.0 - travel - across * 0.7 + frame as f32).rem_euclid(8.0);
    if ph < 2.0 {
        BELT_HI[style]
    } else {
        BELT
    }
}

fn belt_straight(frame: i32, style: usize) -> Image {
    let mut im = Image::new(TILE, TILE);
    for y in 0..TILE {
        for x in 3..21 {
            let across = (x as f32 - 11.5).abs();
            im.set(x, y, belt_pixel(across, 23.5 - y as f32, frame, style));
        }
    }
    im
}

/// Поворот: вход с запада, выход на север (дуга вокруг левого верхнего угла).
fn belt_curve(frame: i32, style: usize) -> Image {
    let mut im = Image::new(TILE, TILE);
    for y in 0..TILE {
        for x in 0..TILE {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let r = (fx * fx + fy * fy).sqrt();
            if !(3.0..21.0).contains(&r) {
                continue;
            }
            let ang = fy.atan2(fx);
            let travel = (1.0 - ang / std::f32::consts::FRAC_PI_2) * 24.0;
            im.set(x, y, belt_pixel((r - 12.0).abs(), travel, frame, style));
        }
    }
    im
}

/// Примыкание с запада к ленте, идущей на север: закрывает зазор до края тайла.
fn belt_stub() -> Image {
    let mut im = Image::new(TILE, TILE);
    for y in 3..21 {
        for x in 0..4 {
            im.set(x, y, if y == 3 || y == 20 { BELT_RAIL } else { BELT });
        }
    }
    im
}

/// Заглушка заднего торца ленты (у ленты, идущей на север, - снизу).
fn belt_cap() -> Image {
    let mut im = Image::new(TILE, TILE);
    im.fill_rect(3, 22, 18, 2, BELT_RAIL);
    im
}

// ---------- машины ----------

fn machine_base(body: Color, sides: bool, back: bool) -> Image {
    let mut im = Image::new(TILE, TILE);
    if sides {
        im.fill_rect(0, 8, 3, 8, OUTLINE);
        im.fill_rect(0, 9, 3, 6, PORT);
        im.fill_rect(21, 8, 3, 8, OUTLINE);
        im.fill_rect(21, 9, 3, 6, PORT);
    }
    if back {
        im.fill_rect(8, 21, 8, 3, OUTLINE);
        im.fill_rect(9, 21, 6, 3, PORT);
    }
    im.fill_round_rect(2, 2, 20, 20, 4.5, OUTLINE);
    im.fill_round_rect(3, 3, 18, 18, 3.5, body);
    im.fill_rect(6, 3, 12, 1, shade(body, 1.3));
    im.fill_rect(6, 20, 12, 1, shade(body, 0.75));
    // Выходной патрубок сверху.
    im.fill_rect(8, 0, 8, 4, OUTLINE);
    im.fill_rect(9, 0, 6, 3, NOZZLE);
    im.fill_rect(11, 0, 2, 2, hex(0x8A93A6));
    im
}

/// Корпус без выхода с приёмом со всех четырёх сторон (утилизатор, торговый пост).
fn sink_base(body: Color) -> Image {
    let mut im = Image::new(TILE, TILE);
    im.fill_rect(8, 0, 8, 24, OUTLINE);
    im.fill_rect(0, 8, 24, 8, OUTLINE);
    im.fill_rect(9, 0, 6, 24, PORT);
    im.fill_rect(0, 9, 24, 6, PORT);
    im.fill_round_rect(2, 2, 20, 20, 4.5, OUTLINE);
    im.fill_round_rect(3, 3, 18, 18, 3.5, body);
    im.fill_rect(6, 3, 12, 1, shade(body, 1.3));
    im.fill_rect(6, 20, 12, 1, shade(body, 0.75));
    im
}

fn extractor(body: Color, drill: bool, f: i32) -> Image {
    let mut im = machine_base(body, false, false);
    im.fill_circle(12.0, 12.0, 7.6, OUTLINE);
    im.fill_circle(12.0, 12.0, 6.6, hex(0x3A3F4B));
    let teeth = if drill { 6 } else { 4 };
    for k in 0..teeth {
        let a = (f as f32 * 90.0 / teeth as f32 + k as f32 * 360.0 / teeth as f32).to_radians();
        let c = if drill { GOLD } else { hex(0xD8DCE6) };
        im.fill_circle(12.0 + a.cos() * 5.0, 12.0 + a.sin() * 5.0, 1.1, c);
    }
    im.fill_circle(12.0, 12.0, 3.6, PORT);
    im
}

fn mixer(f: i32) -> Image {
    let mut im = machine_base(hex(0x3FA7A0), true, true);
    im.fill_circle(12.0, 12.0, 6.6, OUTLINE);
    im.fill_circle(12.0, 12.0, 5.6, hex(0xEFE6D0));
    let a = (f as f32 * 45.0).to_radians();
    im.fill_circle(12.0 + a.cos() * 3.0, 12.0 + a.sin() * 3.0, 1.6, hex(0x7A4FB5));
    im.fill_circle(12.0 - a.cos() * 3.0, 12.0 - a.sin() * 3.0, 1.6, hex(0xE8553A));
    im.set(11, 11, hex(0xC9BFA6));
    im.set(12, 12, hex(0xC9BFA6));
    im
}

fn distiller(f: i32) -> Image {
    let mut im = machine_base(hex(0xC7773A), true, true);
    im.fill_circle(12.0, 12.5, 6.6, OUTLINE);
    im.fill_circle(12.0, 12.5, 5.6, hex(0xF3E9D2));
    for y in 12..19 {
        for x in 6..18 {
            let dx = x as f32 + 0.5 - 12.0;
            let dy = y as f32 + 0.5 - 12.5;
            if dx * dx + dy * dy <= 5.6 * 5.6 {
                im.set(x, y, hex(0xE8A93A));
            }
        }
    }
    im.fill_rect(8, 12, 8, 1, hex(0xF6CB6B));
    im.set(10, 16 - f, hex(0xFFF6DC));
    im.set(14, 15 - (f + 2) % 4, hex(0xFFF6DC));
    im.fill_rect(11, 5, 2, 2, OUTLINE);
    im
}

/// Обрабатывающая машина: на тёмной табличке нарисован предмет, который она выпускает.
fn production(body: Color, to: u8, f: i32) -> Image {
    let mut im = machine_base(body, true, true);
    im.fill_round_rect(5, 5, 14, 14, 3.0, OUTLINE);
    im.fill_round_rect(6, 6, 12, 12, 2.0, hex(0x3A3346));
    im.blit(&item_sprite(15, 2, to), 7, 7, 1);
    let lamp = [(4, 4), (19, 4), (19, 19), (4, 19)][f as usize % 4];
    im.set(lamp.0, lamp.1, hex(0xFFF6DC));
    im
}

fn splitter(body: Color, overflow: bool) -> Image {
    let mut im = machine_base(body, false, true);
    // Боковые выходы.
    im.fill_rect(0, 8, 4, 8, OUTLINE);
    im.fill_rect(0, 9, 3, 6, NOZZLE);
    im.fill_rect(20, 8, 4, 8, OUTLINE);
    im.fill_rect(21, 9, 3, 6, NOZZLE);
    // Стрелки: у перелива главная - вперёд, боковые тонкие.
    im.fill_rect(11, 7, 2, 9, OUTLINE);
    im.fill_rect(10, 8, 4, 1, OUTLINE);
    if overflow {
        im.fill_rect(9, 9, 6, 1, OUTLINE);
        im.fill_rect(7, 14, 10, 1, OUTLINE);
        im.fill_rect(7, 13, 1, 3, OUTLINE);
        im.fill_rect(16, 13, 1, 3, OUTLINE);
    } else {
        im.fill_rect(7, 11, 10, 2, OUTLINE);
        im.fill_rect(8, 10, 1, 4, OUTLINE);
        im.fill_rect(15, 10, 1, 4, OUTLINE);
    }
    im
}

fn filter(body: Color) -> Image {
    let mut im = splitter(body, false);
    // Вместо стрелок - табличка, на которой игра рисует образец.
    im.fill_round_rect(5, 5, 14, 14, 3.0, OUTLINE);
    im.fill_round_rect(6, 6, 12, 12, 2.0, hex(0xEFE6D0));
    im.fill_rect(8, 8, 8, 1, hex(0xC9BFA6));
    im.fill_rect(9, 10, 6, 1, hex(0xC9BFA6));
    im.fill_rect(10, 12, 4, 1, hex(0xC9BFA6));
    im.fill_rect(11, 14, 2, 2, hex(0xC9BFA6));
    im
}

fn storage(body: Color) -> Image {
    let mut im = machine_base(body, true, true);
    im.fill_round_rect(4, 5, 16, 15, 2.0, OUTLINE);
    im.fill_rect(5, 6, 14, 13, shade(body, 1.25));
    for y in [9, 13, 16] {
        im.fill_rect(5, y, 14, 1, shade(body, 0.8));
    }
    im.fill_rect(5, 6, 1, 13, shade(body, 0.8));
    im.fill_rect(18, 6, 1, 13, shade(body, 0.8));
    im
}

fn trash(body: Color) -> Image {
    let mut im = sink_base(body);
    im.fill_circle(12.0, 12.0, 6.6, hex(0xE8553A));
    im.fill_circle(12.0, 12.0, 5.2, PORT);
    for i in 0..6 {
        im.set(9 + i, 9 + i, hex(0xE8553A));
        im.set(10 + i, 9 + i, hex(0xE8553A));
    }
    im
}

fn trade(body: Color) -> Image {
    let mut im = sink_base(body);
    im.fill_circle(12.0, 12.0, 6.8, OUTLINE);
    im.fill_circle(12.0, 12.0, 5.8, GOLD);
    im.fill_circle(11.5, 11.5, 4.2, shade(GOLD, 1.3));
    im.fill_rect(11, 8, 2, 8, hex(0x8A6A1A));
    im.fill_rect(10, 9, 4, 1, hex(0x8A6A1A));
    im.fill_rect(10, 12, 4, 1, hex(0x8A6A1A));
    im.fill_rect(10, 14, 4, 1, hex(0x8A6A1A));
    im
}

fn bridge() -> Image {
    let wood = hex(0x9B6B43);
    let mut im = Image::new(TILE, TILE);
    im.fill_rect(0, 3, 24, 18, BELT_RAIL);
    im.fill_rect(0, 4, 24, 16, BELT);
    im.fill_rect(3, 0, 18, 24, OUTLINE);
    im.fill_rect(4, 0, 16, 24, wood);
    for y in [3, 9, 15, 21] {
        im.fill_rect(4, y, 16, 1, shade(wood, 0.72));
    }
    im.fill_rect(4, 0, 1, 24, shade(wood, 1.3));
    im.fill_rect(19, 0, 1, 24, shade(wood, 0.6));
    im
}

fn hub() -> Image {
    let s = TILE * 3;
    let stone = hex(0x5A5266);
    let mut im = Image::new(s, s);
    im.fill_round_rect(1, 1, 70, 70, 11.0, OUTLINE);
    im.fill_round_rect(3, 3, 66, 66, 9.0, stone);
    im.fill_round_rect(5, 5, 62, 62, 8.0, shade(stone, 1.12));
    im.fill_round_rect(6, 7, 60, 60, 8.0, stone);
    // Широкие приёмные проёмы - ровно под ширину ленты.
    for c in [12, 36, 60] {
        im.fill_rect(c - 9, 0, 18, 5, OUTLINE);
        im.fill_rect(c - 9, 67, 18, 5, OUTLINE);
        im.fill_rect(0, c - 9, 5, 18, OUTLINE);
        im.fill_rect(67, c - 9, 5, 18, OUTLINE);
        im.fill_rect(c - 8, 0, 16, 4, PORT);
        im.fill_rect(c - 8, 68, 16, 4, PORT);
        im.fill_rect(0, c - 8, 4, 16, PORT);
        im.fill_rect(68, c - 8, 4, 16, PORT);
    }
    im.fill_circle(36.0, 36.0, 27.0, OUTLINE);
    im.fill_circle(36.0, 36.0, 26.0, GOLD);
    im.fill_circle(36.0, 35.0, 25.0, shade(GOLD, 1.25));
    im.fill_circle(36.0, 36.5, 25.0, GOLD);
    im.fill_circle(36.0, 36.0, 21.5, OUTLINE);
    im.fill_circle(36.0, 36.0, 20.5, hex(0x3A3346));
    // Четыре стихии по кругу и сияющее ядро.
    for (i, col) in ELEM_COLORS.iter().enumerate() {
        let a = (i as f32 * 90.0 + 45.0).to_radians();
        im.fill_circle(36.0 + a.cos() * 14.0, 36.0 + a.sin() * 14.0, 3.2, *col);
    }
    im.fill_circle(36.0, 36.0, 8.5, hex(0x7A4FB5));
    im.fill_circle(36.0, 36.0, 6.0, hex(0xB58CF0));
    im.fill_circle(36.0, 36.0, 3.0, hex(0xFFF6DC));
    im
}

// ---------- предметы ----------

/// Силуэты форм 2..25: o - контур (цвет чистоты), # - цвет смеси, + - блик,
/// g - золото, w - белый, k - дерево.
const SHAPES: [[&str; 10]; 24] = [
    // 2 зелье
    [
        "...oooo...", "...okko...", "...o##o...", "..o####o..", ".o######o.", ".o+#####o.", ".o######o.",
        ".o######o.", "..o####o..", "...oooo...",
    ],
    // 3 порошок
    [
        "..........", "..........", "....oo....", "...o##o...", "..o#+##o..", "..o####o..", ".o##+###o.",
        ".o######o.", "o########o", "oooooooooo",
    ],
    // 4 слиток
    [
        "..........", "..........", "..oooooo..", ".o++++++o.", ".o######o.", "o########o", "o########o",
        "o########o", "oooooooooo", "..........",
    ],
    // 5 самоцвет
    [
        "..........", "..oooooo..", ".o+w++##o.", "o++++####o", "o########o", ".o######o.", "..o####o..",
        "...o##o...", "....oo....", "..........",
    ],
    // 6 эликсир
    [
        "...gggg...", "...o##o...", "..oo##oo..", "..o####o..", "..o#ww#o..", "..o#ww#o..", "..o####o..",
        "..o####o..", "..o####o..", "..oooooo..",
    ],
    // 7 амулет
    [
        "....oo....", "...o..o...", "..oooooo..", ".oggggggo.", "og##++##go", "og#+####go", "og######go",
        ".oggggggo.", "..oooooo..", "..........",
    ],
    // 8 реликвия
    [
        "....oo....", "...o##o...", "...o##o...", ".ooo##ooo.", "o###ww###o", "o###ww###o", ".ooo##ooo.",
        "...o##o...", "...o##o...", "....oo....",
    ],
    // 9 дымка
    [
        "..........", "...ooo....", "..o###oo..", ".o#+####o.", "o#+######o", "o########o", "o########o",
        ".oo####oo.", "...oooo...", "..........",
    ],
    // 10 лёд
    [
        "..........", ".oooooooo.", ".o+++++#o.", ".o+w####o.", ".o+#####o.", ".o+#####o.", ".o######o.",
        ".o######o.", ".oooooooo.", "..........",
    ],
    // 11 пилюля
    [
        "..........", "..........", "..oooooo..", ".o+++wwwo.", "o###wwwwwo", "o###wwwwwo", ".o###wwwo.",
        "..oooooo..", "..........", "..........",
    ],
    // 12 нить
    [
        "..kkkkkk..", "..okkkko..", "..o####o..", "..o+###o..", "..o####o..", "..o+###o..", "..o####o..",
        "..o+###o..", "..okkkko..", "..kkkkkk..",
    ],
    // 13 монета
    [
        "..........", "...oooo...", "..og++go..", ".og####go.", ".og#ww#go.", ".og#ww#go.", ".og####go.",
        "..og##go..", "...oooo...", "..........",
    ],
    // 14 руна
    [
        "..oooooo..", ".o######o.", ".o#w##w#o.", ".o#w#w##o.", ".o#ww###o.", ".o#w#w##o.", ".o#w##w#o.",
        ".o######o.", ".o######o.", "..oooooo..",
    ],
    // 15 линза
    [
        "..oooo....", ".o+###o...", "o+w####o..", "o+#####o..", "o######o..", ".o####o...", "..ooookk..",
        "......kkk.", ".......kkk", "........kk",
    ],
    // 16 мантия
    [
        "...oooo...", "..o#++#o..", ".o######o.", "o###gg###o", "o###gg###o", "o########o", ".o######o.",
        ".o######o.", ".o######o.", ".oooooooo.",
    ],
    // 17 клинок
    [
        "....oo....", "...o+#o...", "...o+#o...", "...o+#o...", "...o+#o...", "...o+#o...", ".gggggggg.",
        "....kk....", "....kk....", "....gg....",
    ],
    // 18 шестерня
    [
        "....oo....", ".oo.##.oo.", ".o######o.", "..##oo##..", "o##o..o##o", "o##o..o##o", "..##oo##..",
        ".o######o.", ".oo.##.oo.", "....oo....",
    ],
    // 19 призма
    [
        "....oo....", "....oo....", "...o+#o...", "...o+#o...", "..o+###o..", "..o+###o..", ".o+#####o.",
        ".o+#####o.", "o########o", "oooooooooo",
    ],
    // 20 снадобье
    [
        "..kkkkkk..", "..oooooo..", ".o######o.", ".o##ww##o.", ".o#wwww#o.", ".o#wwww#o.", ".o##ww##o.",
        ".o######o.", ".o######o.", "..oooooo..",
    ],
    // 21 гримуар
    [
        ".oooooooo.", ".o######wo", ".o#gggg#wo", ".o#g##g#wo", ".o#g##g#wo", ".o#gggg#wo", ".o######wo",
        ".o######wo", ".oooooooo.", "..........",
    ],
    // 22 голем
    [
        "...oooo...", "...o##o...", ".oooooooo.", "o########o", "o#o####o#o", "o#o#gg#o#o", "ooo####ooo",
        "..o#oo#o..", "..o#oo#o..", "..oooooo..",
    ],
    // 23 астролябия
    [
        "...gggg...", "..g....g..", ".g..##..g.", "g..#oo#..g", "g.#o++o#.g", "g.#o++o#.g", "g..#oo#..g",
        ".g..##..g.", "..g....g..", "...gggg...",
    ],
    // 24 корона
    [
        "..........", "o...oo...o", "oo.o##o.oo", "o#oo##oo#o", "o########o", "o#w####w#o", "o########o",
        "oggggggggo", "oooooooooo", "..........",
    ],
    // 25 магистерий
    [
        "....gg....", ".g..gg..g.", "..gg##gg..", "..g#++#g..", "gg#+ww+#gg", "gg#+ww+#gg", "..g#++#g..",
        "..gg##gg..", ".g..gg..g.", "....gg....",
    ],
];

pub fn item_sprite(mask: u8, purity: u8, form: u8) -> Image {
    let crystal = form == 1;
    let cols: Vec<Color> = (0..4).filter(|e| mask >> e & 1 == 1).map(|e| ELEM_COLORS[e]).collect();
    let n = cols.len().max(1) as i32;
    let oc = [OUTLINE, hex(0xF4F4F8), hex(0xFFC83A)][(purity.clamp(1, 3) - 1) as usize];
    let mut im = Image::new(ITEM, ITEM);
    if form >= 2 {
        for (y, row) in SHAPES[(form as usize - 2).min(SHAPES.len() - 1)].iter().enumerate() {
            let band = cols[((y as i32 - 1).clamp(0, 7) * n / 8) as usize % cols.len().max(1)];
            for (x, ch) in row.bytes().enumerate() {
                let c = match ch {
                    b'o' => oc,
                    b'#' => band,
                    b'+' => shade(band, 1.4),
                    b'g' => GOLD,
                    b'w' => hex(0xFFFFFF),
                    b'k' => hex(0x9B6B43),
                    _ => continue,
                };
                im.set(x as i32, y as i32, c);
            }
        }
        return im;
    }
    for y in 0..ITEM {
        for x in 0..ITEM {
            let fx = x as f32 - 4.5;
            let fy = y as f32 - 4.5;
            let (d, inner, outer) = if crystal {
                (fx.abs() + fy.abs(), 4.2, 5.2)
            } else {
                ((fx * fx + fy * fy).sqrt(), 3.7, 4.8)
            };
            if d <= inner {
                let t = (if crystal { x - 1 } else { y - 1 }).clamp(0, 7);
                let mut c = cols[(t * n / 8) as usize % cols.len().max(1)];
                if crystal && fx + fy < 0.0 {
                    c = shade(c, 1.25);
                } else if !crystal && fy > 1.6 {
                    c = shade(c, 0.85);
                }
                im.set(x, y, c);
            } else if d <= outer {
                im.set(x, y, oc);
            }
        }
    }
    if !crystal {
        let c = im.px[(2 * ITEM + 3) as usize];
        im.set(3, 2, shade(c, 1.6));
    }
    im
}
