//! Игровой слой: камера, инструменты строительства, интерфейс и отрисовка мира.

use crate::assets::*;
use crate::engine::audio::{Audio, Wave};
use crate::engine::font::text_w;
use crate::engine::*;
use crate::net::{Cmd, Mode, Net, NetEvent, Op, StateMsg};
use crate::world::*;

const TOP_H: i32 = 44;
const BAR_H: i32 = 88;
const PANEL: Color = 0xF02B_2433;
const PANEL_HI: Color = hex(0x4A4058);
const GOLD: Color = hex(0xF0C24B);
const TEXT: Color = hex(0xF3ECDD);
const DIM: Color = hex(0x9A93A8);
const GREEN: Color = hex(0x7ED67A);
const RED: Color = hex(0xE8553A);
pub const SAVE_FILE: &str = "alembic_save.txt";

pub fn fmt_money(v: f64) -> String {
    if v < 10_000.0 {
        format!("{}", v.floor() as i64)
    } else if v < 1e6 {
        format!("{:.1}K", v / 1e3)
    } else if v < 1e9 {
        format!("{:.2}M", v / 1e6)
    } else {
        format!("{:.2}B", v / 1e9)
    }
}

/// Постройка в буфере обмена: смещение от левого верхнего угла выделения.
#[derive(Clone, Copy)]
struct ClipItem {
    dx: i32,
    dy: i32,
    kind: Kind,
    dir: Dir,
    level: u8,
}

struct Floater {
    x: f32,
    y: f32,
    text: String,
    t: f32,
}

pub struct Game {
    pub world: World,
    assets: Assets,
    pub audio: Audio,
    pub cam_x: f32,
    pub cam_y: f32,
    pub zoom: i32,
    pub tool: Option<Kind>,
    pub dir: Dir,
    pub tab: usize,
    pub show_help: bool,
    pub show_codex: bool,
    pub show_net: bool,
    pub show_path: bool,
    /// Метка скачанного обновления и просьба перезапуститься (F5).
    pub update_ready: Option<String>,
    pub restart: bool,
    /// Выделенная рамка в тайлах (x0, y0, x1, y1), включительно.
    pub sel: Option<(i32, i32, i32, i32)>,
    sel_start: Option<(i32, i32)>,
    clip: Vec<ClipItem>,
    clip_size: (i32, i32),
    pub pasting: bool,
    pub net: Net,
    pub persist: bool,
    drag_tile: Option<(i32, i32)>,
    rdrag_tile: Option<(i32, i32)>,
    r_block: bool,
    ui_captured: bool,
    floaters: Vec<Floater>,
    toast: Option<(String, f32)>,
    banner: Option<(String, String, f32)>,
    income: f32,
    inc_acc: f64,
    inc_t: f32,
    sold_acc: f64,
    sold_t: f32,
    time: f32,
    save_t: f32,
    music_t: f32,
    sell_snd_t: f32,
    err_snd_t: f32,
    rng: Rng,
    w: i32,
    h: i32,
    mx: i32,
    my: i32,
}

fn inside(r: (i32, i32, i32, i32), x: i32, y: i32) -> bool {
    x >= r.0 && y >= r.1 && x < r.0 + r.2 && y < r.1 + r.3
}

fn panel(c: &mut Image, r: (i32, i32, i32, i32), border: Color) {
    c.fill_rect(r.0, r.1, r.2, r.3, PANEL);
    c.rect_outline(r.0, r.1, r.2, r.3, 2, border);
}

impl Game {
    pub fn new(world: World, audio: Audio, show_help: bool) -> Game {
        Game {
            world,
            assets: Assets::generate(),
            audio,
            cam_x: HUB_X as f32 + 1.5,
            cam_y: HUB_Y as f32 + 1.5,
            zoom: 2,
            tool: None,
            dir: Dir::E,
            tab: 0,
            show_help,
            show_codex: false,
            show_net: false,
            show_path: true,
            update_ready: None,
            restart: false,
            sel: None,
            sel_start: None,
            clip: Vec::new(),
            clip_size: (0, 0),
            pasting: false,
            net: Net::new(),
            persist: true,
            drag_tile: None,
            rdrag_tile: None,
            r_block: false,
            ui_captured: false,
            floaters: Vec::new(),
            toast: None,
            banner: None,
            income: 0.0,
            inc_acc: 0.0,
            inc_t: 0.0,
            sold_acc: 0.0,
            sold_t: 0.0,
            time: 0.0,
            save_t: 0.0,
            music_t: 1.0,
            sell_snd_t: 0.0,
            err_snd_t: 0.0,
            rng: Rng::new(7),
            w: 1280,
            h: 720,
            mx: -1,
            my: -1,
        }
    }

    pub fn save(&self) {
        // Чужой мир (сетевая копия) в своё сохранение не пишем.
        if self.persist && !self.world.replica {
            let _ = std::fs::write(SAVE_FILE, self.world.save());
        }
    }

    // ---------- геометрия ----------

    fn modal(&self) -> bool {
        self.show_help || self.show_codex || self.show_net
    }

    fn ts(&self) -> i32 {
        TILE * self.zoom
    }

    fn origin(&self) -> (i32, i32) {
        let ts = self.ts() as f32;
        (self.w / 2 - (self.cam_x * ts).round() as i32, self.h / 2 - (self.cam_y * ts).round() as i32)
    }

    fn tile_at(&self, mx: i32, my: i32) -> (i32, i32) {
        let (ox, oy) = self.origin();
        ((mx - ox).div_euclid(self.ts()), (my - oy).div_euclid(self.ts()))
    }

    fn toolbar_rect(&self, i: usize, n: usize) -> (i32, i32, i32, i32) {
        let (bw, gap) = (64, 4);
        let n = n as i32;
        let x0 = (self.w - (n * bw + (n - 1) * gap)) / 2;
        (x0 + i as i32 * (bw + gap), self.h - BAR_H, bw, 80)
    }

    fn tab_rect(&self, i: usize) -> (i32, i32, i32, i32) {
        let (tw, gap) = (124, 4);
        let n = TAB_NAMES.len() as i32;
        let x0 = (self.w - (n * tw + (n - 1) * gap)) / 2;
        (x0 + i as i32 * (tw + gap), self.h - BAR_H - 28, tw, 24)
    }

    fn upgrade_rect(&self, i: usize) -> (i32, i32, i32, i32) {
        (self.w - 206, TOP_H + 38 + i as i32 * 44, 198, 40)
    }

    fn over_ui(&self, x: i32, y: i32) -> bool {
        let n = Kind::in_tab(self.tab).len();
        y < TOP_H
            || (0..n).any(|i| inside(self.toolbar_rect(i, n), x, y))
            || (0..TAB_NAMES.len()).any(|i| inside(self.tab_rect(i), x, y))
            || inside((self.w - 214, TOP_H + 8, 214, 30 + N_UPGRADES as i32 * 44 + 6), x, y)
    }

    fn sprite(&self, kind: Kind, frame: usize) -> &Image {
        let f = &self.assets.kinds[kind.0 as usize];
        &f[frame % f.len()]
    }

    // ---------- обновление ----------

    fn say(&mut self, s: &str) {
        self.toast = Some((s.to_string(), 3.0));
    }

    fn error(&mut self, s: &str) {
        self.say(s);
        if self.err_snd_t <= 0.0 {
            self.audio.play(150.0, 0.14, 0.12, Wave::Square, 0.0);
            self.err_snd_t = 0.25;
        }
    }

    fn select_tool(&mut self, kind: Kind) {
        if self.world.level < kind.unlock_level() {
            self.error(&format!("{}: ОТКРОЕТСЯ ПОСЛЕ КОНТРАКТА {}", kind.name(), kind.unlock_level()));
        } else if self.tool == Some(kind) {
            self.tool = None;
        } else {
            self.tool = Some(kind);
            self.audio.play(520.0, 0.05, 0.08, Wave::Tri, 0.0);
        }
    }

    fn try_place(&mut self, kind: Kind, x: i32, y: i32, dir: Dir, loud: bool) {
        if self.net.mode == Mode::Client {
            // Клиент только просит хоста; здесь - лишь предварительная проверка ради подсказок.
            let same = self.world.get(x, y).map_or(false, |b| b.kind == kind && b.dir == dir);
            match self.world.can_place(kind, x, y) {
                Err(e) => {
                    if loud {
                        self.error(e);
                    }
                }
                Ok(()) if same => {}
                Ok(()) if self.world.money < self.world.cost(kind) => self.error(ERR_MONEY),
                Ok(()) => {
                    self.net.send_cmd(&Cmd::Place(kind, x, y, dir));
                    self.audio.play(620.0, 0.05, 0.1, Wave::Tri, 0.0);
                }
            }
            return;
        }
        match self.world.place(kind, x, y, dir) {
            Ok(true) => {
                self.net.broadcast_op(&Op::Set(x, y, kind, dir));
                self.audio.play(620.0 + self.rng.f32() * 80.0, 0.05, 0.1, Wave::Tri, 0.0)
            }
            Ok(false) => {}
            Err(e) => {
                if loud || e == ERR_MONEY {
                    self.error(e);
                }
            }
        }
    }

    pub fn notify_update(&mut self, tag: String) {
        self.say(&format!("СКАЧАНО ОБНОВЛЕНИЕ {}. F5 - ПЕРЕЗАПУСТИТЬ СЕЙЧАС", tag));
        self.audio.play(880.0, 0.25, 0.08, Wave::Tri, 0.0);
        self.update_ready = Some(tag);
    }

    /// Постройки внутри рамки (лавка не в счёт).
    fn selected(&self) -> Vec<ClipItem> {
        let mut v = Vec::new();
        if let Some((x0, y0, x1, y1)) = self.sel {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if let Some(b) = self.world.get(x, y).filter(|b| b.kind != Kind::Hub) {
                        v.push(ClipItem { dx: x - x0, dy: y - y0, kind: b.kind, dir: b.dir, level: b.level });
                    }
                }
            }
        }
        v
    }

    /// Ctrl+C копирует выделенное, Ctrl+X ещё и сносит его (перенос). Сразу включается вставка.
    pub fn copy_selection(&mut self, cut: bool) {
        let items = self.selected();
        let (x0, y0, x1, y1) = match self.sel {
            Some(r) if !items.is_empty() => r,
            _ => {
                self.error("СНАЧАЛА ВЫДЕЛИ ПОСТРОЙКИ: SHIFT + ЛКМ");
                return;
            }
        };
        if cut {
            for it in &items {
                self.remove_at(x0 + it.dx, y0 + it.dy);
            }
        }
        let verb = if cut { "ВЫРЕЗАНО" } else { "СКОПИРОВАНО" };
        self.say(&format!("{}: {}. ЛКМ - ВСТАВИТЬ, R - ПОВЕРНУТЬ", verb, items.len()));
        self.clip = items;
        self.clip_size = (x1 - x0 + 1, y1 - y0 + 1);
        self.pasting = true;
        self.tool = None;
        self.sel = None;
        self.audio.play(700.0, 0.08, 0.1, Wave::Tri, 0.0);
    }

    fn delete_selection(&mut self) {
        let items = self.selected();
        if let (Some((x0, y0, _, _)), false) = (self.sel, items.is_empty()) {
            for it in &items {
                self.remove_at(x0 + it.dx, y0 + it.dy);
            }
            self.say(&format!("СНЕСЕНО ПОСТРОЕК: {}", items.len()));
            self.audio.play(240.0, 0.12, 0.12, Wave::Tri, 0.0);
            self.sel = None;
        }
    }

    /// Поворачивает буфер обмена на 90 градусов по часовой стрелке.
    pub fn rotate_clip(&mut self) {
        let (w, h) = self.clip_size;
        for it in &mut self.clip {
            let (dx, dy) = (it.dx, it.dy);
            it.dx = h - 1 - dy;
            it.dy = dx;
            it.dir = it.dir.cw();
        }
        self.clip_size = (h, w);
    }

    fn clip_cost(&self) -> f64 {
        self.clip.iter().map(|it| self.world.cost(it.kind) * (2f64.powi(it.level as i32) - 1.0)).sum()
    }

    /// Ставит буфер обмена левым верхним углом в (ox, oy). Возвращает, сколько построек встало.
    pub fn paste_at(&mut self, ox: i32, oy: i32) -> usize {
        let items = self.clip.clone();
        let mut ok = 0;
        for it in &items {
            let (x, y) = (ox + it.dx, oy + it.dy);
            if self.world.can_place(it.kind, x, y).is_err() || self.world.level < it.kind.unlock_level() {
                continue;
            }
            if self.net.mode == Mode::Client {
                // Клиент просит хоста: постройка, затем её уровни.
                self.net.send_cmd(&Cmd::Place(it.kind, x, y, it.dir));
                for _ in 1..it.level {
                    self.net.send_cmd(&Cmd::LevelUp(x, y));
                }
                ok += 1;
                continue;
            }
            match self.world.place(it.kind, x, y, it.dir) {
                Ok(true) => self.net.broadcast_op(&Op::Set(x, y, it.kind, it.dir)),
                Ok(false) => {}
                Err(_) => continue,
            }
            ok += 1;
            while self.world.get(x, y).map_or(false, |b| b.level < it.level) {
                match self.world.level_up(x, y) {
                    Ok(l) => self.net.broadcast_op(&Op::Level(x, y, l)),
                    Err(_) => break,
                }
            }
        }
        if ok == items.len() {
            self.say(&format!("ВСТАВЛЕНО ПОСТРОЕК: {}", ok));
            self.audio.play(620.0, 0.08, 0.12, Wave::Tri, 0.0);
        } else {
            self.error(&format!("ВСТАВЛЕНО {} ИЗ {}: НЕТ ДЕНЕГ ИЛИ МЕСТО НЕ ПОДХОДИТ", ok, items.len()));
        }
        ok
    }

    /// Улучшает машину под курсором (клавиша U).
    fn level_up_at(&mut self, x: i32, y: i32) {
        let res = if self.net.mode == Mode::Client {
            match self.world.level_cost(x, y) {
                None => Err("ЭТО НЕЛЬЗЯ УЛУЧШИТЬ"),
                Some(c) if c > self.world.money => Err(ERR_MONEY),
                Some(_) => {
                    self.net.send_cmd(&Cmd::LevelUp(x, y));
                    Ok(0)
                }
            }
        } else {
            self.world.level_up(x, y)
        };
        match res {
            Ok(l) => {
                if l > 0 {
                    self.net.broadcast_op(&Op::Level(x, y, l));
                    self.say(&format!("МАШИНА УЛУЧШЕНА ДО УРОВНЯ {} (X{})", l, level_mult(l)));
                }
                self.audio.play(700.0, 0.15, 0.1, Wave::Tri, 0.0);
                self.audio.play(1050.0, 0.2, 0.08, Wave::Tri, 0.07);
            }
            Err(e) => self.error(e),
        }
    }

    /// Боковые кнопки мыши листают открытые постройки по всем вкладкам подряд.
    fn cycle_tool(&mut self, step: i32) {
        let level = self.world.level;
        let all: Vec<(usize, Kind)> = (0..TAB_NAMES.len())
            .flat_map(|t| Kind::in_tab(t).into_iter().map(move |k| (t, k)))
            .filter(|(_, k)| k.unlock_level() <= level)
            .collect();
        if all.is_empty() {
            return;
        }
        let n = all.len() as i32;
        let idx = match self.tool.and_then(|cur| all.iter().position(|(_, k)| *k == cur)) {
            Some(i) => (i as i32 + step).rem_euclid(n),
            // Ничего не выбрано: начинаем с текущей вкладки.
            None => all.iter().position(|(t, _)| *t == self.tab).unwrap_or(0) as i32,
        };
        let (tab, kind) = all[idx as usize];
        self.tab = tab;
        self.tool = Some(kind);
        self.audio.play(520.0, 0.05, 0.08, Wave::Tri, 0.0);
    }

    fn belt_dir(&mut self, x: i32, y: i32, d: Dir) {
        if self.net.mode == Mode::Client {
            self.net.send_cmd(&Cmd::BeltDir(x, y, d));
            return;
        }
        let changed = self.world.get(x, y).map_or(false, |b| b.kind.is_belt() && b.dir != d);
        self.world.set_belt_dir(x, y, d);
        if changed {
            self.net.broadcast_op(&Op::Dir(x, y, d));
        }
    }

    fn remove_at(&mut self, x: i32, y: i32) -> bool {
        if self.net.mode == Mode::Client {
            let ok = self.world.get(x, y).map_or(false, |b| b.kind != Kind::Hub);
            if ok {
                self.net.send_cmd(&Cmd::Remove(x, y));
            }
            return ok;
        }
        let ok = self.world.remove(x, y);
        if ok {
            self.net.broadcast_op(&Op::Clear(x, y));
        }
        ok
    }

    fn buy_upgrade(&mut self, i: usize) {
        let res = if self.net.mode == Mode::Client {
            match self.world.upgrade_cost(i) {
                None => Err("МАКСИМАЛЬНЫЙ УРОВЕНЬ"),
                Some(c) if c > self.world.money => Err(ERR_MONEY),
                Some(_) => {
                    self.net.send_cmd(&Cmd::Upgrade(i as u8));
                    Ok(())
                }
            }
        } else {
            self.world.buy_upgrade(i)
        };
        match res {
            Ok(()) => {
                self.audio.play(660.0, 0.2, 0.12, Wave::Tri, 0.0);
                self.audio.play(990.0, 0.3, 0.1, Wave::Tri, 0.08);
            }
            Err(e) => self.error(e),
        }
    }

    /// Возвращает игрока из чужой комнаты в его собственный мир.
    fn restore_own_world(&mut self) {
        if !self.world.replica {
            return;
        }
        let own = std::fs::read_to_string(SAVE_FILE).ok().and_then(|t| World::load(&t));
        self.world = own.unwrap_or_else(|| World::new(self.time.to_bits() as u64 + 1));
        self.tool = None;
    }

    /// Сетевые события: хост раздаёт мир и исполняет команды, клиент повторяет за хостом.
    fn net_update(&mut self, dt: f32, tx: i32, ty: i32) {
        self.net.set_cursor(tx, ty);
        for ev in self.net.poll(dt) {
            match ev {
                NetEvent::Hello(id, _) => self.net.send_world(id, &self.world.save()),
                NetEvent::Cmd(c) => match c {
                    Cmd::Place(k, x, y, d) => {
                        if k.unlock_level() <= self.world.level && self.world.place(k, x, y, d) == Ok(true) {
                            self.net.broadcast_op(&Op::Set(x, y, k, d));
                        }
                    }
                    Cmd::Remove(x, y) => {
                        self.remove_at(x, y);
                    }
                    Cmd::BeltDir(x, y, d) => self.belt_dir(x, y, d),
                    Cmd::LevelUp(x, y) => {
                        if let Ok(l) = self.world.level_up(x, y) {
                            self.net.broadcast_op(&Op::Level(x, y, l));
                        }
                    }
                    Cmd::Upgrade(i) => {
                        if (i as usize) < N_UPGRADES {
                            let _ = self.world.buy_upgrade(i as usize);
                        }
                    }
                },
                NetEvent::World(text) => {
                    if let Some(mut w) = World::load(&text) {
                        self.save();
                        w.replica = true;
                        self.world = w;
                        self.tool = None;
                        self.show_net = false;
                        self.say("ТЫ В ЧУЖОЙ КОМНАТЕ. СТРОЙТЕ ВМЕСТЕ!");
                    }
                }
                NetEvent::Op(o) => match o {
                    Op::Set(x, y, k, d) => self.world.force_set(k, x, y, d),
                    Op::Clear(x, y) => self.world.force_clear(x, y),
                    Op::Dir(x, y, d) => self.world.set_belt_dir(x, y, d),
                    Op::Level(x, y, l) => self.world.force_level(x, y, l),
                },
                NetEvent::State(s) => {
                    if self.world.replica {
                        self.world.apply_state(s.money, s.level as usize, s.delivered, s.upgrades, &s.discovered);
                        self.income = s.income;
                    }
                }
                NetEvent::Info(s) => self.say(&s),
                NetEvent::Lost(s) => {
                    self.restore_own_world();
                    self.say(&s);
                }
            }
        }
        if self.net.state_due() {
            self.net.broadcast_state(&StateMsg {
                money: self.world.money,
                level: self.world.level as u32,
                delivered: self.world.delivered,
                upgrades: self.world.upgrades,
                income: self.income,
                discovered: self.world.discovered.clone(),
            });
        }
    }

    fn net_panel(&self) -> (i32, i32, i32, i32) {
        ((self.w - 600) / 2, (self.h - 420) / 2, 600, 420)
    }

    fn net_button(&self, i: i32) -> (i32, i32, i32, i32) {
        let p = self.net_panel();
        (p.0 + 20 + i * 285, p.1 + 84, 275, 34)
    }

    fn net_row(&self, i: usize) -> (i32, i32, i32, i32) {
        let p = self.net_panel();
        (p.0 + 20, p.1 + 162 + i as i32 * 28, 560, 26)
    }

    fn click_net(&mut self, mx: i32, my: i32) {
        if !inside(self.net_panel(), mx, my) {
            self.show_net = false;
            return;
        }
        let b0 = inside(self.net_button(0), mx, my);
        let b1 = inside(self.net_button(1), mx, my);
        match self.net.mode {
            Mode::Offline => {
                if b0 {
                    self.net.host_game();
                } else if b1 {
                    self.net.refresh();
                } else {
                    let hit = (0..self.net.lobbies.len()).find(|i| inside(self.net_row(*i), mx, my));
                    if let Some(i) = hit {
                        let id = self.net.lobbies[i].id;
                        self.net.join(id);
                    }
                }
            }
            Mode::Host | Mode::Client => {
                if b0 {
                    self.net.leave();
                    self.restore_own_world();
                }
            }
        }
    }

    pub fn update(&mut self, inp: &Input, dt: f32, w: i32, h: i32) {
        self.w = w;
        self.h = h;
        self.mx = inp.mx;
        self.my = inp.my;
        self.time += dt;
        let (tx, ty) = self.tile_at(inp.mx, inp.my);
        if inp.x1_pressed {
            self.cycle_tool(-1);
        }
        if inp.x2_pressed {
            self.cycle_tool(1);
        }

        for k in &inp.pressed {
            let num = match k {
                Key::Key1 => 0,
                Key::Key2 => 1,
                Key::Key3 => 2,
                Key::Key4 => 3,
                Key::Key5 => 4,
                Key::Key6 => 5,
                Key::Key7 => 6,
                Key::Key8 => 7,
                Key::Key9 => 8,
                _ => usize::MAX,
            };
            let list = Kind::in_tab(self.tab);
            if num < list.len() {
                self.select_tool(list[num]);
                continue;
            }
            match k {
                Key::R if self.pasting => self.rotate_clip(),
                Key::F5 if self.update_ready.is_some() => self.restart = true,
                Key::R => self.dir = self.dir.cw(),
                Key::U => self.level_up_at(tx, ty),
                Key::P => self.show_path = !self.show_path,
                Key::Tab => {
                    self.tab = (self.tab + 1) % TAB_NAMES.len();
                    self.audio.play(440.0, 0.04, 0.06, Wave::Tri, 0.0);
                }
                Key::C if inp.ctrl => self.copy_selection(false),
                Key::X if inp.ctrl => self.copy_selection(true),
                Key::V if inp.ctrl => {
                    if self.clip.is_empty() {
                        self.error("БУФЕР ПУСТ: ВЫДЕЛИ РАМКОЙ (SHIFT + ЛКМ) И НАЖМИ CTRL+C");
                    } else {
                        self.pasting = true;
                        self.tool = None;
                        self.sel = None;
                    }
                }
                Key::Delete | Key::Backspace => self.delete_selection(),
                Key::C => {
                    self.show_codex = !self.show_codex;
                    self.show_help = false;
                    self.show_net = false;
                }
                Key::F2 => {
                    self.show_net = !self.show_net;
                    self.show_help = false;
                    self.show_codex = false;
                    if self.show_net {
                        self.net.ensure();
                    }
                }
                Key::Escape => {
                    if self.modal() {
                        self.show_help = false;
                        self.show_codex = false;
                        self.show_net = false;
                    } else if self.pasting || self.sel.is_some() {
                        self.pasting = false;
                        self.sel = None;
                    } else {
                        self.tool = None;
                    }
                }
                Key::F1 | Key::H => {
                    self.show_help = !self.show_help;
                    self.show_codex = false;
                }
                Key::M => {
                    self.audio.muted = !self.audio.muted;
                    let msg = if self.audio.muted { "ЗВУК ВЫКЛЮЧЕН" } else { "ЗВУК ВКЛЮЧЁН" };
                    self.say(msg);
                }
                Key::Q => match self.world.get(tx, ty).map(|b| (b.kind, b.dir)) {
                    Some((k, d)) if k != Kind::Hub => {
                        self.tool = Some(k);
                        self.dir = d;
                        self.tab = k.def().tab as usize;
                    }
                    _ => self.tool = None,
                },
                _ => {}
            }
        }

        // Камера.
        let ts = self.ts() as f32;
        let pan = 700.0 / ts * dt;
        self.cam_x += inp.pan_x * pan;
        self.cam_y += inp.pan_y * pan;
        if inp.wheel != 0.0 && !self.modal() {
            let nz = (self.zoom + if inp.wheel > 0.0 { 1 } else { -1 }).clamp(1, 4);
            if nz != self.zoom {
                let nts = (TILE * nz) as f32;
                let (ox, oy) = ((inp.mx - self.w / 2) as f32, (inp.my - self.h / 2) as f32);
                self.cam_x += ox / ts - ox / nts;
                self.cam_y += oy / ts - oy / nts;
                self.zoom = nz;
            }
        }

        // Мышь.
        if !inp.lmb {
            self.ui_captured = false;
            self.drag_tile = None;
            self.sel_start = None;
        }
        if !inp.rmb {
            self.r_block = false;
            self.rdrag_tile = None;
        }
        if self.modal() {
            if inp.lmb_pressed {
                self.ui_captured = true;
                if self.show_net {
                    self.click_net(inp.mx, inp.my);
                } else {
                    self.show_help = false;
                    self.show_codex = false;
                }
            }
        } else {
            if inp.lmb_pressed && self.over_ui(inp.mx, inp.my) {
                self.ui_captured = true;
                self.click_ui(inp.mx, inp.my);
            }
            let busy = self.ui_captured || self.pasting || self.sel_start.is_some() || inp.shift;
            if inp.mmb || (inp.lmb && self.tool.is_none() && !busy) {
                self.cam_x -= inp.dmx as f32 / ts;
                self.cam_y -= inp.dmy as f32 / ts;
            }
            if !self.ui_captured {
                self.world_mouse(inp, tx, ty);
            }
        }
        self.cam_x = self.cam_x.clamp(0.0, W as f32);
        self.cam_y = self.cam_y.clamp(0.0, H as f32);

        // Сеть, симуляция и события.
        self.net_update(dt, tx, ty);
        self.world.update(dt.min(0.1));
        let sold = std::mem::take(&mut self.world.pending_sold);
        if sold > 0.0 {
            self.inc_acc += sold;
            self.sold_acc += sold;
            if self.sell_snd_t <= 0.0 {
                self.audio.play(1250.0 + self.rng.f32() * 200.0, 0.06, 0.025, Wave::Sine, 0.0);
                self.sell_snd_t = 0.2;
            }
        }
        self.inc_t += dt;
        if self.inc_t >= 1.0 {
            let rate = (self.inc_acc / self.inc_t as f64) as f32;
            self.income = if self.income == 0.0 { rate } else { self.income * 0.7 + rate * 0.3 };
            self.inc_acc = 0.0;
            self.inc_t = 0.0;
        }
        self.sold_t += dt;
        if self.sold_t >= 0.8 {
            if self.sold_acc > 0.0 {
                let x = HUB_X as f32 + 0.6 + self.rng.f32() * 1.8;
                self.floaters.push(Floater {
                    x,
                    y: HUB_Y as f32 - 0.1,
                    text: format!("+${}", fmt_money(self.sold_acc)),
                    t: 0.0,
                });
            }
            self.sold_acc = 0.0;
            self.sold_t = 0.0;
        }
        for (it, bonus) in std::mem::take(&mut self.world.pending_new) {
            self.say(&format!("НОВИНКА В КАТАЛОГЕ: {}  +${}", it.name(), fmt_money(bonus)));
            self.audio.play(880.0, 0.25, 0.08, Wave::Tri, 0.0);
            self.audio.play(1320.0, 0.3, 0.06, Wave::Tri, 0.09);
        }
        if std::mem::take(&mut self.world.pending_levelups) > 0 {
            let lvl = self.world.level;
            let opened: Vec<&str> =
                (0..N_BUILDABLE as u8).map(Kind).filter(|k| k.unlock_level() == lvl).map(|k| k.name()).collect();
            let sub = if lvl == FINAL_LEVEL {
                "МАГИСТЕРИЙ СОЗДАН. ТЫ - АЛХИМИЧЕСКИЙ МАГНАТ!".to_string()
            } else if !opened.is_empty() {
                format!("ОТКРЫТО: {}", opened.join(", "))
            } else {
                "НОВЫЙ КОНТРАКТ УЖЕ ЖДЁТ".to_string()
            };
            self.banner = Some(("КОНТРАКТ ВЫПОЛНЕН!".to_string(), sub, 5.0));
            for (i, f) in [523.3, 659.3, 784.0, 1046.5].iter().enumerate() {
                self.audio.play(*f, 0.5, 0.12, Wave::Tri, i as f32 * 0.11);
            }
        }

        // Таймеры.
        for f in &mut self.floaters {
            f.t += dt;
        }
        self.floaters.retain(|f| f.t < 1.4);
        if let Some(t) = &mut self.toast {
            t.1 -= dt;
            if t.1 <= 0.0 {
                self.toast = None;
            }
        }
        if let Some(b) = &mut self.banner {
            b.2 -= dt;
            if b.2 <= 0.0 {
                self.banner = None;
            }
        }
        self.sell_snd_t -= dt;
        self.err_snd_t -= dt;
        self.music_t -= dt;
        if self.music_t <= 0.0 {
            // Тихая генеративная мелодия в пентатонике.
            let scale = [196.0, 220.0, 261.6, 293.7, 329.6, 392.0, 440.0, 523.3];
            let f = scale[self.rng.range(0, scale.len() as i32) as usize];
            self.audio.play(f, 1.6, 0.035, Wave::Sine, 0.0);
            if self.rng.range(0, 3) == 0 {
                self.audio.play(f * 1.5, 1.4, 0.02, Wave::Sine, 0.2);
            }
            self.music_t = [0.45, 0.9, 0.9, 1.35][self.rng.range(0, 4) as usize];
        }
        self.save_t += dt;
        if self.save_t >= 45.0 {
            self.save_t = 0.0;
            self.save();
        }
    }

    fn click_ui(&mut self, mx: i32, my: i32) {
        for i in 0..TAB_NAMES.len() {
            if inside(self.tab_rect(i), mx, my) {
                self.tab = i;
                self.audio.play(440.0, 0.04, 0.06, Wave::Tri, 0.0);
                return;
            }
        }
        let list = Kind::in_tab(self.tab);
        for (i, kind) in list.iter().enumerate() {
            if inside(self.toolbar_rect(i, list.len()), mx, my) {
                self.select_tool(*kind);
                return;
            }
        }
        for i in 0..N_UPGRADES {
            if inside(self.upgrade_rect(i), mx, my) {
                self.buy_upgrade(i);
                return;
            }
        }
    }

    fn world_mouse(&mut self, inp: &Input, tx: i32, ty: i32) {
        let cur = (tx, ty);
        // ПКМ сначала снимает вставку и выделение, и только потом сносит постройки.
        if inp.rmb_pressed && (self.pasting || self.sel.is_some()) {
            self.pasting = false;
            self.sel = None;
            self.r_block = true;
        }
        if self.pasting {
            if inp.lmb_pressed {
                let (w, h) = self.clip_size;
                self.paste_at(tx - w / 2, ty - h / 2);
            }
            return;
        }
        // Shift + ЛКМ тянет рамку выделения.
        if inp.shift && inp.lmb_pressed {
            self.sel_start = Some(cur);
        }
        if let Some(s) = self.sel_start {
            if inp.lmb {
                self.sel = Some((s.0.min(tx), s.1.min(ty), s.0.max(tx), s.1.max(ty)));
            }
            return;
        }
        if let Some(kind) = self.tool {
            if inp.lmb {
                match self.drag_tile {
                    None => {
                        self.try_place(kind, tx, ty, self.dir, true);
                    }
                    Some(mut last) => {
                        let mut guard = 0;
                        while last != cur && guard < 96 {
                            guard += 1;
                            let (dx, dy) = (cur.0 - last.0, cur.1 - last.1);
                            let step = if dx.abs() >= dy.abs() { (dx.signum(), 0) } else { (0, dy.signum()) };
                            if kind.is_belt() {
                                // Конвейер сам поворачивается по движению мыши.
                                self.dir = Dir::from_delta(step.0, step.1);
                                self.belt_dir(last.0, last.1, self.dir);
                            }
                            last = (last.0 + step.0, last.1 + step.1);
                            self.try_place(kind, last.0, last.1, self.dir, false);
                        }
                    }
                }
                self.drag_tile = Some(cur);
            }
        }
        if inp.rmb_pressed && self.tool.is_some() && self.world.get(tx, ty).is_none() {
            self.tool = None;
            self.r_block = true;
        }
        if inp.rmb && !self.r_block && self.rdrag_tile != Some(cur) {
            self.rdrag_tile = Some(cur);
            if self.remove_at(tx, ty) {
                self.audio.play(240.0, 0.07, 0.1, Wave::Tri, 0.0);
            }
        }
    }

    // ---------- отрисовка ----------

    pub fn draw(&mut self, c: &mut Image) {
        self.w = c.w;
        self.h = c.h;
        c.clear(hex(0x1D1A24));
        self.draw_world(c);
        self.draw_ui(c);
        if self.show_help {
            self.draw_help(c);
        } else if self.show_codex {
            self.draw_codex(c);
        } else if self.show_net {
            self.draw_net(c);
        }
    }

    fn draw_world(&self, c: &mut Image) {
        let ts = self.ts();
        let z = self.zoom;
        let (ox, oy) = self.origin();
        let x0 = (-ox).div_euclid(ts).max(0);
        let y0 = (-oy).div_euclid(ts).max(0);
        let x1 = (c.w - 1 - ox).div_euclid(ts).min(W - 1);
        let y1 = (c.h - 1 - oy).div_euclid(ts).min(H - 1);
        let a = &self.assets;
        let w = &self.world;
        let anim = (self.time * 8.0) as usize;

        // Земля и жилы.
        for y in y0..=y1 {
            for x in x0..=x1 {
                let hsh = ((x.wrapping_mul(73856093) ^ y.wrapping_mul(19349663)) as u32 >> 4) as usize;
                let v = w.vein(x, y);
                let img = if v > 0 { &a.veins[(v as usize - 1) * 2 + hsh % 2] } else { &a.ground[hsh % 4] };
                c.blit(img, ox + x * ts, oy + y * ts, z);
            }
        }

        // Конвейеры и мосты.
        for y in y0..=y1 {
            for x in x0..=x1 {
                let b = match w.get(x, y) {
                    Some(b) => b,
                    None => continue,
                };
                let (sx, sy) = (ox + x * ts, oy + y * ts);
                match b.kind.role() {
                    Role::Belt(m) => {
                        let style = (m > 1.0) as usize;
                        let fr = ((w.time * 24.0 * (w.belt_speed() * m) as f64) as i64).rem_euclid(8) as usize;
                        let back = w.feeds_from(x, y, b.dir.opp());
                        let left = w.feeds_from(x, y, b.dir.ccw());
                        let right = w.feeds_from(x, y, b.dir.cw());
                        let curve_l = !back && left && !right;
                        let curve_r = !back && right && !left;
                        let img = if curve_l {
                            &a.belt_l[style][fr]
                        } else if curve_r {
                            &a.belt_r[style][fr]
                        } else {
                            &a.belt[style][fr]
                        };
                        let rot = b.dir as u8;
                        c.blit_ex(img, sx, sy, z, rot, 255);
                        // Стыки: боковые примыкания и заглушки свободных торцов.
                        if left && !curve_l {
                            c.blit_ex(&a.belt_stub_l, sx, sy, z, rot, 255);
                        }
                        if right && !curve_r {
                            c.blit_ex(&a.belt_stub_r, sx, sy, z, rot, 255);
                        }
                        if !back && !curve_l && !curve_r {
                            c.blit_ex(&a.belt_cap, sx, sy, z, rot, 255);
                        }
                        if !w.accepts_from(x + b.dir.dx(), y + b.dir.dy(), b.dir) {
                            c.blit_ex(&a.belt_cap, sx, sy, z, rot + 2, 255);
                        }
                    }
                    Role::Bridge => c.blit(self.sprite(b.kind, 0), sx, sy, z),
                    _ => {}
                }
            }
        }

        // Предметы на конвейерах.
        for y in y0..=y1 {
            for x in x0..=x1 {
                let b = match w.get(x, y) {
                    Some(b) if b.kind.is_track() => b,
                    _ => continue,
                };
                for lane in &b.lanes {
                    for bi in lane {
                        let out = if b.kind.is_belt() { b.dir } else { bi.d };
                        let (fx, fy) = if bi.p < 0.5 {
                            let t = 0.5 * (1.0 - bi.p * 2.0);
                            (0.5 - bi.d.dx() as f32 * t, 0.5 - bi.d.dy() as f32 * t)
                        } else {
                            let t = bi.p - 0.5;
                            (0.5 + out.dx() as f32 * t, 0.5 + out.dy() as f32 * t)
                        };
                        let px = ox + x * ts + (fx * ts as f32) as i32 - ITEM * z / 2;
                        let py = oy + y * ts + (fy * ts as f32) as i32 - ITEM * z / 2;
                        c.blit(&a.items[bi.item.id()], px, py, z);
                    }
                }
            }
        }

        // Машины.
        for y in y0..=y1 {
            for x in x0..=x1 {
                let b = match w.get(x, y) {
                    Some(b) if !b.kind.is_track() && b.kind != Kind::Hub => b,
                    _ => continue,
                };
                let (sx, sy) = (ox + x * ts, oy + y * ts);
                let frame = if b.working { anim + (x * 3 + y * 5) as usize } else { 0 };
                c.blit_ex(self.sprite(b.kind, frame), sx, sy, z, b.dir as u8, 255);
                // Золотые метки уровня машины.
                for i in 0..(b.level as i32 - 1) {
                    c.fill_rect(sx + (1 + i * 4) * z, sy + 19 * z, 4 * z, 4 * z, OUTLINE);
                    c.fill_rect(sx + (2 + i * 4) * z, sy + 20 * z, 2 * z, 2 * z, GOLD);
                }
                let (cx, cy) = (sx + ts / 2, sy + ts / 2);
                let icon = |c: &mut Image, it: Item| c.blit(&a.items[it.id()], cx - ITEM * z / 2, cy - ITEM * z / 2, z);
                match b.kind.role() {
                    Role::Extractor(_) => {
                        let v = w.vein(x, y);
                        if v > 0 {
                            c.fill_circle(cx as f32, cy as f32, 3.1 * z as f32, ELEM_COLORS[v as usize - 1]);
                        }
                    }
                    Role::Filter => {
                        if let Some(it) = b.slots[1] {
                            icon(c, it);
                        }
                    }
                    Role::Storage => {
                        if let Some(it) = b.slots[0].filter(|_| b.count > 0).or(b.out) {
                            icon(c, it);
                            if z >= 2 {
                                c.text_center(cx, sy + ts - 8 * z, &format!("{}", b.count), z - 1, TEXT);
                            }
                        }
                    }
                    Role::Mixer | Role::Distiller => {
                        if let Some(it) = b.out {
                            icon(c, it);
                        }
                    }
                    _ => {}
                }
            }
        }

        // Лавка.
        let (hx, hy) = (ox + HUB_X * ts, oy + HUB_Y * ts);
        c.blit(self.sprite(Kind::Hub, 0), hx, hy, z);
        if z >= 2 {
            let s = format!("УР.{}", w.level + 1);
            c.text_center(hx + ts * 3 / 2, hy + ts * 3 - 9 * z, &s, z - 1, GOLD);
        }

        // Призрак постройки под курсором.
        let (tx, ty) = self.tile_at(self.mx, self.my);
        if !self.over_ui(self.mx, self.my) && !self.modal() && World::inb(tx, ty) {
            let (sx, sy) = (ox + tx * ts, oy + ty * ts);
            if let Some(kind) = self.tool {
                c.blit_ex(self.sprite(kind, 0), sx, sy, z, self.dir as u8, 170);
                let bad = w.can_place(kind, tx, ty).is_err() || w.money < w.cost(kind);
                c.fill_rect(sx, sy, ts, ts, if bad { 0x60E8_553A } else { 0x30FF_FFFF });
                // Стрелка направления выхода.
                let (ax, ay) = (sx + ts / 2 + self.dir.dx() * ts * 3 / 4, sy + ts / 2 + self.dir.dy() * ts * 3 / 4);
                c.fill_circle(ax as f32, ay as f32, 2.5 * z as f32, 0xD0F0_C24B);
            } else {
                c.rect_outline(sx, sy, ts, ts, z, 0x50FF_FFFF);
            }
        }

        // Рамка выделения.
        if let Some((sx0, sy0, sx1, sy1)) = self.sel {
            let (px, py) = (ox + sx0 * ts, oy + sy0 * ts);
            let (pw, ph) = ((sx1 - sx0 + 1) * ts, (sy1 - sy0 + 1) * ts);
            c.fill_rect(px, py, pw, ph, 0x3058_B4F0);
            c.rect_outline(px, py, pw, ph, z.max(2), hex(0x58B4F0));
        }

        // Призрак вставки из буфера обмена.
        if self.pasting && !self.over_ui(self.mx, self.my) && !self.modal() {
            let (cw, ch) = self.clip_size;
            let (bx, by) = (tx - cw / 2, ty - ch / 2);
            for it in &self.clip {
                let (x, y) = (bx + it.dx, by + it.dy);
                let (sx, sy) = (ox + x * ts, oy + y * ts);
                c.blit_ex(self.sprite(it.kind, 0), sx, sy, z, it.dir as u8, 170);
                if w.can_place(it.kind, x, y).is_err() {
                    c.fill_rect(sx, sy, ts, ts, 0x70E8_553A);
                }
            }
            c.rect_outline(ox + bx * ts, oy + by * ts, cw * ts, ch * ts, z.max(2), GOLD);
        }

        // Курсоры других игроков.
        for p in &self.net.peers {
            let col = [hex(0x58B4F0), hex(0xF07AD8), hex(0x7ED67A), hex(0xFFB03A)][(p.id.raw() % 4) as usize];
            let (sx, sy) = (ox + p.cx * ts, oy + p.cy * ts);
            c.rect_outline(sx, sy, ts, ts, z.max(2), col);
            c.text_center(sx + ts / 2, sy - 18, &p.name, 2, col);
        }

        // Всплывающие суммы.
        for f in &self.floaters {
            let sx = ox + (f.x * ts as f32) as i32;
            let sy = oy + ((f.y - f.t * 0.7) * ts as f32) as i32;
            let col = with_alpha(GOLD, (255.0 * (1.4 - f.t).min(1.0)) as u8);
            c.text_center(sx, sy, &f.text, 2, col);
        }
    }

    fn draw_ui(&self, c: &mut Image) {
        let w = &self.world;
        let a = &self.assets;
        let hover_ui = self.over_ui(self.mx, self.my);

        // Верхняя панель: деньги, доход, контракт.
        c.fill_rect(0, 0, c.w, TOP_H, PANEL);
        c.fill_rect(0, TOP_H, c.w, 2, PANEL_HI);
        let mut x = c.text(14, 15, &format!("${}", fmt_money(w.money)), 2, GOLD) + 16;
        x = c.text(x, 15, &format!("+{}/С", fmt_money(self.income as f64)), 2, GREEN) + 28;
        let ct = contract(w.level);
        x = c.text(x.max(250), 15, &format!("КОНТРАКТ {}:", w.level + 1), 2, DIM) + 10;
        c.blit(&a.items[ct.item.id()], x, 7, 3);
        x = c.text(x + 38, 15, &ct.item.name(), 2, TEXT) + 14;
        let bw = 150;
        c.fill_rect(x, 11, bw, 22, hex(0x14121A));
        c.fill_rect(x + 2, 13, (bw - 4) * w.delivered as i32 / ct.need.max(1) as i32, 18, hex(0x5DAA4A));
        let prog = format!("{}/{}", w.delivered, ct.need);
        c.text_center(x + bw / 2, 15, &prog, 2, TEXT);
        x = c.text(x + bw + 14, 15, &format!("+${}", fmt_money(ct.reward * w.reward_mult())), 2, GOLD);
        let help = "F2 - СЕТЬ  C - КАТАЛОГ  F1 - ПОМОЩЬ";
        let hx = c.w - text_w(help, 2) - 14;
        if hx > x + 20 {
            c.text(hx, 15, help, 2, DIM);
        }

        if let Some(tag) = &self.update_ready {
            let s = format!("ОБНОВЛЕНИЕ {} ГОТОВО: F5 - ПЕРЕЗАПУСК (ИЛИ ВСТАНЕТ ПРИ ВЫХОДЕ)", tag);
            let tw = text_w(&s, 2);
            c.fill_rect((c.w - tw) / 2 - 10, TOP_H + 6, tw + 20, 26, 0xE02B_2433);
            c.text((c.w - tw) / 2, TOP_H + 12, &s, 2, GREEN);
        }
        if self.net.online() {
            let who = if self.net.mode == Mode::Host { "ТЫ ХОСТ" } else { "В ГОСТЯХ" };
            let s = format!("СЕТЬ: {}, ИГРОКОВ {}", who, self.net.peers.len() + 1);
            c.text_sh(14, TOP_H + 12, &s, 2, GREEN);
        }
        if self.show_path && !self.modal() {
            self.draw_path(c, TOP_H + 10 + if self.net.online() { 26 } else { 0 });
        }

        // Панель улучшений.
        let ux = c.w - 214;
        panel(c, (ux, TOP_H + 8, 214, 30 + N_UPGRADES as i32 * 44 + 6), PANEL_HI);
        c.text(ux + 12, TOP_H + 18, "УЛУЧШЕНИЯ", 2, TEXT);
        let mut tip: Option<String> = None;
        for i in 0..N_UPGRADES {
            let r = self.upgrade_rect(i);
            let cost = w.upgrade_cost(i);
            let can = cost.map_or(false, |v| v <= w.money);
            let hov = inside(r, self.mx, self.my);
            c.fill_rect(r.0, r.1, r.2, r.3, if hov { hex(0x4A4058) } else { hex(0x3A3346) });
            c.rect_outline(r.0, r.1, r.2, r.3, 2, if can { GOLD } else { hex(0x4A4058) });
            c.text(r.0 + 10, r.1 + 5, UPGRADE_NAMES[i], 2, TEXT);
            let lv = format!("УР.{}", w.upgrades[i]);
            c.text(r.0 + r.2 - 10 - text_w(&lv, 2), r.1 + 5, &lv, 2, DIM);
            match cost {
                Some(v) => c.text(r.0 + 10, r.1 + 22, &format!("${}", fmt_money(v)), 2, if can { GOLD } else { DIM }),
                None => c.text(r.0 + 10, r.1 + 22, "МАКСИМУМ", 2, GREEN),
            };
            if hov {
                tip = Some(UPGRADE_DESC[i].to_string());
            }
        }

        // Вкладки построек.
        for (i, name) in TAB_NAMES.iter().enumerate() {
            let r = self.tab_rect(i);
            let sel = self.tab == i;
            let hov = inside(r, self.mx, self.my);
            c.fill_rect(r.0, r.1, r.2, r.3, if sel || hov { 0xF04A_4058 } else { PANEL });
            c.rect_outline(r.0, r.1, r.2, r.3, 2, if sel { GOLD } else { PANEL_HI });
            let col = if sel { GOLD } else { DIM };
            c.text(r.0 + (r.2 - text_w(name, 2)) / 2, r.1 + 5, name, 2, col);
        }

        // Панель построек текущей вкладки.
        let list = Kind::in_tab(self.tab);
        for (i, kind) in list.iter().enumerate() {
            let r = self.toolbar_rect(i, list.len());
            let locked = w.level < kind.unlock_level();
            let sel = self.tool == Some(*kind);
            let hov = inside(r, self.mx, self.my);
            c.fill_rect(r.0, r.1, r.2, r.3, if hov && !locked { 0xF04A_4058 } else { PANEL });
            c.rect_outline(r.0, r.1, r.2, r.3, 2, if sel { GOLD } else { PANEL_HI });
            c.blit_ex(self.sprite(*kind, 0), r.0 + 8, r.1 + 6, 2, 0, if locked { 70 } else { 255 });
            c.text(r.0 + 5, r.1 + 5, &format!("{}", i + 1), 1, if sel { GOLD } else { DIM });
            if locked {
                c.text_center(r.0 + r.2 / 2, r.1 + 24, "ЗАКРЫТО", 1, RED);
                c.text_center(r.0 + r.2 / 2, r.1 + 62, &format!("КОНТР.{}", kind.unlock_level()), 1, DIM);
            } else {
                let cost = format!("${}", fmt_money(w.cost(*kind)));
                let col = if w.money >= w.cost(*kind) { GOLD } else { RED };
                let sc = if text_w(&cost, 2) <= r.2 - 4 { 2 } else { 1 };
                c.text_center(r.0 + r.2 / 2, r.1 + 59 + (2 - sc) * 3, &cost, sc, col);
            }
            if hov {
                tip = Some(format!("{} - {}", kind.name(), kind.desc()));
            }
        }

        // Подсказка / сообщение над вкладками.
        let line_y = self.h - BAR_H - 28 - 30;
        let hint: Option<(String, Color)> = if let Some((msg, t)) = &self.toast {
            Some((msg.clone(), with_alpha(hex(0xFFD9A0), (255.0 * t.min(1.0)) as u8)))
        } else if let Some(t) = &tip {
            Some((t.clone(), TEXT))
        } else if self.pasting {
            let cost = fmt_money(self.clip_cost());
            Some((format!("ВСТАВКА (${}): ЛКМ - ПОСТАВИТЬ, R - ПОВЕРНУТЬ, ПКМ - ВЫХОД", cost), TEXT))
        } else if self.sel.is_some() {
            let n = self.selected().len();
            Some((format!("ВЫДЕЛЕНО {}: DEL - СНЕСТИ, CTRL+C - КОПИЯ, CTRL+X - ПЕРЕНОС", n), TEXT))
        } else {
            self.tool.map(|k| (format!("{}: ЛКМ - СТРОИТЬ, R - ПОВЕРНУТЬ, ПКМ - ОТМЕНА", k.name()), DIM))
        };
        if let Some((msg, col)) = hint {
            let tw = text_w(&msg, 2);
            c.fill_rect((c.w - tw) / 2 - 10, line_y - 6, tw + 20, 26, 0xD02B_2433);
            c.text((c.w - tw) / 2, line_y, &msg, 2, col);
        }

        // Информация о тайле под курсором.
        if !hover_ui && self.tool.is_none() && !self.modal() && !self.pasting {
            let (tx, ty) = self.tile_at(self.mx, self.my);
            let mut lines: Vec<String> = Vec::new();
            if let Some(b) = w.get(tx, ty) {
                lines.push(b.kind.name().to_string());
                if World::can_level(b.kind) {
                    lines.push(match w.level_cost(tx, ty) {
                        Some(cost) => {
                            format!("УР.{} (X{}). U - УЛУЧШИТЬ ЗА ${}", b.level, level_mult(b.level), fmt_money(cost))
                        }
                        None => format!("УР.{} (X{}) - МАКСИМУМ", b.level, level_mult(b.level)),
                    });
                }
                match b.kind.role() {
                    Role::Hub => lines.push("ПРОДАЁТ ВСЁ, ЧТО ПРИВЕЗУТ".to_string()),
                    Role::Belt(_) | Role::Bridge | Role::Trash | Role::Trade => {}
                    Role::Filter => match b.slots[1] {
                        Some(s) => lines.push(format!("ОБРАЗЕЦ: {}", s.name())),
                        None => lines.push("ОБРАЗЕЦ ЕЩЁ НЕ ЗАДАН".to_string()),
                    },
                    Role::Storage => match b.slots[0].filter(|_| b.count > 0) {
                        Some(s) => lines.push(format!("{} X{}", s.name(), b.count)),
                        None => lines.push("ПУСТО".to_string()),
                    },
                    _ => {
                        for s in b.slots.iter().flatten() {
                            lines.push(format!("ВНУТРИ: {}", s.name()));
                        }
                        if let Some(o) = b.out {
                            lines.push(format!("ГОТОВО: {} (${})", o.name(), fmt_money(o.value() * w.price_mult())));
                        }
                    }
                }
            }
            let v = w.vein(tx, ty);
            if v > 0 {
                lines.push(format!("ЖИЛА: {}", ELEM_NAMES[v as usize - 1]));
            }
            if !lines.is_empty() {
                let tw = lines.iter().map(|l| text_w(l, 2)).max().unwrap_or(0);
                let bh = lines.len() as i32 * 20 + 10;
                let bx = (self.mx + 18).min(c.w - tw - 24);
                let by = (self.my + 18).min(c.h - bh - 4);
                panel(c, (bx, by, tw + 20, bh), PANEL_HI);
                for (i, l) in lines.iter().enumerate() {
                    c.text(bx + 10, by + 8 + i as i32 * 20, l, 2, if i == 0 { GOLD } else { TEXT });
                }
            }
        }

        // Баннер выполненного контракта.
        if let Some((title, sub, t)) = &self.banner {
            let al = (255.0 * t.min(1.0)) as u8;
            let bw = text_w(sub, 2).max(text_w(title, 4)) + 60;
            let (bx, by) = ((c.w - bw) / 2, c.h / 4);
            c.fill_rect(bx, by, bw, 96, with_alpha(hex(0x2B2433), (al as u32 * 235 / 255) as u8));
            c.rect_outline(bx, by, bw, 96, 3, with_alpha(GOLD, al));
            c.text_center(c.w / 2, by + 16, title, 4, with_alpha(GOLD, al));
            c.text_center(c.w / 2, by + 62, sub, 2, with_alpha(TEXT, al));
        }
    }

    /// Каталог: таблица всех видов предметов (форма x смесь) и карточка выбранного.
    fn draw_codex(&self, c: &mut Image) {
        let w = &self.world;
        c.fill_rect(0, 0, c.w, c.h, 0xA010_0E16);
        let (cell, lw, info_w) = (22, 130, 400);
        let bw = 16 + lw + 15 * cell + 20 + info_w + 16;
        let bh = 46 + N_FORMS as i32 * cell + 16;
        let (bx, by) = ((c.w - bw) / 2, (c.h - bh) / 2);
        panel(c, (bx, by, bw, bh), GOLD);
        let found = w.discovered.iter().filter(|d| **d).count();
        c.text(bx + 16, by + 16, &format!("КАТАЛОГ: ОТКРЫТО {} ИЗ {}", found, N_KINDS), 2, GOLD);
        let gx = bx + 16 + lw;
        let gy = by + 46;
        let mut hover: Option<Item> = None;
        for form in 0..N_FORMS {
            let y = gy + form as i32 * cell;
            c.text(bx + 16, y + 4, FORM_NAMES[form], 2, DIM);
            for m in 0..15 {
                let x = gx + m * cell;
                let it = Item::new(m as u8 + 1, 1, form as u8);
                let known = w.discovered[it.kind_idx()];
                let hov = inside((x, y, cell, cell), self.mx, self.my);
                c.fill_rect(x + 1, y + 1, cell - 2, cell - 2, if known { hex(0x4A4058) } else { hex(0x322C3C) });
                c.blit_ex(&self.assets.items[it.id()], x + 1, y + 1, 2, 0, if known { 255 } else { 70 });
                if hov {
                    c.rect_outline(x, y, cell, cell, 2, GOLD);
                    hover = Some(it);
                }
            }
        }
        // Карточка предмета.
        let ix = gx + 15 * cell + 20;
        match hover {
            Some(it) => {
                let (machine, from) = form_recipe(it.form);
                let known = w.discovered[it.kind_idx()];
                c.fill_rect(ix, gy, 96, 96, hex(0x3A3346));
                c.blit(&self.assets.items[it.id()], ix + 8, gy + 8, 8);
                let lines = [
                    (it.name(), GOLD),
                    (String::new(), TEXT),
                    (format!("ЦЕНА: ${}", fmt_money(it.value())), TEXT),
                    (format!("ЧИСТОТА II: ${}", fmt_money(it.value() * 3.0)), DIM),
                    (format!("ЧИСТОТА III: ${}", fmt_money(it.value() * 10.0)), DIM),
                    (String::new(), TEXT),
                    ("ГДЕ ДЕЛАЮТ:".to_string(), DIM),
                    (machine, TEXT),
                    ("ИЗ ЧЕГО:".to_string(), DIM),
                    (from, TEXT),
                    (String::new(), TEXT),
                    if known {
                        ("УЖЕ ПРОДАВАЛОСЬ".to_string(), GREEN)
                    } else {
                        (format!("ПРЕМИЯ ЗА ПЕРВУЮ ПРОДАЖУ: ${}", fmt_money(it.value() * 10.0)), GOLD)
                    },
                ];
                for (i, (l, col)) in lines.iter().enumerate() {
                    c.text(ix, gy + 112 + i as i32 * 22, l, 2, *col);
                }
            }
            None => {
                let lines = [
                    "НАВЕДИ НА ПРЕДМЕТ, ЧТОБЫ УЗНАТЬ",
                    "ЦЕНУ И СПОСОБ ПОЛУЧЕНИЯ.",
                    "",
                    "СТРОКИ - ФОРМЫ ОБРАБОТКИ,",
                    "СТОЛБЦЫ - 15 СМЕСЕЙ СТИХИЙ.",
                    "",
                    "ЗА ПЕРВУЮ ПРОДАЖУ КАЖДОГО ВИДА",
                    "ЛАВКА ПЛАТИТ ПРЕМИЮ.",
                    "",
                    "ЩЕЛЧОК ИЛИ C - ЗАКРЫТЬ",
                ];
                for (i, l) in lines.iter().enumerate() {
                    c.text(ix, gy + i as i32 * 22, l, 2, if i == lines.len() - 1 { GOLD } else { TEXT });
                }
            }
        }
    }

    /// Панель под контрактом: вся цепочка от стихий до нужного предмета.
    fn draw_path(&self, c: &mut Image, y0: i32) {
        let steps = recipe_steps(contract(self.world.level).item);
        let rows = (((self.h - BAR_H - 90 - y0) / 24).max(1) as usize).min(steps.len());
        let row_w = |s: &Step| s.inputs.len() as i32 * 24 + text_w(s.machine, 2) + 62;
        let title = "ПУТЬ КОНТРАКТА (P - СКРЫТЬ)";
        let wide = steps.iter().take(rows).map(row_w).max().unwrap_or(0).max(text_w(title, 1));
        let more = steps.len() - rows;
        c.fill_rect(8, y0, wide + 14, 22 + rows as i32 * 24 + if more > 0 { 14 } else { 0 }, 0xC82B_2433);
        c.text(14, y0 + 7, title, 1, DIM);
        for (i, s) in steps.iter().take(rows).enumerate() {
            let y = y0 + 20 + i as i32 * 24;
            let mut x = 14;
            for it in &s.inputs {
                c.blit(&self.assets.items[it.id()], x, y, 2);
                x += 24;
            }
            if !s.inputs.is_empty() {
                x = c.text(x + 2, y + 3, ">", 2, DIM) + 4;
            }
            x = c.text(x, y + 3, s.machine, 2, TEXT) + 4;
            x = c.text(x, y + 3, ">", 2, DIM) + 4;
            c.blit(&self.assets.items[s.out.id()], x, y, 2);
        }
        if more > 0 {
            c.text(14, y0 + 22 + rows as i32 * 24, &format!("... И ЕЩЁ ШАГОВ: {}", more), 1, DIM);
        }
    }

    fn draw_net(&self, c: &mut Image) {
        c.fill_rect(0, 0, c.w, c.h, 0xA010_0E16);
        let p = self.net_panel();
        panel(c, p, GOLD);
        c.text(p.0 + 20, p.1 + 18, "СЕТЕВАЯ ИГРА ЧЕРЕЗ STEAM", 2, GOLD);
        let ver = format!("ВЕРСИЯ {}", crate::update::VERSION);
        c.text(p.0 + p.2 - 20 - text_w(&ver, 2), p.1 + 18, &ver, 2, DIM);
        c.text(p.0 + 20, p.1 + 48, &self.net.status, 2, if self.net.online() { GREEN } else { TEXT });
        let button = |c: &mut Image, r: (i32, i32, i32, i32), label: &str| {
            let hov = inside(r, self.mx, self.my);
            c.fill_rect(r.0, r.1, r.2, r.3, if hov { hex(0x5A4E6C) } else { hex(0x3A3346) });
            c.rect_outline(r.0, r.1, r.2, r.3, 2, GOLD);
            c.text(r.0 + (r.2 - text_w(label, 2)) / 2, r.1 + 10, label, 2, TEXT);
        };
        match self.net.mode {
            Mode::Offline => {
                button(c, self.net_button(0), "СОЗДАТЬ КОМНАТУ");
                button(c, self.net_button(1), "НАЙТИ КОМНАТЫ");
                c.text(p.0 + 20, p.1 + 136, "КОМНАТЫ (ЩЁЛКНИ, ЧТОБЫ ВОЙТИ):", 2, DIM);
                for (i, l) in self.net.lobbies.iter().enumerate() {
                    let r = self.net_row(i);
                    let hov = inside(r, self.mx, self.my);
                    c.fill_rect(r.0, r.1, r.2, r.3, if hov { hex(0x5A4E6C) } else { hex(0x3A3346) });
                    c.text(r.0 + 8, r.1 + 6, &format!("{}  ({} ИГР.)", l.name, l.members), 2, TEXT);
                }
            }
            Mode::Host | Mode::Client => {
                let label = if self.net.mode == Mode::Host { "ЗАКРЫТЬ КОМНАТУ" } else { "ВЫЙТИ ИЗ КОМНАТЫ" };
                button(c, self.net_button(0), label);
                c.text(p.0 + 20, p.1 + 136, "ИГРОКИ:", 2, DIM);
                c.text(p.0 + 20, p.1 + 166, &format!("{} (ТЫ)", self.net.my_name), 2, TEXT);
                for (i, pl) in self.net.peers.iter().enumerate() {
                    c.text(p.0 + 20, p.1 + 194 + i as i32 * 28, &pl.name, 2, TEXT);
                }
            }
        }
        let foot = [
            "НУЖЕН STEAM. ТАМ ИГРА ВИДНА КАК SPACEWAR.",
            "ДРУГ: F2, НАЙТИ КОМНАТЫ, ЩЕЛЧОК ПО КОМНАТЕ.",
            "МИР, ДЕНЬГИ И КОНТРАКТЫ - ОБЩИЕ, ОТ ХОСТА.",
        ];
        for (i, l) in foot.iter().enumerate() {
            c.text(p.0 + 20, p.1 + p.3 - 78 + i as i32 * 22, l, 2, DIM);
        }
    }

    fn draw_help(&self, c: &mut Image) {
        let lines = [
            "ALEMBIC - АЛХИМИЧЕСКИЙ ТАЙКУН",
            "",
            "КАЧАЙ ЭССЕНЦИИ СТИХИЙ ИЗ ЖИЛ, ВЕЗИ ИХ КОНВЕЙЕРАМИ",
            "В ЛАВКУ В ЦЕНТРЕ КАРТЫ И ПРОДАВАЙ. СМЕШИВАЙ, ОЧИЩАЙ",
            "И ПЕРЕРАБАТЫВАЙ - СЛОЖНЫЕ ТОВАРЫ СТОЯТ НАМНОГО ДОРОЖЕ.",
            "ВЫПОЛНЯЙ КОНТРАКТЫ, ЧТОБЫ ОТКРЫВАТЬ НОВЫЕ МАШИНЫ.",
            "",
            "TAB            СЛЕДУЮЩАЯ ВКЛАДКА ПОСТРОЕК",
            "1-9            ВЫБОР ПОСТРОЙКИ НА ВКЛАДКЕ",
            "МЫШЬ 4 / 5     ПРЕДЫДУЩАЯ / СЛЕДУЮЩАЯ ПОСТРОЙКА",
            "ЛКМ            СТРОИТЬ (КОНВЕЙЕР - ТЯНУТЬ МЫШЬЮ)",
            "ПКМ            СНЕСТИ (ДЕНЬГИ ВОЗВРАЩАЮТСЯ)",
            "R              ПОВЕРНУТЬ ПОСТРОЙКУ",
            "Q              ВЗЯТЬ ПОСТРОЙКУ ПОД КУРСОРОМ",
            "U              УЛУЧШИТЬ МАШИНУ ПОД КУРСОРОМ",
            "SHIFT + ЛКМ    ВЫДЕЛИТЬ РАМКОЙ",
            "DEL            СНЕСТИ ВЫДЕЛЕННОЕ",
            "CTRL+C / X / V КОПИЯ / ПЕРЕНОС / ВСТАВКА (R - ПОВОРОТ)",
            "P              ПОКАЗАТЬ / СКРЫТЬ ПУТЬ КОНТРАКТА",
            "C              КАТАЛОГ ВСЕХ ПРЕДМЕТОВ И РЕЦЕПТОВ",
            "F2             СЕТЕВАЯ ИГРА ЧЕРЕЗ STEAM",
            "WASD / ЛКМ     ДВИГАТЬ КАМЕРУ",
            "КОЛЕСО         МАСШТАБ",
            "M              ЗВУК ВКЛ/ВЫКЛ",
            "F1             ЭТА СПРАВКА",
            "ИГРА ОБНОВЛЯЕТСЯ САМА; F5 - ПЕРЕЗАПУСК ПОСЛЕ СКАЧИВАНИЯ.",
            "",
            "МАШИНЫ БЕРУТ СЫРЬЁ СЗАДИ И С БОКОВ, ВЫДАЮТ ВПЕРЁД.",
            "НА МАШИНЕ НАРИСОВАНО ТО, ЧТО ОНА ВЫПУСКАЕТ.",
            "ИГРА СОХРАНЯЕТСЯ САМА.",
            "",
            "ЩЁЛКНИ МЫШЬЮ, ЧТОБЫ НАЧАТЬ",
        ];
        c.fill_rect(0, 0, c.w, c.h, 0xA010_0E16);
        let bw = 700.min(c.w - 20);
        let bh = lines.len() as i32 * 22 + 40;
        let (bx, by) = ((c.w - bw) / 2, (c.h - bh) / 2);
        panel(c, (bx, by, bw, bh), GOLD);
        for (i, l) in lines.iter().enumerate() {
            let y = by + 22 + i as i32 * 22;
            if i == 0 || i == lines.len() - 1 {
                c.text_center(c.w / 2, y, l, 2, GOLD);
            } else {
                c.text(bx + 30, y, l, 2, TEXT);
            }
        }
    }
}
