//! Сенсорное управление: экранные кнопки и перевод касаний в привычный игре ввод мыши и клавиш.
//!
//! Платформа отдаёт один палец как мышь с зажатой левой кнопкой, два пальца - как среднюю
//! кнопку (камера) и колесо (щипок). Здесь к этому добавляется то, чего у пальца нет:
//! - кнопки по углам экрана вместо клавиш; постройки ставятся касанием, как мышью;
//! - режимы "СНОС" (палец работает как правая кнопка) и "РАМКА" (как SHIFT + ЛКМ);
//! - выбранная постройка: первое касание выбирает её и показывает сведения и действия,
//!   второе - щёлкает по ней (сторона машины, окно настройки);
//! - в окнах касание срабатывает при отпускании, а движение пальца листает списки;
//! - вставка из буфера ставится там, где палец отпустили.

use super::*;

/// Палец дрожит сильнее мыши: сдвиг меньше этого - всё ещё касание, а не движение.
const SLOP: i32 = 14;
const BW: i32 = 100;
const BH: i32 = 56;
const GAP: i32 = 6;
/// Кнопок в левом углу: меню, слой, провода, масштаб.
const LEFT_BUTTONS: i32 = 5;

#[derive(Clone, Copy, PartialEq)]
enum TAct {
    /// Нажать клавишу.
    Key(Key),
    /// Нажать клавишу с CTRL.
    Ctrl(Key),
    /// Нажать клавишу так, будто курсор стоит на выбранной постройке.
    Focus(Key),
    Demolish,
    Select,
    Menu,
    Cancel,
    Zoom(f32),
    RemoveFocus,
    Ip(char),
    IpDel,
    /// Переключить сторону выбранной машины (абсолютное направление).
    Side(Dir),
}

struct TBtn {
    r: (i32, i32, i32, i32),
    label: String,
    sub: String,
    act: TAct,
    on: bool,
}

#[derive(Default)]
pub struct Touch {
    /// Включено ли сенсорное управление (телефон или ключ --touch).
    pub on: bool,
    demolish: bool,
    select: bool,
    menu: bool,
    /// Постройка, выбранная касанием.
    pub focus: Option<(i32, i32)>,
    /// Палец опустился на экранную кнопку: до отпускания мир его не видит.
    captured: bool,
    /// Палец опустился на панель самой игры (постройки, вкладки, улучшения).
    down_ui: bool,
    down_at: Option<(i32, i32)>,
    moved: bool,
    /// В жесте участвовал второй палец: это камера, а не касание.
    multi: bool,
    was_down: bool,
    scroll: i32,
    last: (i32, i32),
}

impl Touch {
    /// Насколько может сдвинуться указатель, чтобы нажатие осталось щелчком.
    pub fn slop(&self) -> i32 {
        if self.on {
            SLOP
        } else {
            6
        }
    }
}

impl Game {
    /// Середина тайла на экране.
    fn tile_center(&self, x: i32, y: i32) -> (i32, i32) {
        let (ox, oy) = self.origin();
        let ts = self.ts();
        (ox + x * ts + ts / 2, oy + y * ts + ts / 2)
    }

    /// Где "курсор", пока палец не на экране: на выбранной постройке, на месте вставки
    /// или нигде (тогда ничего не подсвечивается).
    fn touch_cursor(&self) -> (i32, i32) {
        if self.pasting {
            self.touch.last
        } else if let Some((x, y)) = self.touch.focus.filter(|_| self.tool.is_none() && !self.modal()) {
            self.tile_center(x, y)
        } else {
            (-9999, self.h + 9999)
        }
    }

    /// Кнопки в углу: от нижнего края вверх рядами, слева или справа от панели построек.
    fn corner(&self, right: bool, list: Vec<(&str, String, TAct, bool)>) -> Vec<TBtn> {
        let per = self.per_row();
        list.into_iter()
            .enumerate()
            .map(|(i, (label, sub, act, on))| {
                let (col, row) = (i as i32 % per, i as i32 / per);
                let x = if right { self.w - 8 - BW - col * (BW + GAP) } else { 8 + col * (BW + GAP) };
                TBtn { r: (x, self.h - 8 - BH - row * (BH + GAP), BW, BH), label: label.to_string(), sub, act, on }
            })
            .collect()
    }

    /// Сколько рядов кнопок в левом углу (над ними рисуется счётчик кадров).
    pub fn touch_left_rows(&self) -> i32 {
        (LEFT_BUTTONS + self.per_row() - 1) / self.per_row()
    }

    /// Сколько кнопок встаёт в ряд сбоку от панели построек.
    fn per_row(&self) -> i32 {
        // Панель построек бывает шире вкладок: кнопки не должны на неё залезать.
        let bar_w = (TAB_NAMES.len() as i32 * 128).max(Kind::in_tab(self.tab).len() as i32 * 68) - 4;
        (((self.w - bar_w) / 2 - 16 + GAP) / (BW + GAP)).max(1)
    }

    fn menu_items() -> [(&'static str, Key); 10] {
        [
            ("КАТАЛОГ", Key::C),
            ("СТАТИСТИКА", Key::T),
            ("ГИЛЬДИИ", Key::G),
            ("ВИТРАЖ И ВИТОК", Key::V),
            ("ЧЕРТЕЖИ", Key::B),
            ("СЕТЕВАЯ ИГРА", Key::F2),
            ("ПУТЬ КОНТРАКТА", Key::P),
            ("ЗВУК", Key::M),
            ("СЧЁТЧИК FPS", Key::F3),
            ("СПРАВКА", Key::F1),
        ]
    }

    fn menu_rect(&self) -> (i32, i32, i32, i32) {
        let rows = self.touch_left_rows();
        let h = 5 * 58 + 14;
        (8, self.h - 8 - rows * (BH + GAP) - h - 4, 2 * 206 + 14, h)
    }

    /// Цифровая клавиатура для адреса комнаты: справа от окна сетевой игры.
    fn ip_pad(&self) -> Vec<TBtn> {
        let p = self.net_panel();
        let keys = ['1', '2', '3', '4', '5', '6', '7', '8', '9', '.', '0', '<'];
        keys.iter()
            .enumerate()
            .map(|(i, ch)| {
                let r = (p.0 + p.2 + 12 + (i as i32 % 3) * 74, p.1 + 80 + (i as i32 / 3) * 62, 68, 56);
                let act = if *ch == '<' { TAct::IpDel } else { TAct::Ip(*ch) };
                TBtn { r, label: ch.to_string(), sub: String::new(), act, on: false }
            })
            .collect()
    }

    fn touch_buttons(&self) -> Vec<TBtn> {
        let no = String::new;
        if self.modal() {
            let mut v = vec![TBtn { r: (self.w - 8 - 112, 6, 112, 48), label: "ЗАКРЫТЬ".to_string(), sub: no(), act: TAct::Key(Key::Escape), on: true }];
            if self.show_net && self.net.mode == Mode::Offline {
                v.extend(self.ip_pad());
            }
            return v;
        }
        let layer = if self.layer == 1 { "НАВЕРХ" } else { "ВНИЗ" };
        let mut v = self.corner(
            false,
            vec![
                ("МЕНЮ", no(), TAct::Menu, self.touch.menu),
                (layer, no(), TAct::Key(Key::Z), false),
                ("ПРОВОДА", no(), TAct::Key(Key::L), self.show_wires),
                ("+", no(), TAct::Zoom(1.0), false),
                ("-", no(), TAct::Zoom(-1.0), false),
            ],
        );
        if self.touch.menu {
            let m = self.menu_rect();
            for (i, (label, key)) in Self::menu_items().iter().enumerate() {
                let r = (m.0 + 7 + (i as i32 % 2) * 206, m.1 + 7 + (i as i32 / 2) * 58, 200, 52);
                v.push(TBtn { r, label: label.to_string(), sub: no(), act: TAct::Key(*key), on: false });
            }
        }
        let mut list: Vec<(&str, String, TAct, bool)> = Vec::new();
        let dir = ["ВВЕРХ", "ВПРАВО", "ВНИЗ", "ВЛЕВО"][self.dir as usize];
        if self.pasting {
            list.push(("ПОВОРОТ", no(), TAct::Key(Key::R), false));
            list.push(("ОТМЕНА", no(), TAct::Cancel, false));
        } else if self.sel.is_some() {
            list.push(("КОПИЯ", no(), TAct::Ctrl(Key::C), false));
            list.push(("ПЕРЕНОС", no(), TAct::Ctrl(Key::X), false));
            list.push(("УДАЛИТЬ", no(), TAct::Key(Key::Delete), false));
            list.push(("ОТМЕНА", no(), TAct::Cancel, false));
        } else {
            list.push(("СНОС", no(), TAct::Demolish, self.touch.demolish));
            list.push(("РАМКА", no(), TAct::Select, self.touch.select));
            if self.tool.is_some() {
                list.push(("ПОВОРОТ", dir.to_string(), TAct::Key(Key::R), false));
            }
            if self.tool.is_some() || self.touch.demolish || self.touch.select {
                list.push(("ОТМЕНА", no(), TAct::Cancel, false));
            } else if !self.clip.is_empty() || !self.clip_nodes.is_empty() {
                list.push(("ВСТАВКА", no(), TAct::Ctrl(Key::V), false));
            }
            // Действия с выбранной постройкой или деталью логики.
            if let Some((x, y)) = self.touch.focus.filter(|_| self.tool.is_none() && !self.touch.demolish && !self.touch.select) {
                let node = self.wires_on() && self.world.node(x, y).is_some();
                let b = self.world.get(x, y);
                if let (false, Some(cost)) = (node, self.world.level_cost(x, y)) {
                    list.push(("УЛУЧШИТЬ", format!("${}", fmt_money(cost)), TAct::Focus(Key::U), false));
                }
                let cfg = if node { self.world.node(x, y).map_or(false, |n| lk_has_param(n.lk())) } else { b.map_or(false, |b| has_panel(b.kind)) };
                if cfg {
                    list.push(("ОКНО", "НАСТРОЕК".to_string(), TAct::Focus(Key::E), false));
                }
                if let (false, Some(b)) = (node, b) {
                    if b.kind.role() == Role::Filter {
                        list.push(("СБРОС", "ОБРАЗЦА".to_string(), TAct::Focus(Key::F), false));
                    }
                    if b.kind != Kind::Hub {
                        list.push(("ВЗЯТЬ", "ТАКУЮ ЖЕ".to_string(), TAct::Focus(Key::Q), false));
                    }
                }
                if node || b.map_or(false, |b| b.kind != Kind::Hub) {
                    list.push(("СНЕСТИ", no(), TAct::RemoveFocus, false));
                }
            }
        }
        v.extend(self.corner(true, list));
        v.extend(self.side_buttons());
        v
    }

    /// Крупные кнопки сторон вокруг выбранной машины: палец не попадает в край тайла,
    /// а здесь видно, что стоит на каждой стороне, и касание переключает её.
    fn side_buttons(&self) -> Vec<TBtn> {
        let free = self.tool.is_none() && !self.touch.demolish && !self.touch.select && !self.pasting && self.sel.is_none();
        let Some((x, y)) = self.touch.focus.filter(|_| free) else { return Vec::new() };
        if self.wires_on() && self.world.node(x, y).is_some() {
            return Vec::new();
        }
        let Some(b) = self.world.get(x, y) else { return Vec::new() };
        if self.world.next_side(x, y, 0).is_none() {
            return Vec::new();
        }
        let (cx, cy) = self.tile_center(x, y);
        let half = self.ts() / 2 + 4;
        let (sw, sh) = (92, 46);
        [Dir::N, Dir::E, Dir::S, Dir::W]
            .into_iter()
            .map(|d| {
                let rel = rel_side(b.dir, d);
                let rej = can_reject(b.kind.role()) && b.cfg & 7 == rel as u64 + 1;
                let mode = if rej { "ОТКАЗ" } else { ["АВТО", "ВХОД", "ВЫХОД", "ЗАКРЫТО"][(b.ports[rel] as usize).min(3)] };
                let r = match d {
                    Dir::N => (cx - sw / 2, cy - half - sh, sw, sh),
                    Dir::S => (cx - sw / 2, cy + half, sw, sh),
                    Dir::W => (cx - half - sw, cy - sh / 2, sw, sh),
                    Dir::E => (cx + half, cy - sh / 2, sw, sh),
                };
                let arrow = ["ВВЕРХ", "ВПРАВО", "ВНИЗ", "ВЛЕВО"][d as usize];
                TBtn { r, label: mode.to_string(), sub: arrow.to_string(), act: TAct::Side(d), on: mode != "АВТО" }
            })
            .collect()
    }

    fn touch_act(&mut self, act: TAct, o: &mut Input) {
        let click = |g: &Game| g.audio.play(520.0, 0.05, 0.08, Wave::Tri, 0.0);
        match act {
            TAct::Key(k) => {
                if k == Key::Delete {
                    self.touch.select = false;
                }
                self.touch.menu = false;
                o.pressed.push(k);
            }
            TAct::Ctrl(k) => {
                self.touch.select = false;
                o.ctrl = true;
                o.pressed.push(k);
            }
            TAct::Focus(k) => {
                if let Some((x, y)) = self.touch.focus {
                    (o.mx, o.my) = self.tile_center(x, y);
                    o.pressed.push(k);
                    if k == Key::Q {
                        self.touch.focus = None;
                    }
                }
            }
            TAct::Demolish => {
                self.touch.demolish = !self.touch.demolish;
                self.touch.select = false;
                self.tool = None;
                self.pasting = false;
                self.sel = None;
                if self.touch.demolish {
                    self.say("СНОС: ВЕДИ ПАЛЬЦЕМ ПО ПОСТРОЙКАМ. ДЕНЬГИ ВОЗВРАЩАЮТСЯ");
                }
                click(self);
            }
            TAct::Select => {
                self.touch.select = !self.touch.select;
                self.touch.demolish = false;
                self.tool = None;
                self.pasting = false;
                if self.touch.select {
                    self.say("РАМКА: ПРОВЕДИ ПАЛЬЦЕМ ПО ПОСТРОЙКАМ");
                }
                click(self);
            }
            TAct::Menu => {
                self.touch.menu = !self.touch.menu;
                click(self);
            }
            TAct::Cancel => {
                self.touch.demolish = false;
                self.touch.select = false;
                o.pressed.push(Key::Escape);
            }
            TAct::Zoom(d) => {
                (o.mx, o.my) = (self.w / 2, self.h / 2);
                o.wheel = d;
            }
            TAct::RemoveFocus => {
                if let Some((x, y)) = self.touch.focus.take() {
                    if self.remove_at(x, y) {
                        self.audio.play(240.0, 0.07, 0.1, Wave::Tri, 0.0);
                    }
                }
            }
            TAct::Ip(ch) => {
                if self.ip_text.len() < 21 {
                    self.ip_text.push(ch);
                }
                click(self);
            }
            TAct::Side(d) => {
                if let Some((x, y)) = self.touch.focus {
                    self.cycle_side(x, y, d);
                }
            }
            TAct::IpDel => {
                self.ip_text.pop();
                click(self);
            }
        }
    }

    /// Переводит кадр сенсорного ввода во ввод, который понимает остальная игра.
    pub(super) fn touch_adapt(&mut self, inp: &Input) -> Input {
        let mut o = inp.clone();
        let (mx, my) = (inp.mx, inp.my);
        let released = self.touch.was_down && !inp.lmb;
        self.touch.was_down = inp.lmb;
        if inp.mmb {
            self.touch.multi = true;
        }

        // Инструмент выбран на панели: режимы пальца и выбранная постройка сбрасываются.
        if self.tool.is_some() || self.pasting {
            self.touch.demolish = false;
            self.touch.select = false;
            self.touch.focus = None;
        }
        if let Some((x, y)) = self.touch.focus {
            let gone = self.world.get(x, y).is_none() && self.world.node(x, y).is_none();
            if gone || layer_of(y) != self.layer {
                self.touch.focus = None;
            }
        }

        if inp.lmb_pressed {
            self.touch.captured = false;
            self.touch.multi = inp.mmb;
            self.touch.moved = false;
            self.touch.scroll = 0;
            self.touch.down_at = Some((mx, my));
            self.touch.down_ui = !self.modal() && self.over_ui(mx, my);
            let hit = self.touch_buttons().into_iter().find(|b| inside(b.r, mx, my)).map(|b| b.act);
            if let Some(act) = hit {
                self.touch.captured = true;
                (o.mx, o.my) = self.touch_cursor();
                self.touch_act(act, &mut o);
            } else if self.touch.menu {
                // Касание мимо меню закрывает его и больше ничего не делает.
                self.touch.menu = false;
                self.touch.captured = true;
            }
        }
        if let (true, Some((dx, dy))) = (inp.lmb, self.touch.down_at) {
            if (mx - dx).abs() + (my - dy).abs() > SLOP {
                self.touch.moved = true;
            }
        }
        let tap = released && !self.touch.moved && !self.touch.multi && !self.touch.captured;
        let down_at = self.touch.down_at.unwrap_or((mx, my));

        if self.touch.captured {
            o.lmb = false;
            o.lmb_pressed = false;
            (o.dmx, o.dmy) = (0, 0);
            if !inp.lmb_pressed {
                (o.mx, o.my) = self.touch_cursor();
            }
        } else if self.modal() {
            // Окна: щелчок - при отпускании, движение пальца листает.
            o.lmb_pressed = false;
            if inp.lmb && self.touch.moved {
                self.touch.scroll += inp.dmy;
                if self.touch.scroll.abs() >= 24 {
                    o.wheel = self.touch.scroll.signum() as f32;
                    self.touch.scroll -= 24 * self.touch.scroll.signum();
                }
            }
            if tap {
                // В каталоге и статистике касание внутри окна только показывает сведения.
                let (bw, bh) = (1180.min(self.w - 20), 640.min(self.h - 20));
                let stats = ((self.w - bw) / 2, (self.h - bh) / 2, bw, bh);
                let pbtn = (0..3).any(|i| inside(self.codex_pbtn(i), down_at.0, down_at.1));
                let look = (self.show_codex && inside(self.codex_geom().0, down_at.0, down_at.1) && !pbtn)
                    || (self.show_stats && inside(stats, down_at.0, down_at.1));
                (o.mx, o.my) = down_at;
                if !look {
                    o.lmb = true;
                    o.lmb_pressed = true;
                }
            } else if !inp.lmb {
                (o.mx, o.my) = if self.show_codex { self.touch.last } else { (-9999, self.h + 9999) };
            }
        } else if self.touch.down_ui {
            // Панели самой игры работают как под мышью.
            if !inp.lmb {
                (o.mx, o.my) = self.touch_cursor();
            }
        } else if self.pasting {
            // Вставка: палец ведёт призрак, ставится он при отпускании.
            o.lmb = false;
            o.lmb_pressed = false;
            if inp.lmb {
                self.touch.last = (mx, my);
            } else {
                (o.mx, o.my) = self.touch.last;
                if released && !self.touch.multi {
                    o.lmb = true;
                    o.lmb_pressed = true;
                }
            }
        } else {
            if self.touch.demolish {
                o.rmb = inp.lmb;
                o.rmb_pressed = inp.lmb_pressed;
                o.lmb = false;
                o.lmb_pressed = false;
            }
            o.shift = self.touch.select;
            if tap && self.tool.is_none() && !self.touch.demolish && !self.touch.select {
                // Первое касание выбирает постройку, второе по ней же - щёлкает.
                let t = self.tile_at(down_at.0, down_at.1);
                let wires = self.wires_on();
                let any = self.world.get(t.0, t.1).is_some() || (wires && self.world.node(t.0, t.1).is_some());
                if self.touch.focus != Some(t) {
                    self.click_at = None;
                    self.touch.focus = any.then_some(t);
                    if any {
                        self.audio.play(520.0, 0.04, 0.06, Wave::Tri, 0.0);
                    }
                }
            }
            if !inp.lmb {
                (o.mx, o.my) = self.touch_cursor();
            }
        }
        if inp.lmb && !self.touch.captured {
            self.touch.last = (mx, my);
        }
        if !inp.lmb {
            self.touch.captured = false;
            self.touch.down_ui = false;
            self.touch.down_at = None;
            if !inp.mmb {
                self.touch.multi = false;
            }
        }
        o
    }

    pub(super) fn draw_touch(&self, c: &mut Image) {
        // Рамка вокруг выбранной постройки.
        if let Some((x, y)) = self.touch.focus.filter(|_| self.tool.is_none() && !self.modal() && !self.pasting) {
            let (cx, cy) = self.tile_center(x, y);
            let ts = self.ts();
            if !self.over_ui(cx, cy) {
                c.rect_outline(cx - ts / 2 - 2, cy - ts / 2 - 2, ts + 4, ts + 4, 3, GOLD);
            }
        }
        if self.touch.menu && !self.modal() {
            panel(c, self.menu_rect(), GOLD);
        }
        for b in self.touch_buttons() {
            let r = b.r;
            let fill = if b.on { 0xF05A_4E6C } else { 0xE02B_2433 };
            c.fill_round_rect(r.0, r.1, r.2, r.3, 8.0, fill);
            c.rect_outline(r.0 + 2, r.1 + 2, r.2 - 4, r.3 - 4, 2, if b.on { GOLD } else { PANEL_HI });
            let col = if b.on { GOLD } else { TEXT };
            if b.sub.is_empty() {
                let sc = if b.label.chars().count() <= 2 { 4 } else { 2 };
                c.text_center(r.0 + r.2 / 2, r.1 + (r.3 - 7 * sc) / 2, &b.label, sc, col);
            } else {
                c.text_center(r.0 + r.2 / 2, r.1 + 9, &b.label, 2, col);
                let sc = if text_w(&b.sub, 2) <= r.2 - 8 { 2 } else { 1 };
                c.text_center(r.0 + r.2 / 2, r.1 + 31 + (2 - sc) * 3, &b.sub, sc, DIM);
            }
        }
    }
}
