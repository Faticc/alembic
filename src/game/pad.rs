//! Геймпад: тот же перевод в привычный игре ввод мыши и клавиш, что и у сенсора.
//!
//! - левый стик ведёт курсор, крестовина шагает им по тайлам (в окнах - листает);
//! - правый стик двигает камеру, курки - масштаб;
//! - A - левая кнопка мыши, B - правая (в окнах - закрыть), X - поворот, Y - окно настроек;
//! - X + крестовина - переключить сторону машины (вход, выход, закрыто, отказ);
//! - LB / RB - предыдущая и следующая постройка, BACK - слой, L3 - рамка, R3 - провода;
//! - START - меню со всеми остальными действиями, а в окнах с вводом - экранная клавиатура.

use super::*;
use crate::engine::btn;

/// Мёртвая зона стиков.
const DEAD: f32 = 0.22;
/// Задержка перед повтором крестовины и шаг повтора.
const REP_FIRST: f32 = 0.35;
const REP_NEXT: f32 = 0.09;

#[derive(Clone, Copy, PartialEq)]
enum PAct {
    Key(Key),
    Ctrl(Key),
    Frame,
    Char(char),
    Del,
    Enter,
}

#[derive(Default)]
pub struct PadState {
    /// Последним трогали геймпад: курсор и подсказки - его.
    pub active: bool,
    pub(super) cur: (f32, f32),
    prev: u32,
    /// Открыто меню (или клавиатура) и выбранный в нём пункт.
    menu: bool,
    sel: usize,
    /// Режим рамки: курсор работает как SHIFT + ЛКМ.
    frame: bool,
    /// X зажат как модификатор: если за это время меняли сторону, поворота при отпускании нет.
    x_used: bool,
    rep: f32,
    rep_dir: u32,
    zoom_t: f32,
    scroll_t: f32,
}

/// Отклик стика: мёртвая зона и мягкое начало.
fn curve(v: f32) -> f32 {
    let a = v.abs();
    if a < DEAD {
        0.0
    } else {
        v.signum() * ((a - DEAD) / (1.0 - DEAD)).powf(1.6)
    }
}

const DIRS: [(u32, Dir); 4] = [(btn::UP, Dir::N), (btn::RIGHT, Dir::E), (btn::DOWN, Dir::S), (btn::LEFT, Dir::W)];

impl Game {
    /// В открытом окне идёт ввод текста или кода: START показывает клавиатуру.
    fn pad_typing(&self) -> bool {
        (self.show_net && self.net.mode == Mode::Offline) || self.show_bp || self.show_cfg.is_some()
    }

    fn pad_items(&self) -> Vec<(String, PAct)> {
        let s = |t: &str| t.to_string();
        if self.pad_typing() {
            let chars: Vec<char> = if self.show_cfg.is_some() {
                "1234567890".chars().collect()
            } else if self.show_net {
                "1234567890.:".chars().collect()
            } else {
                "АБВГДЕЁЖЗИЙКЛМНОПРСТУФХЦЧШЩЪЫЬЭЮЯ1234567890 -".chars().filter(|c| has_glyph(*c)).collect()
            };
            let mut v: Vec<(String, PAct)> = chars.into_iter().map(|c| (if c == ' ' { s("_") } else { c.to_string() }, PAct::Char(c))).collect();
            if self.show_cfg.is_none() {
                v.push((s("<"), PAct::Del));
                v.push((s("ОК"), PAct::Enter));
            }
            return v;
        }
        let mut v = vec![
            (s("КАТАЛОГ"), PAct::Key(Key::C)),
            (s("СТАТИСТИКА"), PAct::Key(Key::T)),
            (s("ГИЛЬДИИ"), PAct::Key(Key::G)),
            (s("ВИТРАЖ И ВИТОК"), PAct::Key(Key::V)),
            (s("ЧЕРТЕЖИ"), PAct::Key(Key::B)),
            (s("СЕТЕВАЯ ИГРА"), PAct::Key(Key::F2)),
            (s("ПУТЬ КОНТРАКТА"), PAct::Key(Key::P)),
            (s("СПРАВКА"), PAct::Key(Key::F1)),
            (s("ВКЛАДКА ПОСТРОЕК"), PAct::Key(Key::Tab)),
            (s("УЛУЧШИТЬ"), PAct::Key(Key::U)),
            (s("ВЗЯТЬ ТАКУЮ ЖЕ"), PAct::Key(Key::Q)),
            (s("СБРОС ОБРАЗЦА"), PAct::Key(Key::F)),
            (s(if self.pad.frame { "РАМКА: ВКЛ" } else { "РАМКА" }), PAct::Frame),
            (s("КОПИЯ"), PAct::Ctrl(Key::C)),
            (s("ПЕРЕНОС"), PAct::Ctrl(Key::X)),
            (s("ВСТАВКА"), PAct::Ctrl(Key::V)),
            (s("УДАЛИТЬ ВЫДЕЛЕННОЕ"), PAct::Key(Key::Delete)),
            (s(if self.layer == 1 { "НАВЕРХ" } else { "ПОДЗЕМЕЛЬЕ" }), PAct::Key(Key::Z)),
            (s("ПРОВОДА"), PAct::Key(Key::L)),
            (s(if self.audio.muted { "ВКЛЮЧИТЬ ЗВУК" } else { "ВЫКЛЮЧИТЬ ЗВУК" }), PAct::Key(Key::M)),
            (s("СЧЁТЧИК FPS"), PAct::Key(Key::F3)),
        ];
        if !cfg!(target_os = "android") {
            v.push((s("ВО ВЕСЬ ЭКРАН"), PAct::Key(Key::F11)));
        }
        if self.update_ready.is_some() {
            v.push((s("ОБНОВИТЬСЯ"), PAct::Key(Key::F5)));
        }
        v
    }

    fn pad_cols(&self) -> usize {
        if self.pad_typing() {
            if self.show_bp {
                12
            } else {
                6
            }
        } else {
            3
        }
    }

    /// Клетка меню: прямоугольник i-го пункта и рамка всего меню.
    fn pad_cell(&self, i: usize, n: usize) -> ((i32, i32, i32, i32), (i32, i32, i32, i32)) {
        let cols = self.pad_cols();
        let rows = ((n + cols - 1) / cols) as i32;
        let (cw, ch) = if self.pad_typing() { (60, 50) } else { (240, 44) };
        let (gap, pad) = (6, 12);
        let w = cols as i32 * (cw + gap) - gap + 2 * pad;
        let h = rows * (ch + gap) - gap + 2 * pad + 26;
        let x0 = (self.w - w) / 2;
        // Клавиатура - внизу, чтобы было видно поле, куда идёт набор.
        let y0 = if self.pad_typing() { self.h - h - 12 } else { (self.h - h) / 2 };
        let (c, r) = ((i % cols) as i32, (i / cols) as i32);
        ((x0 + pad + c * (cw + gap), y0 + pad + r * (ch + gap), cw, ch), (x0, y0, w, h))
    }

    /// Переводит кадр геймпада во ввод мыши и клавиш. None - геймпад сейчас не главный.
    pub(super) fn pad_adapt(&mut self, inp: &Input, dt: f32) -> Option<Input> {
        let p = inp.pad;
        let held = if p.connected { p.held } else { 0 };
        let pressed = held & !self.pad.prev;
        let released = !held & self.pad.prev;
        self.pad.prev = held;
        let (lx, ly, rx, ry) = (curve(p.lx), curve(p.ly), curve(p.rx), curve(p.ry));
        let touched = pressed != 0 || lx != 0.0 || ly != 0.0 || rx != 0.0 || ry != 0.0 || p.lt > 0.3 || p.rt > 0.3;
        if touched && !self.pad.active {
            self.pad.active = true;
            let on_screen = inp.mx >= 0 && inp.my >= 0 && inp.mx < self.w && inp.my < self.h;
            self.pad.cur = if on_screen { (inp.mx as f32, inp.my as f32) } else { (self.w as f32 / 2.0, self.h as f32 / 2.0) };
        }
        // Мышь или палец снова в деле - курсор их.
        if !touched && (inp.lmb_pressed || inp.rmb_pressed || inp.dmx != 0 || inp.dmy != 0 || inp.wheel != 0.0) {
            self.pad.active = false;
            self.pad.menu = false;
        }
        if !self.pad.active {
            return None;
        }

        let mut o = inp.clone();
        (o.lmb, o.rmb, o.mmb, o.lmb_pressed, o.rmb_pressed) = (false, false, false, false, false);
        o.wheel = 0.0;
        let old = self.pad.cur;

        // Крестовина с повтором при удержании; в меню её заменяет и левый стик.
        let mut dpad = held & (btn::UP | btn::DOWN | btn::LEFT | btn::RIGHT);
        if self.pad.menu {
            let stick = |v: f32, neg: u32, pos: u32| if v < -0.5 { neg } else if v > 0.5 { pos } else { 0 };
            dpad |= stick(p.lx, btn::LEFT, btn::RIGHT) | stick(p.ly, btn::UP, btn::DOWN);
        }
        let step = if dpad == 0 {
            0
        } else if dpad != self.pad.rep_dir {
            self.pad.rep = REP_FIRST;
            dpad
        } else {
            self.pad.rep -= dt;
            if self.pad.rep <= 0.0 {
                self.pad.rep = REP_NEXT;
                dpad
            } else {
                0
            }
        };
        self.pad.rep_dir = dpad;

        if self.pad.menu {
            let items = self.pad_items();
            let n = items.len();
            let cols = self.pad_cols();
            let sel = &mut self.pad.sel;
            *sel = (*sel).min(n.saturating_sub(1));
            if step & btn::LEFT != 0 && *sel > 0 {
                *sel -= 1;
            }
            if step & btn::RIGHT != 0 && *sel + 1 < n {
                *sel += 1;
            }
            if step & btn::UP != 0 && *sel >= cols {
                *sel -= cols;
            }
            if step & btn::DOWN != 0 && *sel + cols < n {
                *sel += cols;
            }
            if pressed & (btn::B | btn::START) != 0 {
                self.pad.menu = false;
            } else if pressed & btn::Y != 0 && self.pad_typing() {
                o.pressed.push(Key::Backspace);
            } else if pressed & btn::A != 0 {
                let act = items[self.pad.sel].1;
                self.pad_act(act, &mut o);
            }
            (o.mx, o.my) = (self.pad.cur.0 as i32, self.pad.cur.1 as i32);
            (o.dmx, o.dmy) = (0, 0);
            return Some(o);
        }

        // Курсор: левый стик, скорость растёт с наклоном.
        let speed = 260.0 + 1300.0 * lx.abs().max(ly.abs());
        self.pad.cur.0 += lx * speed * dt;
        self.pad.cur.1 += ly * speed * dt;
        let x_mod = held & btn::X != 0;
        let modal = self.modal();
        if step != 0 && x_mod && !modal {
            // X + крестовина: сторона машины под курсором.
            let (tx, ty) = self.tile_at(self.pad.cur.0 as i32, self.pad.cur.1 as i32);
            if let Some((_, d)) = DIRS.iter().find(|(b, _)| step & b != 0) {
                if self.world.get(tx, ty).is_some() && self.world.next_side(tx, ty, 0).is_some() {
                    self.cycle_side(tx, ty, *d);
                } else {
                    self.error("ТУТ НЕТ МАШИНЫ СО СТОРОНАМИ");
                }
            }
            self.pad.x_used = true;
        } else if step != 0 && modal {
            if step & btn::UP != 0 {
                o.wheel = 1.0;
            }
            if step & btn::DOWN != 0 {
                o.wheel = -1.0;
            }
            let ts = 40.0;
            if step & btn::LEFT != 0 {
                self.pad.cur.0 -= ts;
            }
            if step & btn::RIGHT != 0 {
                self.pad.cur.0 += ts;
            }
        } else if step != 0 {
            // Шаг на соседний тайл, курсор встаёт в его середину.
            let (mut tx, mut ty) = self.tile_at(self.pad.cur.0 as i32, self.pad.cur.1 as i32);
            for (b, d) in DIRS {
                if step & b != 0 {
                    tx += d.dx();
                    ty += d.dy();
                }
            }
            let (ox, oy) = self.origin();
            let ts = self.ts();
            self.pad.cur = ((ox + tx * ts + ts / 2) as f32, (oy + ty * ts + ts / 2) as f32);
        }
        // Курсор у края экрана тянет за собой камеру.
        let (w, h) = (self.w as f32, self.h as f32);
        if !modal {
            if (self.pad.cur.0 <= 1.0 && lx < 0.0) || (self.pad.cur.0 >= w - 2.0 && lx > 0.0) {
                o.pan_x += lx;
            }
            if (self.pad.cur.1 <= 1.0 && ly < 0.0) || (self.pad.cur.1 >= h - 2.0 && ly > 0.0) {
                o.pan_y += ly;
            }
            o.pan_x += rx;
            o.pan_y += ry;
            // Камера сдвинулась сама: курсор, вставший по тайлу, пусть едет вместе с миром.
        } else if ry != 0.0 {
            self.pad.scroll_t -= dt * ry.abs() * 12.0;
            if self.pad.scroll_t <= 0.0 {
                self.pad.scroll_t = 1.0;
                o.wheel = -ry.signum();
            }
        }
        self.pad.cur = (self.pad.cur.0.clamp(0.0, w - 1.0), self.pad.cur.1.clamp(0.0, h - 1.0));

        // Масштаб курками с повтором.
        let z = (p.rt > 0.5 || held & btn::RT != 0) as i32 - (p.lt > 0.5 || held & btn::LT != 0) as i32;
        if z == 0 {
            self.pad.zoom_t = 0.0;
        } else {
            self.pad.zoom_t -= dt;
            if self.pad.zoom_t <= 0.0 {
                self.pad.zoom_t = 0.3;
                o.wheel = z as f32;
            }
        }

        // Кнопки.
        o.lmb = held & btn::A != 0;
        o.lmb_pressed = pressed & btn::A != 0;
        let exit = modal || self.pasting || (self.sel.is_some() && self.tool.is_none()) || (self.pad.frame && self.tool.is_none());
        if pressed & btn::B != 0 && exit {
            self.pad.frame = false;
            o.pressed.push(Key::Escape);
        } else if !exit {
            o.rmb = held & btn::B != 0;
            o.rmb_pressed = pressed & btn::B != 0;
        }
        if pressed & btn::X != 0 {
            self.pad.x_used = false;
        }
        if released & btn::X != 0 && !self.pad.x_used {
            o.pressed.push(Key::R);
        }
        if pressed & btn::Y != 0 && !modal {
            o.pressed.push(Key::E);
        }
        o.x1_pressed |= pressed & btn::LB != 0;
        o.x2_pressed |= pressed & btn::RB != 0;
        if pressed & btn::BACK != 0 {
            o.pressed.push(Key::Z);
        }
        if pressed & btn::R3 != 0 {
            o.pressed.push(Key::L);
        }
        if pressed & btn::L3 != 0 {
            self.pad_act(PAct::Frame, &mut o);
        }
        if pressed & btn::START != 0 {
            self.pad.menu = true;
            self.pad.sel = 0;
            self.audio.play(520.0, 0.05, 0.08, Wave::Tri, 0.0);
        }
        o.shift |= self.pad.frame && self.tool.is_none() && !self.pasting;
        (o.mx, o.my) = (self.pad.cur.0 as i32, self.pad.cur.1 as i32);
        (o.dmx, o.dmy) = (o.mx - old.0 as i32, o.my - old.1 as i32);
        Some(o)
    }

    fn pad_act(&mut self, act: PAct, o: &mut Input) {
        let click = |g: &Game| g.audio.play(520.0, 0.05, 0.08, Wave::Tri, 0.0);
        match act {
            PAct::Key(k) => {
                self.pad.menu = false;
                o.pressed.push(k);
            }
            PAct::Ctrl(k) => {
                self.pad.menu = false;
                self.pad.frame = false;
                o.ctrl = true;
                o.pressed.push(k);
            }
            PAct::Frame => {
                self.pad.menu = false;
                self.pad.frame = !self.pad.frame;
                if self.pad.frame {
                    self.tool = None;
                    self.pasting = false;
                    self.say("РАМКА: ЗАЖМИ A И ВЕДИ КУРСОР. START - КОПИЯ И ПЕРЕНОС");
                }
                click(self);
            }
            PAct::Char(ch) => {
                if self.show_cfg.is_some() {
                    let k = [Key::Key0, Key::Key1, Key::Key2, Key::Key3, Key::Key4, Key::Key5, Key::Key6, Key::Key7, Key::Key8, Key::Key9];
                    if let Some(d) = ch.to_digit(10) {
                        o.pressed.push(k[d as usize]);
                    }
                } else {
                    o.typed.push(ch);
                }
                click(self);
            }
            PAct::Del => o.pressed.push(Key::Backspace),
            PAct::Enter => {
                self.pad.menu = false;
                o.pressed.push(Key::Enter);
            }
        }
    }

    pub(super) fn draw_pad(&self, c: &mut Image) {
        if !self.pad.active {
            return;
        }
        if self.pad.menu {
            let items = self.pad_items();
            let n = items.len();
            let (_, frame) = self.pad_cell(0, n);
            c.fill_round_rect(frame.0, frame.1, frame.2, frame.3, 10.0, 0xF01C_1722);
            c.rect_outline(frame.0, frame.1, frame.2, frame.3, 2, GOLD);
            for (i, (label, _)) in items.iter().enumerate() {
                let (r, _) = self.pad_cell(i, n);
                let on = i == self.pad.sel;
                c.fill_round_rect(r.0, r.1, r.2, r.3, 6.0, if on { 0xF05A_4E6C } else { 0xE02B_2433 });
                c.rect_outline(r.0 + 1, r.1 + 1, r.2 - 2, r.3 - 2, 2, if on { GOLD } else { PANEL_HI });
                let sc = if text_w(label, 2) <= r.2 - 10 { 2 } else { 1 };
                c.text_center(r.0 + r.2 / 2, r.1 + (r.3 - 7 * sc) / 2, label, sc, if on { GOLD } else { TEXT });
            }
            let hint = if self.pad_typing() { "A - ВВОД   Y - СТЕРЕТЬ   B - ЗАКРЫТЬ" } else { "A - ВЫБРАТЬ   B - ЗАКРЫТЬ" };
            c.text_center(frame.0 + frame.2 / 2, frame.1 + frame.3 - 20, hint, 1, DIM);
            return;
        }
        // Курсор геймпада.
        let (x, y) = (self.pad.cur.0 as i32, self.pad.cur.1 as i32);
        let col = if self.pad.frame { hex(0x7FD0FF) } else { GOLD };
        c.fill_circle(x as f32, y as f32, 7.0, 0xC010_0C14);
        c.fill_circle(x as f32, y as f32, 4.0, col);
        for r in [(x - 14, y - 1, 6, 2), (x + 8, y - 1, 6, 2), (x - 1, y - 14, 2, 6), (x - 1, y + 8, 2, 6)] {
            c.fill_rect(r.0, r.1, r.2, r.3, col);
        }
        if !self.modal() {
            let hint = "A СТРОИТЬ  B СНОС/ОТМЕНА  X ПОВОРОТ  X+КРЕСТ СТОРОНА  Y НАСТРОЙКИ  LB/RB ПОСТРОЙКИ  LT/RT МАСШТАБ  START МЕНЮ";
            let w = text_w(hint, 1) + 12;
            c.fill_round_rect((self.w - w) / 2, TOP_H + 4, w, 16, 4.0, 0xB01C_1722);
            c.text_center(self.w / 2, TOP_H + 8, hint, 1, DIM);
        }
    }
}
