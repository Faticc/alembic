//! Игровой слой: камера, инструменты строительства, интерфейс и отрисовка мира.

use crate::assets::*;
use crate::engine::audio::{Audio, Wave};
use crate::engine::font::{has_glyph, text_w};
use crate::engine::*;
use crate::net::{Cmd, Mode, Net, NetEvent, Op, Presence, StateMsg};
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
/// Как часто хост рассылает состояние логики вне очереди, когда срабатывают датчики.
const LOGIC_PUSH: f32 = 0.1;

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
#[derive(Clone, Copy, PartialEq, Debug)]
struct ClipItem {
    dx: i32,
    dy: i32,
    kind: Kind,
    dir: Dir,
    level: u8,
    ports: [u8; 4],
    cfg: u64,
}

/// Деталь логики в буфере обмена: лежит поверх построек, со своей настройкой.
#[derive(Clone, Copy, PartialEq, Debug)]
struct ClipNode {
    dx: i32,
    dy: i32,
    kind: Kind,
    dir: Dir,
    param: u64,
}

/// Чертёж из библиотеки: буфер обмена под именем.
#[derive(Clone, PartialEq, Debug)]
struct Blueprint {
    name: String,
    size: (i32, i32),
    items: Vec<ClipItem>,
    nodes: Vec<ClipNode>,
}

/// Библиотека чертежей лежит отдельно от сохранения: чертежи общие для всех игр.
pub const BP_FILE: &str = "alembic_blueprints.txt";
const BP_HEAD: &str = "ALEMBIC-BP1";
const BP_NAME_MAX: usize = 24;

/// Сколько стоит поставить чертёж целиком (постройки с уровнями и детали логики).
fn bp_cost(w: &World, items: &[ClipItem], nodes: &[ClipNode]) -> f64 {
    let b: f64 = items.iter().map(|it| w.cost(it.kind) * (2f64.powi(it.level as i32) - 1.0)).sum();
    b + nodes.iter().map(|n| w.cost(n.kind)).sum::<f64>()
}

/// Файл библиотеки: "B ширина высота имя", затем постройки "T dx dy вид направление уровень
/// стороны настройка" и детали логики "N dx dy вид направление настройка".
fn bp_to_text(list: &[Blueprint]) -> String {
    let mut s = format!("{}\n", BP_HEAD);
    for bp in list {
        s.push_str(&format!("B {} {} {}\n", bp.size.0, bp.size.1, bp.name));
        for it in &bp.items {
            s.push_str(&format!("T {} {} {} {} {} {} {}\n", it.dx, it.dy, it.kind.0, it.dir as u8, it.level, pack_ports(it.ports), it.cfg));
        }
        for n in &bp.nodes {
            s.push_str(&format!("N {} {} {} {} {}\n", n.dx, n.dy, n.kind.0, n.dir as u8, n.param));
        }
    }
    s
}

/// Читает библиотеку. Непонятные строки, неизвестные постройки и клетки за пределами чертежа
/// пропускаются: файл можно править руками и переносить между версиями.
fn bp_from_text(text: &str) -> Vec<Blueprint> {
    let mut list: Vec<Blueprint> = Vec::new();
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some(BP_HEAD) {
        return list;
    }
    for line in lines {
        let line = line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("B ") {
            let mut it = rest.splitn(3, ' ');
            let (w, h) = (it.next().and_then(|v| v.parse::<i32>().ok()), it.next().and_then(|v| v.parse::<i32>().ok()));
            let name: String = it.next().unwrap_or("").trim().chars().filter(|c| has_glyph(*c)).take(BP_NAME_MAX).collect();
            match (w, h) {
                (Some(w), Some(h)) if w >= 1 && h >= 1 && w <= W && h <= H && !name.is_empty() => {
                    list.push(Blueprint { name, size: (w, h), items: Vec::new(), nodes: Vec::new() })
                }
                // Испорченный заголовок: его клетки не должны попасть в предыдущий чертёж.
                _ => list.push(Blueprint { name: String::new(), size: (0, 0), items: Vec::new(), nodes: Vec::new() }),
            }
            continue;
        }
        let (tag, rest) = match line.split_once(' ') {
            Some(p) => p,
            None => continue,
        };
        let v: Vec<u64> = rest.split_whitespace().filter_map(|v| v.parse().ok()).collect();
        let bp = match list.last_mut() {
            Some(bp) => bp,
            None => continue,
        };
        if v.len() < 5 || v[0] >= bp.size.0 as u64 || v[1] >= bp.size.1 as u64 || v[2] > 255 {
            continue;
        }
        let (dx, dy, dir) = (v[0] as i32, v[1] as i32, Dir::from_u8((v[3] % 4) as u8));
        let kind = match Kind::from_u8(v[2] as u8) {
            Some(k) if k != Kind::Hub => k,
            _ => continue,
        };
        match (tag, kind.role()) {
            ("N", Role::Logic(lk)) => {
                if !bp.nodes.iter().any(|n| (n.dx, n.dy) == (dx, dy)) {
                    bp.nodes.push(ClipNode { dx, dy, kind, dir, param: clean_lparam(lk, v[4]) });
                }
            }
            ("T", role) if !matches!(role, Role::Logic(_)) && v.len() >= 7 => {
                if !bp.items.iter().any(|t| (t.dx, t.dy) == (dx, dy)) {
                    let level = if World::can_level(kind) { v[4].clamp(1, MAX_LEVEL as u64) as u8 } else { 1 };
                    let ports = if port_modes(kind) > 0 { unpack_ports(v[5] as u8) } else { [PORT_AUTO; 4] };
                    bp.items.push(ClipItem { dx, dy, kind, dir, level, ports, cfg: clean_cfg(kind.role(), v[6]) });
                }
            }
            _ => {}
        }
    }
    list.retain(|bp| !bp.name.is_empty() && (!bp.items.is_empty() || !bp.nodes.is_empty()));
    // Одинаковые имена: остаётся последний чертёж, как при сохранении в игре.
    let mut out: Vec<Blueprint> = Vec::new();
    for bp in list {
        match out.iter_mut().find(|o| o.name == bp.name) {
            Some(o) => *o = bp,
            None => out.push(bp),
        }
    }
    out
}

/// Действие кнопки в окне настройки постройки.
#[derive(Clone, Copy)]
enum Act {
    Mask(u8),
    Purity(u8),
    Form(u8),
    Boxed(u8),
    /// Пропорция: какое из двух чисел и на сколько изменить.
    Ratio(usize, i32),
    /// Сортировочный узел: какой из трёх выходов настраивается.
    Slot(usize),
    /// Код телепорта: разряд и на сколько его изменить.
    Digit(usize, i32),
    CodeClear,
    /// Деталь логики: изменить число на столько; выбрать режим исполнителя.
    Num(i64),
    Mode(u64),
}

struct Btn {
    r: (i32, i32, i32, i32),
    label: String,
    icon: Option<Item>,
    act: Act,
    on: bool,
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
    /// Открытое окно настройки постройки: её тайл.
    pub show_cfg: Option<(i32, i32)>,
    /// Какой выход сортировочного узла выбран в окне настройки.
    pub cfg_tab: usize,
    /// Какую чистоту показывает каталог: 1..3.
    pub codex_p: u8,
    pub show_stats: bool,
    stats_scroll: i32,
    pub show_guild: bool,
    pub show_great: bool,
    /// Кнопка нового витка нажата один раз и ждёт подтверждения.
    prestige_arm: bool,
    state_n: u32,
    /// Показан ли слой проводов (клавиша L; сам включается, когда в руках деталь логики).
    pub show_wires: bool,
    /// Какой слой карты на экране: 0 - поверхность, 1 - подземелье (клавиша Z).
    pub layer: i32,
    /// Окно настройки открыто для детали логики, а не для постройки.
    pub cfg_node: bool,
    /// Счётчик кадров в углу (F3); `fps` и `frame_ms` присылает главный цикл.
    pub show_fps: bool,
    pub fps: f32,
    pub frame_ms: f32,
    /// Просьба главному циклу переключить полноэкранный режим (F11, ALT+ENTER).
    pub toggle_fullscreen: bool,
    /// Метка скачанного обновления и просьба перезапуститься (F5).
    pub update_ready: Option<String>,
    /// Где нажали ЛКМ без инструмента: если мышь не сдвинулась, это щелчок по стороне машины.
    click_at: Option<(i32, i32)>,
    pub restart: bool,
    /// Выделенная рамка в тайлах (x0, y0, x1, y1), включительно.
    pub sel: Option<(i32, i32, i32, i32)>,
    sel_start: Option<(i32, i32)>,
    clip: Vec<ClipItem>,
    clip_nodes: Vec<ClipNode>,
    clip_size: (i32, i32),
    /// Библиотека чертежей (клавиша B): список, набираемое имя, прокрутка, ждущее подтверждения удаление.
    pub show_bp: bool,
    blueprints: Vec<Blueprint>,
    bp_name: String,
    bp_scroll: usize,
    bp_del_arm: Option<usize>,
    /// Файл библиотеки; None - ничего не читать и не писать (кадры-самопроверки).
    bp_file: Option<String>,
    /// Пауза перед следующей внеочередной рассылкой состояния логики.
    logic_t: f32,
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

fn ping_color(ms: f32) -> Color {
    if ms < 80.0 {
        GREEN
    } else if ms < 200.0 {
        GOLD
    } else {
        RED
    }
}

/// Строки подсказки про ауры [скорость, экономия сырья, двойной выход]; нулевые пропускаются.
fn aura_lines(s: [f32; 3], prefix: &str) -> Vec<String> {
    let pct = |v: f32| (v * 100.0).round() as i32;
    let mut v = Vec::new();
    if s[0] > 0.0 {
        v.push(format!("{}СКОРОСТЬ +{}% (ОГОНЬ)", prefix, pct(s[0])));
    }
    if s[2] > 0.0 {
        v.push(format!("{}ДВОЙНОЙ ВЫХОД {}% (ЗЕМЛЯ)", prefix, pct(aura_chance(s[2]))));
    }
    if s[1] > 0.0 {
        v.push(format!("{}ЭКОНОМИЯ СЫРЬЯ {}% (ВОДА)", prefix, pct(aura_chance(s[1]))));
    }
    v
}

/// Сколько построек этой роли настроено на код телепорта.
fn w_count(w: &World, role: Role, code: usize) -> usize {
    w.tiles.iter().flatten().filter(|b| b.kind.role() == role && tele_code(b.cfg) == Some(code)).count()
}

/// Что деталь логики делает сейчас - одной строкой для подсказки и окна настройки.
fn node_text(n: &Node) -> String {
    match n.lk() {
        Lk::Wire => "ПРОВОД".to_string(),
        Lk::SenJam => "СИГНАЛ ПРИ ЗАТОРЕ".to_string(),
        Lk::SenStore => format!("СИГНАЛ, КОГДА СКЛАД ЗАПОЛНЕН НА {}%", n.param),
        Lk::SenItem => format!("ИМПУЛЬС НА ПРЕДМЕТ: {}", Cond::from_bits(n.param).text()),
        Lk::Timer => format!("ПЕРИОД {:.1} С: ПОЛОВИНУ ВКЛЮЧЁН, ПОЛОВИНУ ВЫКЛЮЧЕН", n.param as f64 / 10.0),
        Lk::Counter => format!("КАЖДЫЙ {}-Й ИМПУЛЬС (СЕЙЧАС {})", n.param, n.count),
        Lk::Act => (if n.param & 1 == 0 { "СИГНАЛ ОСТАНАВЛИВАЕТ ПОСТРОЙКУ" } else { "СИГНАЛ ВКЛЮЧАЕТ ПОСТРОЙКУ" }).to_string(),
        Lk::And => "ВСЕ ВХОДЫ".to_string(),
        Lk::Or => "ЛЮБОЙ ВХОД".to_string(),
        Lk::Not => "НИ ОДНОГО ВХОДА".to_string(),
        Lk::Latch => "СЗАДИ - ВКЛЮЧИТЬ, СБОКУ - СБРОСИТЬ".to_string(),
    }
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
            show_cfg: None,
            cfg_tab: 0,
            codex_p: 1,
            show_stats: false,
            stats_scroll: 0,
            show_guild: false,
            show_great: false,
            prestige_arm: false,
            state_n: 0,
            show_wires: false,
            layer: 0,
            cfg_node: false,
            show_fps: true,
            fps: 0.0,
            frame_ms: 0.0,
            toggle_fullscreen: false,
            update_ready: None,
            click_at: None,
            restart: false,
            sel: None,
            sel_start: None,
            clip: Vec::new(),
            clip_nodes: Vec::new(),
            clip_size: (0, 0),
            show_bp: false,
            blueprints: Vec::new(),
            bp_name: String::new(),
            bp_scroll: 0,
            bp_del_arm: None,
            bp_file: None,
            logic_t: 0.0,
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
        self.show_help || self.show_codex || self.show_net || self.show_stats || self.show_guild || self.show_great || self.show_bp || self.show_cfg.is_some()
    }

    /// Виден ли слой проводов прямо сейчас.
    fn wires_on(&self) -> bool {
        self.show_wires || self.tool.map_or(false, |k| matches!(k.role(), Role::Logic(_))) || (self.pasting && !self.clip_nodes.is_empty())
    }

    fn close_modals(&mut self) {
        self.show_help = false;
        self.show_codex = false;
        self.show_net = false;
        self.show_stats = false;
        self.show_guild = false;
        self.show_great = false;
        self.show_bp = false;
        self.bp_del_arm = None;
        self.prestige_arm = false;
        self.show_cfg = None;
    }

    // ---------- гильдии и Великое Делание ----------

    fn wide_panel(&self) -> (i32, i32, i32, i32) {
        let (bw, bh) = (1120.min(self.w - 20), 640.min(self.h - 20));
        ((self.w - bw) / 2, (self.h - bh) / 2, bw, bh)
    }

    fn guild_card(&self, g: usize) -> (i32, i32, i32, i32) {
        let p = self.wide_panel();
        (p.0 + 20 + g as i32 * 272, p.1 + 50, 264, 190)
    }

    fn skin_btn(&self, g: usize) -> (i32, i32, i32, i32) {
        let r = self.guild_card(g);
        (r.0 + 12, r.1 + 148, r.2 - 24, 30)
    }

    fn offer_card(&self, i: usize) -> (i32, i32, i32, i32) {
        let p = self.wide_panel();
        (p.0 + 20 + i as i32 * 362, p.1 + 290, 354, 190)
    }

    fn offer_btn(&self, i: usize) -> (i32, i32, i32, i32) {
        let r = self.offer_card(i);
        (r.0 + 12, r.1 + 146, r.2 - 24, 32)
    }

    fn prestige_btn(&self) -> (i32, i32, i32, i32) {
        let p = self.wide_panel();
        (p.0 + p.2 - 470, p.1 + p.3 - 70, 440, 40)
    }

    fn click_guild(&mut self, mx: i32, my: i32) {
        if !inside(self.wide_panel(), mx, my) {
            self.close_modals();
            return;
        }
        if let Some(i) = (0..self.world.offers.len().min(3)).find(|i| inside(self.offer_btn(*i), mx, my)) {
            if self.net.mode == Mode::Client {
                self.net.send_cmd(&Cmd::Order(i as u8));
            } else if let Err(e) = self.world.take_order(i) {
                self.error(e);
                return;
            }
            self.audio.play(700.0, 0.12, 0.1, Wave::Tri, 0.0);
        }
        if let Some(g) = (0..4).find(|g| inside(self.skin_btn(*g), mx, my)) {
            let skin = if self.world.skin == g as u8 + 1 { 0 } else { g as u8 + 1 };
            if self.net.mode == Mode::Client {
                self.net.send_cmd(&Cmd::Skin(skin));
            } else if !self.world.set_skin(skin) {
                self.error("СКИН ОТКРОЕТСЯ ПРИ РЕПУТАЦИИ 3");
            }
        }
    }

    fn click_great(&mut self, mx: i32, my: i32) {
        if !inside(self.wide_panel(), mx, my) {
            self.close_modals();
            return;
        }
        if !inside(self.prestige_btn(), mx, my) {
            self.prestige_arm = false;
            return;
        }
        if self.world.stones_now == 0 {
            self.error("НУЖЕН ХОТЯ БЫ ОДИН ФИЛОСОФСКИЙ КАМЕНЬ, СДАННЫЙ В ЛАВКУ");
        } else if !self.prestige_arm {
            self.prestige_arm = true;
        } else {
            self.prestige_arm = false;
            self.do_prestige();
        }
    }

    fn do_prestige(&mut self) {
        if self.net.mode == Mode::Client {
            self.net.send_cmd(&Cmd::Prestige);
            return;
        }
        if let Ok(n) = self.world.prestige() {
            self.tool = None;
            self.net.broadcast_world(&self.world.save());
            self.banner = Some(("НОВЫЙ ВИТОК!".to_string(), format!("КАМНЕЙ В ЗАЧЁТ: +{}. ФАБРИКА НАЧИНАЕТСЯ ЗАНОВО", n), 6.0));
            self.close_modals();
        }
    }

    fn draw_guild(&self, c: &mut Image) {
        let w = &self.world;
        c.fill_rect(0, 0, c.w, c.h, 0xA010_0E16);
        let p = self.wide_panel();
        panel(c, p, GOLD);
        c.text(p.0 + 20, p.1 + 16, "ГИЛЬДИИ ЧЕТЫРЁХ СТИХИЙ", 2, GOLD);
        c.text(p.0 + p.2 - 20 - text_w("ЩЕЛЧОК МИМО ИЛИ G - ЗАКРЫТЬ", 2), p.1 + 16, "ЩЕЛЧОК МИМО ИЛИ G - ЗАКРЫТЬ", 2, DIM);
        for g in 0..4 {
            let r = self.guild_card(g);
            let col = ELEM_COLORS[g];
            c.fill_rect(r.0, r.1, r.2, r.3, hex(0x3A3346));
            c.rect_outline(r.0, r.1, r.2, r.3, 2, col);
            c.text(r.0 + 12, r.1 + 10, GUILD_NAMES[g], 2, col);
            c.text(r.0 + 12, r.1 + 34, &format!("РЕПУТАЦИЯ {}", w.rep[g]), 2, TEXT);
            c.fill_rect(r.0 + 12, r.1 + 56, r.2 - 24, 8, hex(0x14121A));
            c.fill_rect(r.0 + 12, r.1 + 56, (r.2 - 24) * w.rep[g].min(REP_BUILD) as i32 / REP_BUILD as i32, 8, col);
            for (i, (t, l)) in [(REP_SKIN, "СКИН ЛЕНТ"), (REP_VEIN, "ЖИЛА ЧИСТОТЫ II"), (REP_BUILD, "ГИЛЬД. ЛАВКА")].iter().enumerate() {
                let got = w.rep[g] >= *t;
                c.text(r.0 + 12, r.1 + 74 + i as i32 * 22, &format!("{:>2}: {}", t, l), 2, if got { GREEN } else { DIM });
            }
            let b = self.skin_btn(g);
            let (label, bc) = if w.rep[g] < REP_SKIN {
                ("СКИН ЗАКРЫТ", DIM)
            } else if w.skin == g as u8 + 1 {
                ("СКИН НАДЕТ - СНЯТЬ", GOLD)
            } else {
                ("НАДЕТЬ СКИН ЛЕНТ", TEXT)
            };
            c.fill_rect(b.0, b.1, b.2, b.3, if inside(b, self.mx, self.my) { hex(0x5A4E6C) } else { hex(0x2B2433) });
            c.rect_outline(b.0, b.1, b.2, b.3, 2, bc);
            c.text(b.0 + (b.2 - text_w(label, 2)) / 2, b.1 + 8, label, 2, bc);
        }
        c.text(p.0 + 20, p.1 + 260, "СРОЧНЫЕ ЗАКАЗЫ: ВЫБЕРИ ОДИН ИЗ ТРЁХ", 2, GOLD);
        for (i, o) in w.offers.iter().take(3).enumerate() {
            let r = self.offer_card(i);
            let col = ELEM_COLORS[o.guild as usize % 4];
            c.fill_rect(r.0, r.1, r.2, r.3, hex(0x3A3346));
            c.rect_outline(r.0, r.1, r.2, r.3, 2, PANEL_HI);
            c.text(r.0 + 12, r.1 + 10, GUILD_NAMES[o.guild as usize % 4], 2, col);
            c.blit(&self.assets.items[o.item.id()], r.0 + 12, r.1 + 36, 3);
            c.text(r.0 + 50, r.1 + 44, &o.item.name(), if text_w(&o.item.name(), 2) > r.2 - 60 { 1 } else { 2 }, TEXT);
            c.text(r.0 + 12, r.1 + 76, &format!("СДАТЬ {} ШТ ЗА {}:{:02}", o.need, o.secs as i32 / 60, o.secs as i32 % 60), 2, TEXT);
            c.text(r.0 + 12, r.1 + 98, &format!("НАГРАДА ${}", fmt_money(o.reward * w.reward_mult())), 2, GOLD);
            c.text(r.0 + 12, r.1 + 120, &format!("РЕПУТАЦИЯ +{}", o.rep), 2, col);
            let b = self.offer_btn(i);
            let busy = w.order.is_some();
            c.fill_rect(b.0, b.1, b.2, b.3, if inside(b, self.mx, self.my) && !busy { hex(0x5A4E6C) } else { hex(0x2B2433) });
            c.rect_outline(b.0, b.1, b.2, b.3, 2, if busy { PANEL_HI } else { GOLD });
            let l = if busy { "СНАЧАЛА ЗАКОНЧИ ТЕКУЩИЙ" } else { "ВЗЯТЬ ЗАКАЗ" };
            c.text(b.0 + (b.2 - text_w(l, 2)) / 2, b.1 + 9, l, 2, if busy { DIM } else { GOLD });
        }
        let y = p.1 + 500;
        match w.order {
            Some((o, done, deadline)) => {
                let left = (deadline - w.time).max(0.0) as i32;
                c.text(p.0 + 20, y, &format!("ТЕКУЩИЙ ЗАКАЗ ({}):", GUILD_NAMES[o.guild as usize % 4]), 2, GOLD);
                c.blit(&self.assets.items[o.item.id()], p.0 + 20, y + 26, 3);
                let s = format!("{}  {}/{}  ОСТАЛОСЬ {}:{:02}", o.item.name(), done, o.need, left / 60, left % 60);
                c.text(p.0 + 60, y + 34, &s, 2, TEXT);
                c.fill_rect(p.0 + 20, y + 66, p.2 - 40, 14, hex(0x14121A));
                c.fill_rect(p.0 + 22, y + 68, (p.2 - 44) * done as i32 / o.need.max(1) as i32, 10, hex(0x5DAA4A));
            }
            None => {
                c.text(p.0 + 20, y, "ЗАКАЗА НЕТ. ВОЗЬМИ ЛЮБОЙ ИЗ ТРЁХ: ТОВАР НАДО СДАТЬ В ЛАВКУ ДО СРОКА.", 2, DIM);
                c.text(p.0 + 20, y + 24, "НЕ УСПЕЛ - НАГРАДЫ НЕТ. ПРЕДЛОЖЕНИЯ МЕНЯЮТСЯ РАЗ В 10 МИНУТ.", 2, DIM);
            }
        }
    }

    /// Великое Делание: витраж-роза из 1170 стёкол. Кольца от центра - формы, сектора - смеси
    /// и чистоты. Стекло загорается цветом смеси, когда позиция сдана в лавку.
    fn draw_great(&self, c: &mut Image) {
        let w = &self.world;
        c.fill_rect(0, 0, c.w, c.h, 0xA010_0E16);
        let p = self.wide_panel();
        panel(c, p, GOLD);
        let done = w.great.iter().filter(|g| **g).count();
        c.text(p.0 + 20, p.1 + 16, &format!("ВЕЛИКОЕ ДЕЛАНИЕ: {} ИЗ {}", done, N_ITEM_IDS), 2, GOLD);
        let rad = ((p.3 - 70) / 2).min(290) as f32;
        let (cx, cy) = (p.0 as f32 + 30.0 + rad, p.1 as f32 + 50.0 + rad);
        let core = rad * 0.11;
        let ring_w = (rad - core) / N_FORMS as f32;
        let mut hover: Option<Item> = None;
        for py in -(rad as i32)..=rad as i32 {
            for px in -(rad as i32)..=rad as i32 {
                let r = ((px * px + py * py) as f32).sqrt();
                if r > rad {
                    continue;
                }
                let (sx, sy) = (cx as i32 + px, cy as i32 + py);
                if r < core {
                    c.put(sx, sy, if w.great_done { GOLD } else { hex(0x3A3346) });
                    continue;
                }
                let ring = ((r - core) / ring_w) as usize;
                let a = ((py as f32).atan2(px as f32) / std::f32::consts::TAU + 0.75).fract() * 45.0;
                let (seg, fr) = (a as usize % 45, a.fract());
                let edge = ((r - core) / ring_w).fract() < 0.12 || fr < 0.07 * (N_FORMS as f32 / (ring as f32 + 2.0)).min(3.0);
                if edge || r > rad - 1.5 {
                    c.put(sx, sy, hex(0x14121A));
                    continue;
                }
                let it = Item::new((seg / 3) as u8 + 1, (seg % 3) as u8 + 1, ring.min(N_FORMS - 1) as u8);
                if (sx, sy) == (self.mx, self.my) {
                    hover = Some(it);
                }
                let col = if w.great[it.id()] {
                    let cols: Vec<Color> = (0..4).filter(|e| it.mask >> e & 1 == 1).map(|e| ELEM_COLORS[e]).collect();
                    let band = cols[((fr * cols.len() as f32) as usize).min(cols.len() - 1)];
                    shade(band, [0.75, 1.0, 1.3][it.purity as usize - 1])
                } else {
                    hex(0x2A2533)
                };
                c.put(sx, sy, col);
            }
        }
        let tx = p.0 + 60 + 2 * rad as i32;
        let pct = done * 100 / N_ITEM_IDS;
        let mut lines: Vec<(String, Color)> = vec![
            ("СДАЙ В ЛАВКУ ПО ОДНОМУ ПРЕДМЕТУ".to_string(), TEXT),
            ("КАЖДОЙ ПОЗИЦИИ КАТАЛОГА.".to_string(), TEXT),
            (format!("ГОТОВО {}%. НАГРАДА ${}", pct, fmt_money(GREAT_REWARD)), if w.great_done { GREEN } else { GOLD }),
            (String::new(), TEXT),
            ("КОЛЬЦА ОТ ЦЕНТРА - ФОРМЫ,".to_string(), DIM),
            ("СЕКТОРА - 15 СМЕСЕЙ X 3 ЧИСТОТЫ.".to_string(), DIM),
            (match hover {
                Some(it) => format!("{}: {}", it.name(), if w.great[it.id()] { "СДАНО" } else { "НЕ СДАНО" }),
                None => "НАВЕДИ НА СТЕКЛО, ЧТОБЫ УЗНАТЬ ПОЗИЦИЮ".to_string(),
            }, GOLD),
            (String::new(), TEXT),
            (format!("НОВЫЙ ВИТОК: СЕЙЧАС ВИТОК {}", w.cycle + 1), GOLD),
            (format!("КАМНИ: {} (+{}% ЦЕНЫ, +{}% СКОРОСТИ)", w.stones, w.stones.min(STONES_CAP) * 5, w.stones.min(STONES_CAP) * 2), TEXT),
            (format!("СДАНО КАМНЕЙ В ЭТОМ ВИТКЕ: {}", w.stones_now), TEXT),
            ("ВИТОК СНОСИТ ФАБРИКУ, ДЕНЬГИ, КОНТРАКТЫ".to_string(), DIM),
            ("И УЛУЧШЕНИЯ. ОСТАЮТСЯ КАТАЛОГ, ВИТРАЖ,".to_string(), DIM),
            ("ГИЛЬДИИ И КАРТА. ЗА КАЖДЫЙ КАМЕНЬ:".to_string(), DIM),
            ("+5% ЦЕНЫ, +2% СКОРОСТИ, +$500.".to_string(), DIM),
        ];
        if lines[6].0.len() > 80 {
            lines[6].0.truncate(80);
        }
        for (i, (l, col)) in lines.iter().enumerate() {
            let sc = if text_w(l, 2) > p.0 + p.2 - tx - 16 { 1 } else { 2 };
            c.text(tx, p.1 + 60 + i as i32 * 24, l, sc, *col);
        }
        let b = self.prestige_btn();
        let ok = w.stones_now > 0;
        let label = if !ok {
            "НУЖЕН КАМЕНЬ, СДАННЫЙ В ЛАВКУ".to_string()
        } else if self.prestige_arm {
            "ТОЧНО? ЩЁЛКНИ ЕЩЁ РАЗ".to_string()
        } else {
            format!("НАЧАТЬ НОВЫЙ ВИТОК (+{} КАМНЕЙ)", w.stones_now)
        };
        c.fill_rect(b.0, b.1, b.2, b.3, if self.prestige_arm { hex(0x8A2A2A) } else { hex(0x3A3346) });
        c.rect_outline(b.0, b.1, b.2, b.3, 2, if ok { GOLD } else { PANEL_HI });
        c.text(b.0 + (b.2 - text_w(&label, 2)) / 2, b.1 + 13, &label, 2, if ok { GOLD } else { DIM });
    }

    fn ts(&self) -> i32 {
        TILE * self.zoom
    }

    /// Экранные координаты тайла (0, 0) мира. Подземелье лежит в мире ниже поверхности, поэтому
    /// при взгляде на него начало отсчёта уезжает вверх на высоту слоя: камера остаётся над той
    /// же клеткой, а всё, что рисуется по координатам мира, попадает на свой слой само.
    fn origin(&self) -> (i32, i32) {
        let ts = self.ts() as f32;
        let cy = self.cam_y + (self.layer * DUNGEON_Y) as f32;
        (self.w / 2 - (self.cam_x * ts).round() as i32, self.h / 2 - (cy * ts).round() as i32)
    }

    /// Z: спуститься в подземелье или подняться. Камера остаётся над той же клеткой.
    pub fn switch_layer(&mut self) {
        self.layer = 1 - self.layer;
        self.sel = None;
        self.sel_start = None;
        self.drag_tile = None;
        self.rdrag_tile = None;
        self.show_cfg = None;
        self.say(if self.layer == 1 { "ПОДЗЕМЕЛЬЕ: ЖИЛЫ ЧИСТОТЫ II. Z - НАВЕРХ" } else { "ПОВЕРХНОСТЬ" });
        self.audio.play(if self.layer == 1 { 330.0 } else { 520.0 }, 0.1, 0.1, Wave::Tri, 0.0);
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
        if !self.world.unlocked(kind) {
            let when = if kind.role() == Role::GuildPost {
                "ПРИ РЕПУТАЦИИ 10 В ЛЮБОЙ ГИЛЬДИИ".to_string()
            } else {
                format!("ПОСЛЕ КОНТРАКТА {}", kind.unlock_level())
            };
            self.error(&format!("{}: ОТКРОЕТСЯ {}", kind.name(), when));
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
                        v.push(ClipItem { dx: x - x0, dy: y - y0, kind: b.kind, dir: b.dir, level: b.level, ports: b.ports, cfg: b.cfg });
                    }
                }
            }
        }
        v
    }

    /// Детали логики внутри рамки.
    fn selected_nodes(&self) -> Vec<ClipNode> {
        let mut v = Vec::new();
        if let Some((x0, y0, x1, y1)) = self.sel {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if let Some(n) = self.world.node(x, y) {
                        v.push(ClipNode { dx: x - x0, dy: y - y0, kind: n.kind, dir: n.dir, param: n.param });
                    }
                }
            }
        }
        v
    }

    /// Ctrl+C копирует выделенное, Ctrl+X ещё и сносит его (перенос). Сразу включается вставка.
    /// В буфер попадают оба слоя: постройки и детали логики над ними.
    pub fn copy_selection(&mut self, cut: bool) {
        let (items, nodes) = (self.selected(), self.selected_nodes());
        let (x0, y0, x1, y1) = match self.sel {
            Some(r) if !items.is_empty() || !nodes.is_empty() => r,
            _ => {
                self.error("СНАЧАЛА ВЫДЕЛИ ПОСТРОЙКИ: SHIFT + ЛКМ");
                return;
            }
        };
        if cut {
            for it in &items {
                self.remove_building_at(x0 + it.dx, y0 + it.dy);
            }
            for n in &nodes {
                self.remove_node_at(x0 + n.dx, y0 + n.dy);
            }
        }
        let verb = if cut { "ВЫРЕЗАНО" } else { "СКОПИРОВАНО" };
        self.say(&format!("{}: {}. ЛКМ - ВСТАВИТЬ, R - ПОВЕРНУТЬ", verb, items.len() + nodes.len()));
        self.clip = items;
        self.clip_nodes = nodes;
        self.clip_size = (x1 - x0 + 1, y1 - y0 + 1);
        self.pasting = true;
        self.tool = None;
        self.sel = None;
        self.audio.play(700.0, 0.08, 0.1, Wave::Tri, 0.0);
    }

    /// DEL сносит выделенные постройки; при открытом слое проводов - детали логики, если они
    /// в рамке есть (как и ПКМ, который на этом слое снимает сначала деталь).
    fn delete_selection(&mut self) {
        let (items, nodes) = (self.selected(), self.selected_nodes());
        let (x0, y0) = match self.sel {
            Some(r) if !items.is_empty() || !nodes.is_empty() => (r.0, r.1),
            _ => return,
        };
        if self.wires_on() && !nodes.is_empty() {
            for n in &nodes {
                self.remove_node_at(x0 + n.dx, y0 + n.dy);
            }
            self.say(&format!("СНЯТО ДЕТАЛЕЙ ЛОГИКИ: {}", nodes.len()));
        } else if !items.is_empty() {
            for it in &items {
                self.remove_building_at(x0 + it.dx, y0 + it.dy);
            }
            self.say(&format!("СНЕСЕНО ПОСТРОЕК: {}", items.len()));
        } else {
            self.error("В РАМКЕ ТОЛЬКО ДЕТАЛИ ЛОГИКИ: ОТКРОЙ СЛОЙ ПРОВОДОВ (L)");
            return;
        }
        self.audio.play(240.0, 0.12, 0.12, Wave::Tri, 0.0);
        self.sel = None;
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
        for n in &mut self.clip_nodes {
            let (dx, dy) = (n.dx, n.dy);
            n.dx = h - 1 - dy;
            n.dy = dx;
            n.dir = n.dir.cw();
        }
        self.clip_size = (h, w);
    }

    fn clip_cost(&self) -> f64 {
        bp_cost(&self.world, &self.clip, &self.clip_nodes)
    }

    /// Ставит буфер обмена левым верхним углом в (ox, oy). Возвращает, сколько построек
    /// и деталей логики встало.
    pub fn paste_at(&mut self, ox: i32, oy: i32) -> usize {
        let items = self.clip.clone();
        let nodes = self.clip_nodes.clone();
        let mut ok = 0;
        for it in &items {
            let (x, y) = (ox + it.dx, oy + it.dy);
            if self.world.can_place(it.kind, x, y).is_err() || !self.world.unlocked(it.kind) {
                continue;
            }
            if self.net.mode == Mode::Client {
                // Клиент просит хоста: постройка, затем её уровни.
                self.net.send_cmd(&Cmd::Place(it.kind, x, y, it.dir));
                for _ in 1..it.level {
                    self.net.send_cmd(&Cmd::LevelUp(x, y));
                }
                if it.ports != [PORT_AUTO; 4] {
                    self.net.send_cmd(&Cmd::SetPorts(x, y, pack_ports(it.ports)));
                }
                if it.cfg != 0 {
                    self.net.send_cmd(&Cmd::SetCfg(x, y, it.cfg));
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
            let packed = pack_ports(it.ports);
            if self.world.get(x, y).map_or(false, |b| b.ports != it.ports) && self.world.set_ports(x, y, packed) {
                self.net.broadcast_op(&Op::Ports(x, y, packed));
            }
            if self.world.get(x, y).map_or(false, |b| b.cfg != it.cfg) {
                self.apply_cfg(x, y, it.cfg);
            }
        }
        // Детали логики ложатся поверх построек вместе со своими настройками.
        for n in &nodes {
            let (x, y) = (ox + n.dx, oy + n.dy);
            let lk = match n.kind.role() {
                Role::Logic(lk) if World::inb(x, y) && self.world.unlocked(n.kind) => lk,
                _ => continue,
            };
            if self.net.mode == Mode::Client {
                self.net.send_cmd(&Cmd::Place(n.kind, x, y, n.dir));
                if lk_has_param(lk) {
                    self.net.send_cmd(&Cmd::LCfg(x, y, n.param));
                }
                ok += 1;
                continue;
            }
            match self.world.place(n.kind, x, y, n.dir) {
                Ok(true) => self.net.broadcast_op(&Op::Set(x, y, n.kind, n.dir)),
                Ok(false) => {}
                Err(_) => continue,
            }
            ok += 1;
            if lk_has_param(lk) && self.world.node(x, y).map_or(false, |m| m.param != n.param) {
                self.apply_lparam(x, y, n.param);
            }
        }
        let total = items.len() + nodes.len();
        if ok == total {
            self.say(&format!("ВСТАВЛЕНО: {}", ok));
            self.audio.play(620.0, 0.08, 0.12, Wave::Tri, 0.0);
        } else {
            self.error(&format!("ВСТАВЛЕНО {} ИЗ {}: НЕТ ДЕНЕГ ИЛИ МЕСТО НЕ ПОДХОДИТ", ok, total));
        }
        ok
    }

    // ---------- библиотека чертежей ----------

    /// Подключает файл библиотеки: читает из него чертежи и дальше пишет в него же.
    pub fn open_library(&mut self, path: &str) {
        self.blueprints = std::fs::read_to_string(path).map(|t| bp_from_text(&t)).unwrap_or_default();
        self.bp_file = Some(path.to_string());
    }

    fn write_library(&mut self) {
        if let Some(path) = &self.bp_file {
            if std::fs::write(path, bp_to_text(&self.blueprints)).is_err() {
                self.error("НЕ УДАЛОСЬ ЗАПИСАТЬ ФАЙЛ БИБЛИОТЕКИ ЧЕРТЕЖЕЙ");
            }
        }
    }

    /// Буква в поле имени: только то, что умеет рисовать шрифт, до 24 знаков.
    fn bp_type(&mut self, ch: char) {
        let up = ch.to_uppercase().next().unwrap_or(ch);
        let lead_space = up == ' ' && self.bp_name.is_empty();
        if !ch.is_control() && has_glyph(up) && !lead_space && self.bp_name.chars().count() < BP_NAME_MAX {
            self.bp_name.push(up);
        }
    }

    /// Сохраняет буфер обмена в библиотеку под набранным именем. Чертёж с тем же именем заменяется.
    pub fn bp_save(&mut self) {
        if self.clip.is_empty() && self.clip_nodes.is_empty() {
            self.error("БУФЕР ПУСТ: ВЫДЕЛИ РАМКОЙ (SHIFT + ЛКМ) И НАЖМИ CTRL+C");
            return;
        }
        let mut name = self.bp_name.trim().to_string();
        if name.is_empty() {
            let free = (1..).find(|n| !self.blueprints.iter().any(|b| b.name == format!("ЧЕРТЁЖ {}", n))).unwrap_or(1);
            name = format!("ЧЕРТЁЖ {}", free);
        }
        let bp = Blueprint { name: name.clone(), size: self.clip_size, items: self.clip.clone(), nodes: self.clip_nodes.clone() };
        let verb = match self.blueprints.iter_mut().find(|b| b.name == name) {
            Some(old) => {
                *old = bp;
                "ЗАМЕНЁН"
            }
            None => {
                self.blueprints.push(bp);
                "СОХРАНЁН"
            }
        };
        self.bp_name.clear();
        self.bp_del_arm = None;
        self.write_library();
        self.say(&format!("ЧЕРТЁЖ {}: {}", verb, name));
        self.audio.play(700.0, 0.12, 0.1, Wave::Tri, 0.0);
    }

    /// Берёт чертёж из библиотеки в буфер обмена и включает вставку.
    pub fn bp_load(&mut self, i: usize) -> bool {
        let bp = match self.blueprints.get(i) {
            Some(bp) => bp.clone(),
            None => return false,
        };
        self.close_modals();
        self.clip = bp.items;
        self.clip_nodes = bp.nodes;
        self.clip_size = bp.size;
        self.pasting = true;
        self.tool = None;
        self.sel = None;
        self.say(&format!("ЧЕРТЁЖ {}: ЛКМ - ВСТАВИТЬ, R - ПОВЕРНУТЬ", bp.name));
        self.audio.play(700.0, 0.08, 0.1, Wave::Tri, 0.0);
        true
    }

    fn bp_name_rect(&self) -> (i32, i32, i32, i32) {
        let p = self.wide_panel();
        (p.0 + 20, p.1 + 54, 520.min(p.2 - 380), 36)
    }

    fn bp_save_btn(&self) -> (i32, i32, i32, i32) {
        let r = self.bp_name_rect();
        (r.0 + r.2 + 12, r.1, 320, 36)
    }

    fn bp_rows(&self) -> usize {
        ((self.wide_panel().3 - 160) / 44).max(1) as usize
    }

    fn bp_row(&self, i: usize) -> (i32, i32, i32, i32) {
        let p = self.wide_panel();
        (p.0 + 20, p.1 + 140 + i as i32 * 44, p.2 - 40, 40)
    }

    fn bp_del_btn(&self, i: usize) -> (i32, i32, i32, i32) {
        let r = self.bp_row(i);
        (r.0 + r.2 - 136, r.1 + 5, 130, 30)
    }

    fn click_bp(&mut self, mx: i32, my: i32) {
        if !inside(self.wide_panel(), mx, my) {
            self.close_modals();
            return;
        }
        if inside(self.bp_save_btn(), mx, my) {
            self.bp_save();
            return;
        }
        let hit = (0..self.bp_rows()).find(|i| inside(self.bp_row(*i), mx, my)).map(|i| (i, i + self.bp_scroll));
        match hit {
            Some((row, i)) if i < self.blueprints.len() => {
                if !inside(self.bp_del_btn(row), mx, my) {
                    self.bp_load(i);
                } else if self.bp_del_arm == Some(i) {
                    // Второй щелчок по той же кнопке подтверждает удаление.
                    let gone = self.blueprints.remove(i);
                    self.bp_del_arm = None;
                    self.bp_scroll = self.bp_scroll.min(self.blueprints.len().saturating_sub(1));
                    self.write_library();
                    self.say(&format!("ЧЕРТЁЖ УДАЛЁН: {}", gone.name));
                    self.audio.play(240.0, 0.12, 0.12, Wave::Tri, 0.0);
                } else {
                    self.bp_del_arm = Some(i);
                }
            }
            _ => self.bp_del_arm = None,
        }
    }

    fn draw_bp(&self, c: &mut Image) {
        c.fill_rect(0, 0, c.w, c.h, 0xA010_0E16);
        let p = self.wide_panel();
        panel(c, p, GOLD);
        c.text(p.0 + 20, p.1 + 16, &format!("БИБЛИОТЕКА ЧЕРТЕЖЕЙ: {}", self.blueprints.len()), 2, GOLD);
        c.text(p.0 + p.2 - 20 - text_w("ЩЕЛЧОК МИМО ИЛИ ESC - ЗАКРЫТЬ", 2), p.1 + 16, "ЩЕЛЧОК МИМО ИЛИ ESC - ЗАКРЫТЬ", 2, DIM);
        let r = self.bp_name_rect();
        c.fill_rect(r.0, r.1, r.2, r.3, hex(0x14121A));
        c.rect_outline(r.0, r.1, r.2, r.3, 2, GOLD);
        if self.bp_name.is_empty() {
            c.text(r.0 + 10, r.1 + 11, "НАБЕРИ ИМЯ ЧЕРТЕЖА", 2, DIM);
        }
        let end = c.text(r.0 + 10, r.1 + 11, &self.bp_name, 2, TEXT);
        if (self.time * 2.0) as i32 % 2 == 0 {
            c.fill_rect(end + 1, r.1 + 9, 3, 18, GOLD);
        }
        let b = self.bp_save_btn();
        let empty = self.clip.is_empty() && self.clip_nodes.is_empty();
        let bc = if empty { DIM } else { GOLD };
        c.fill_rect(b.0, b.1, b.2, b.3, if inside(b, self.mx, self.my) && !empty { hex(0x5A4E6C) } else { hex(0x2B2433) });
        c.rect_outline(b.0, b.1, b.2, b.3, 2, bc);
        let l = "СОХРАНИТЬ БУФЕР (ENTER)";
        c.text(b.0 + (b.2 - text_w(l, 2)) / 2, b.1 + 11, l, 2, bc);
        let info = if empty {
            "БУФЕР ПУСТ: ВЫДЕЛИ РАМКОЙ (SHIFT + ЛКМ), НАЖМИ CTRL+C И ВЕРНИСЬ СЮДА (B)".to_string()
        } else {
            let (w, h) = self.clip_size;
            format!("В БУФЕРЕ: {}×{}, ПОСТРОЕК {}, ДЕТАЛЕЙ ЛОГИКИ {}, ЦЕНА ${}", w, h, self.clip.len(), self.clip_nodes.len(), fmt_money(self.clip_cost()))
        };
        c.text(p.0 + 20, p.1 + 104, &info, 2, if empty { DIM } else { TEXT });
        if self.blueprints.is_empty() {
            c.text(p.0 + 20, p.1 + 150, "ЧЕРТЕЖЕЙ ПОКА НЕТ. ОНИ ХРАНЯТСЯ В ОТДЕЛЬНОМ ФАЙЛЕ И ГОДЯТСЯ ДЛЯ ЛЮБОЙ ИГРЫ.", 2, DIM);
        }
        for row in 0..self.bp_rows() {
            let i = row + self.bp_scroll;
            let bp = match self.blueprints.get(i) {
                Some(bp) => bp,
                None => break,
            };
            let r = self.bp_row(row);
            let d = self.bp_del_btn(row);
            let over_del = inside(d, self.mx, self.my);
            let hot = inside(r, self.mx, self.my) && !over_del;
            c.fill_rect(r.0, r.1, r.2, r.3, if hot { hex(0x5A4E6C) } else { hex(0x3A3346) });
            c.rect_outline(r.0, r.1, r.2, r.3, 2, if hot { GOLD } else { PANEL_HI });
            c.text(r.0 + 12, r.1 + 13, &bp.name, 2, TEXT);
            let locked = bp.items.iter().map(|it| it.kind).chain(bp.nodes.iter().map(|n| n.kind)).filter(|k| !self.world.unlocked(*k)).count();
            let cost = bp_cost(&self.world, &bp.items, &bp.nodes);
            let mut s = format!("{}×{}  ПОСТРОЕК {}  ЛОГИКИ {}  ${}", bp.size.0, bp.size.1, bp.items.len(), bp.nodes.len(), fmt_money(cost));
            if locked > 0 {
                s.push_str(&format!("  ЗАКРЫТО {}", locked));
            }
            c.text(r.0 + 330, r.1 + 13, &s, 2, if locked > 0 { DIM } else { GOLD });
            let arm = self.bp_del_arm == Some(i);
            c.fill_rect(d.0, d.1, d.2, d.3, if over_del { hex(0x5A3030) } else { hex(0x2B2433) });
            c.rect_outline(d.0, d.1, d.2, d.3, 2, if arm { RED } else { PANEL_HI });
            let l = if arm { "ТОЧНО?" } else { "УДАЛИТЬ" };
            c.text(d.0 + (d.2 - text_w(l, 2)) / 2, d.1 + 8, l, 2, if arm { RED } else { DIM });
        }
        if self.blueprints.len() > self.bp_rows() {
            let s = format!("КОЛЕСО - ЛИСТАТЬ ({}-{} ИЗ {})", self.bp_scroll + 1, (self.bp_scroll + self.bp_rows()).min(self.blueprints.len()), self.blueprints.len());
            c.text(p.0 + p.2 - 20 - text_w(&s, 2), p.1 + 104, &s, 2, DIM);
        }
    }

    /// Сторона тайла, ближайшая к точке экрана: (тайл, абсолютное направление).
    fn side_at(&self, mx: i32, my: i32) -> ((i32, i32), Dir) {
        let (tx, ty) = self.tile_at(mx, my);
        let (ox, oy) = self.origin();
        let ts = self.ts();
        let dx = mx - (ox + tx * ts) - ts / 2;
        let dy = my - (oy + ty * ts) - ts / 2;
        let d = if dx.abs() > dy.abs() {
            if dx > 0 { Dir::E } else { Dir::W }
        } else if dy > 0 {
            Dir::S
        } else {
            Dir::N
        };
        ((tx, ty), d)
    }

    fn apply_ports(&mut self, x: i32, y: i32, packed: u8) {
        if self.net.mode == Mode::Client {
            self.net.send_cmd(&Cmd::SetPorts(x, y, packed));
        } else if self.world.set_ports(x, y, packed) {
            self.net.broadcast_op(&Op::Ports(x, y, packed));
        }
    }

    fn apply_cfg(&mut self, x: i32, y: i32, cfg: u64) {
        if self.net.mode == Mode::Client {
            self.net.send_cmd(&Cmd::SetCfg(x, y, cfg));
        } else if self.world.set_cfg(x, y, cfg) {
            // Рассылается то, что мир принял на самом деле (после проверки значений).
            let v = self.world.get(x, y).map_or(0, |b| b.cfg);
            self.net.broadcast_op(&Op::Cfg(x, y, v));
        }
    }

    /// Открывает окно настройки постройки на тайле, если ей есть что настраивать.
    fn open_cfg(&mut self, x: i32, y: i32) -> bool {
        // При открытом слое проводов настраивается деталь логики, иначе - постройка под ней.
        let node = self.wires_on() && self.world.node(x, y).map_or(false, |n| lk_has_param(n.lk()));
        if !node && !self.world.get(x, y).map_or(false, |b| has_panel(b.kind)) {
            return false;
        }
        self.close_modals();
        self.cfg_node = node;
        self.show_cfg = Some((x, y));
        self.cfg_tab = 0;
        self.audio.play(520.0, 0.05, 0.08, Wave::Tri, 0.0);
        true
    }

    /// Щелчок по краю машины: авто -> вход -> выход -> закрыто -> авто.
    /// Щелчок по середине постройки с настройками открывает её окно.
    fn click_port(&mut self, mx: i32, my: i32) {
        let ((x, y), d) = self.side_at(mx, my);
        let (facing, old_cfg) = match self.world.get(x, y) {
            Some(b) => (b.dir, b.cfg),
            None => return,
        };
        let (ox, oy) = self.origin();
        let ts = self.ts();
        let (cx, cy) = (mx - (ox + x * ts) - ts / 2, my - (oy + y * ts) - ts / 2);
        if cx.abs().max(cy.abs()) <= ts / 5 && self.open_cfg(x, y) {
            return;
        }
        let rel = rel_side(facing, d);
        if let Some((packed, cfg)) = self.world.next_side(x, y, rel) {
            self.apply_ports(x, y, packed);
            if cfg != old_cfg {
                self.apply_cfg(x, y, cfg);
            }
            let mode = if cfg & 7 == rel as u64 + 1 && cfg != old_cfg {
                "ОТКАЗ: СЮДА УХОДИТ ТО, ЧТО МАШИНА НЕ БЕРЁТ"
            } else {
                ["АВТО", "ВХОД", "ВЫХОД", "ЗАКРЫТО"][unpack_ports(packed)[rel] as usize]
            };
            self.say(&format!("СТОРОНА: {}", mode));
            self.audio.play(560.0, 0.05, 0.1, Wave::Tri, 0.0);
        }
    }

    // ---------- окно настройки постройки ----------

    fn node_target(&self) -> Option<(i32, i32, Node)> {
        let (x, y) = self.show_cfg.filter(|_| self.cfg_node)?;
        self.world.node(x, y).filter(|n| lk_has_param(n.lk())).map(|n| (x, y, *n))
    }

    fn apply_lparam(&mut self, x: i32, y: i32, v: u64) {
        if self.net.mode == Mode::Client {
            self.net.send_cmd(&Cmd::LCfg(x, y, v));
        } else if self.world.set_lparam(x, y, v) {
            let v = self.world.node(x, y).map_or(0, |n| n.param);
            self.net.broadcast_op(&Op::LCfg(x, y, v));
        }
    }

    fn node_rect(&self) -> (i32, i32, i32, i32) {
        let h = if self.node_target().map_or(false, |t| t.2.lk() == Lk::SenItem) { 316 } else { 200 };
        ((self.w - 740) / 2, (self.h - h) / 2, 740, h)
    }

    fn node_buttons(&self) -> Vec<Btn> {
        let mut v = Vec::new();
        let (nd, p) = match self.node_target() {
            Some(t) => (t.2, self.node_rect()),
            None => return v,
        };
        match nd.lk() {
            Lk::SenItem => Self::cond_buttons(&mut v, p.0 + 20, p.1 + 78, Cond::from_bits(nd.param)),
            Lk::Act => {
                for (i, l) in ["СИГНАЛ ОСТАНАВЛИВАЕТ", "СИГНАЛ ВКЛЮЧАЕТ"].iter().enumerate() {
                    let r = (p.0 + 20 + i as i32 * 350, p.1 + 84, 340, 36);
                    v.push(Btn { r, label: l.to_string(), icon: None, act: Act::Mode(i as u64), on: nd.param == i as u64 });
                }
            }
            _ => {
                for (i, d) in [-100i64, -10, -1, 1, 10, 100].iter().enumerate() {
                    let r = (p.0 + 60 + i as i32 * 78 + if i >= 3 { 150 } else { 0 }, p.1 + 84, 70, 36);
                    let label = if *d > 0 { format!("+{}", d) } else { format!("{}", d) };
                    v.push(Btn { r, label, icon: None, act: Act::Num(*d), on: false });
                }
            }
        }
        v
    }

    fn click_node(&mut self, mx: i32, my: i32) {
        let (x, y, nd) = match self.node_target() {
            Some(t) if inside(self.node_rect(), mx, my) => t,
            _ => {
                self.show_cfg = None;
                return;
            }
        };
        let cond = Cond::from_bits(nd.param);
        let new = match self.node_buttons().into_iter().find(|b| inside(b.r, mx, my)).map(|b| b.act) {
            Some(Act::Mask(m)) => Cond { mask: m, ..cond }.bits(),
            Some(Act::Purity(p)) => Cond { purity: p, ..cond }.bits(),
            Some(Act::Form(f)) => Cond { form: f, ..cond }.bits(),
            Some(Act::Boxed(b)) => Cond { boxed: b, ..cond }.bits(),
            Some(Act::Num(d)) => (nd.param as i64 + d).max(1) as u64,
            Some(Act::Mode(m)) => m,
            _ => return,
        };
        self.apply_lparam(x, y, new);
        self.audio.play(560.0, 0.05, 0.1, Wave::Tri, 0.0);
    }

    fn draw_node_cfg(&self, c: &mut Image) {
        let (x, y, nd) = match self.node_target() {
            Some(t) => t,
            None => return,
        };
        c.fill_rect(0, 0, c.w, c.h, 0x8010_0E16);
        let p = self.node_rect();
        panel(c, p, GOLD);
        c.text(p.0 + 20, p.1 + 16, &format!("{} ({}, {})", nd.kind.name(), x, y), 2, GOLD);
        let mut tip = "ЩЕЛЧОК МИМО ОКНА ИЛИ ESC - ЗАКРЫТЬ".to_string();
        c.text(p.0 + 20, p.1 + 44, &node_text(&nd), 2, TEXT);
        match nd.lk() {
            Lk::SenItem => {
                for (i, l) in ["СМЕСЬ", "ЧИСТОТА", "ФОРМА", "", "ЯЩИК"].iter().enumerate() {
                    c.text(p.0 + 20, p.1 + 86 + i as i32 * 36, l, 2, DIM);
                }
            }
            Lk::Act => {
                c.text(p.0 + 20, p.1 + 136, "НА ЛЕНТЕ - ВОРОТА, НА МАШИНЕ - ОСТАНОВ. ПЕРЕКЛЮЧАТЕЛЮ СИГНАЛ ВЫБИРАЕТ ВТОРОЙ ВЫХОД.", 1, DIM);
            }
            lk => {
                let val = if lk == Lk::Timer { format!("{:.1} С", nd.param as f64 / 10.0) } else { format!("{}", nd.param) };
                c.text_center(p.0 + 370, p.1 + 88, &val, 3, GOLD);
            }
        }
        for bt in self.node_buttons() {
            let hov = inside(bt.r, self.mx, self.my);
            let r = bt.r;
            c.fill_rect(r.0, r.1, r.2, r.3, if hov { hex(0x5A4E6C) } else { hex(0x3A3346) });
            c.rect_outline(r.0, r.1, r.2, r.3, 2, if bt.on { GOLD } else { PANEL_HI });
            match bt.icon {
                Some(it) => {
                    self.draw_item(c, it, r.0 + 5, r.1 + 5, 2);
                    if hov {
                        tip = bt.label.clone();
                    }
                }
                None => {
                    let col = if bt.on { GOLD } else { TEXT };
                    c.text(r.0 + (r.2 - text_w(&bt.label, 2)) / 2, r.1 + (r.3 - 14) / 2, &bt.label, 2, col);
                }
            }
        }
        c.text(p.0 + 20, p.1 + p.3 - 28, &tip, 2, DIM);
    }

    fn cfg_target(&self) -> Option<(i32, i32, &Building)> {
        let (x, y) = self.show_cfg.filter(|_| !self.cfg_node)?;
        self.world.get(x, y).filter(|b| has_panel(b.kind)).map(|b| (x, y, b))
    }

    fn cfg_rect(&self) -> (i32, i32, i32, i32) {
        let h = match self.cfg_target().map(|t| t.2.kind.role()) {
            Some(Role::Ratio) => 190,
            Some(Role::TeleIn | Role::TeleOut) => 286,
            Some(Role::Sorter) => 356,
            _ => 316,
        };
        ((self.w - 740) / 2, (self.h - h) / 2, 740, h)
    }

    /// Кнопки выбора условия: смесь, чистота, форма, ящик. (x0, y0) - левый верхний угол блока.
    fn cond_buttons(v: &mut Vec<Btn>, x0: i32, y0: i32, c: Cond) {
        let any = |v: &mut Vec<Btn>, y: i32, act: Act, on: bool| {
            v.push(Btn { r: (x0 + 110, y, 84, 30), label: "ЛЮБАЯ".to_string(), icon: None, act, on });
        };
        any(v, y0, Act::Mask(0), c.mask == 0);
        for m in 1..=15u8 {
            let r = (x0 + 202 + (m as i32 - 1) * 32, y0, 30, 30);
            v.push(Btn { r, label: mix_name(m).to_string(), icon: Some(Item::new(m, 1, 0)), act: Act::Mask(m), on: c.mask == m });
        }
        any(v, y0 + 36, Act::Purity(0), c.purity == 0);
        for (i, l) in ["I", "II", "III"].iter().enumerate() {
            let r = (x0 + 202 + i as i32 * 64, y0 + 36, 60, 30);
            let p = i as u8 + 1;
            v.push(Btn { r, label: l.to_string(), icon: None, act: Act::Purity(p), on: c.purity == p });
        }
        any(v, y0 + 72, Act::Form(0), c.form == 0);
        let mask = if c.mask == 0 { 15 } else { c.mask };
        for f in 0..N_FORMS_ALL as u8 {
            let r = (x0 + 202 + (f as i32 % 14) * 32, y0 + 72 + (f as i32 / 14) * 32, 30, 30);
            let icon = Some(Item::new(mask, c.purity.max(1), f));
            v.push(Btn { r, label: form_name(f).to_string(), icon, act: Act::Form(f + 1), on: c.form == f + 1 });
        }
        for (i, l) in ["НЕВАЖНО", "РОССЫПЬЮ", "В ЯЩИКЕ"].iter().enumerate() {
            let r = (x0 + 110 + i as i32 * 120, y0 + 144, 114, 30);
            v.push(Btn { r, label: l.to_string(), icon: None, act: Act::Boxed(i as u8), on: c.boxed == i as u8 });
        }
    }

    fn cfg_buttons(&self) -> Vec<Btn> {
        let mut v = Vec::new();
        let (b, p) = match self.cfg_target() {
            Some(t) => (t.2, self.cfg_rect()),
            None => return v,
        };
        match b.kind.role() {
            Role::Filter => Self::cond_buttons(&mut v, p.0 + 20, p.1 + 78, Cond::from_bits(b.cfg)),
            Role::Sorter => {
                for (i, l) in ["ВПЕРЁД", "ВПРАВО", "ВЛЕВО"].iter().enumerate() {
                    let r = (p.0 + 20 + i as i32 * 234, p.1 + 44, 230, 30);
                    v.push(Btn { r, label: l.to_string(), icon: None, act: Act::Slot(i), on: self.cfg_tab == i });
                }
                Self::cond_buttons(&mut v, p.0 + 20, p.1 + 118, sorter_cond(b.cfg, self.cfg_tab));
            }
            Role::TeleIn | Role::TeleOut => {
                for i in 0..4 {
                    let bx = p.0 + 150 + i as i32 * 70;
                    for (l, d, y) in [("+", 1, 62), ("-", -1, 148)] {
                        v.push(Btn { r: (bx, p.1 + y, 56, 28), label: l.to_string(), icon: None, act: Act::Digit(i, d), on: false });
                    }
                }
                let r = (p.0 + 480, p.1 + 104, 200, 34);
                v.push(Btn { r, label: "СБРОСИТЬ КОД".to_string(), icon: None, act: Act::CodeClear, on: false });
            }
            Role::Ratio => {
                for i in 0..2 {
                    let bx = p.0 + 60 + i as i32 * 330;
                    for (j, (l, d)) in [("-", -1), ("+", 1)].iter().enumerate() {
                        let r = (bx + 150 + j as i32 * 100, p.1 + 84, 40, 40);
                        v.push(Btn { r, label: l.to_string(), icon: None, act: Act::Ratio(i, *d), on: false });
                    }
                }
            }
            _ => {}
        }
        v
    }

    fn click_cfg(&mut self, mx: i32, my: i32) {
        if !inside(self.cfg_rect(), mx, my) || self.cfg_target().is_none() {
            self.show_cfg = None;
            return;
        }
        let act = match self.cfg_buttons().into_iter().find(|b| inside(b.r, mx, my)) {
            Some(b) => b.act,
            None => return,
        };
        let (x, y, cfg, role) = match self.cfg_target() {
            Some((x, y, b)) => (x, y, b.cfg, b.kind.role()),
            None => return,
        };
        // Условие, которое правят кнопки: у фильтра оно одно, у узла - по одному на выход.
        let slot = if role == Role::Sorter { self.cfg_tab.min(2) } else { 0 };
        let cond = sorter_cond(cfg, slot);
        let put = |c: Cond| if role == Role::Sorter { sorter_set(cfg, slot, c) } else { c.bits() };
        let new = match act {
            Act::Mask(m) => put(Cond { mask: m, ..cond }),
            Act::Purity(p) => put(Cond { purity: p, ..cond }),
            Act::Form(f) => put(Cond { form: f, ..cond }),
            Act::Boxed(v) => put(Cond { boxed: v, ..cond }),
            Act::Slot(i) => {
                self.cfg_tab = i;
                self.audio.play(440.0, 0.04, 0.06, Wave::Tri, 0.0);
                return;
            }
            Act::Digit(i, d) => {
                let code = tele_code(cfg).unwrap_or(0) as i32;
                let unit = [1000, 100, 10, 1][i.min(3)];
                let digit = (code / unit % 10 + d).rem_euclid(10);
                tele_cfg((code - code / unit % 10 * unit + digit * unit) as usize)
            }
            Act::CodeClear => 0,
            Act::Num(_) | Act::Mode(_) => return,
            Act::Ratio(i, d) => {
                let (n, m) = ratio_nm(cfg);
                let adj = |v: u32| (v as i32 + d).clamp(1, 9) as u32;
                if i == 0 {
                    ratio_cfg(adj(n), m)
                } else {
                    ratio_cfg(n, adj(m))
                }
            }
        };
        self.apply_cfg(x, y, new);
        self.audio.play(560.0, 0.05, 0.1, Wave::Tri, 0.0);
    }

    fn draw_cfg(&self, c: &mut Image) {
        let (x, y, b) = match self.cfg_target() {
            Some(t) => t,
            None => return,
        };
        c.fill_rect(0, 0, c.w, c.h, 0x8010_0E16);
        let p = self.cfg_rect();
        panel(c, p, GOLD);
        c.text(p.0 + 20, p.1 + 16, &format!("{} ({}, {})", b.kind.name(), x, y), 2, GOLD);
        let mut tip = "ЩЕЛЧОК МИМО ОКНА ИЛИ ESC - ЗАКРЫТЬ".to_string();
        match b.kind.role() {
            Role::Filter => {
                let cond = Cond::from_bits(b.cfg);
                let s = if cond.is_any() {
                    "ОБРАЗЕЦ НЕ ЗАДАН: ФИЛЬТР ЗАПОМНИТ ПЕРВЫЙ ПРОЕХАВШИЙ ПРЕДМЕТ".to_string()
                } else {
                    format!("ПРЯМО ИДЁТ: {}. ОСТАЛЬНОЕ - ВБОК", cond.text())
                };
                c.text(p.0 + 20, p.1 + 44, &s, 2, TEXT);
                for (i, l) in ["СМЕСЬ", "ЧИСТОТА", "ФОРМА", "", "ЯЩИК"].iter().enumerate() {
                    c.text(p.0 + 20, p.1 + 86 + i as i32 * 36, l, 2, DIM);
                }
            }
            Role::Sorter => {
                let cond = sorter_cond(b.cfg, self.cfg_tab.min(2));
                let s = if cond.is_any() {
                    "ЭТОТ ВЫХОД БЕЗ УСЛОВИЯ: БЕРЁТ ВСЁ, ЧТО НЕ ПОДОШЛО ДРУГИМ".to_string()
                } else {
                    format!("В ЭТОТ ВЫХОД ИДЁТ: {}", cond.text())
                };
                c.text(p.0 + 20, p.1 + 88, &s, 2, TEXT);
                for (i, l) in ["СМЕСЬ", "ЧИСТОТА", "ФОРМА", "", "ЯЩИК"].iter().enumerate() {
                    c.text(p.0 + 20, p.1 + 126 + i as i32 * 36, l, 2, DIM);
                }
            }
            Role::TeleIn | Role::TeleOut => {
                let code = tele_code(b.cfg);
                for i in 0..4 {
                    let d = match code {
                        Some(v) => format!("{}", v / [1000, 100, 10, 1][i] % 10),
                        None => "-".to_string(),
                    };
                    c.text_center(p.0 + 178 + i as i32 * 70, p.1 + 100, &d, 5, if code.is_some() { GOLD } else { DIM });
                }
                c.text(p.0 + 20, p.1 + 110, "КОД", 2, DIM);
                let s = match code {
                    Some(v) => {
                        let tx = w_count(&self.world, Role::TeleIn, v);
                        let rx = w_count(&self.world, Role::TeleOut, v);
                        format!("НА КОДЕ {:04}: ПЕРЕДАТЧИКОВ {}, ПРИЁМНИКОВ {}", v, tx, rx)
                    }
                    None => "КОД НЕ ЗАДАН: ТЕЛЕПОРТ НЕ РАБОТАЕТ".to_string(),
                };
                c.text(p.0 + 20, p.1 + 192, &s, 2, TEXT);
let rate = format!("КРОЛИЧЬЯ НОРА: ВСЁ, ЧТО ВОШЛО В ПЕРЕДАТЧИК, ЧЕРЕЗ {} С ВЫХОДИТ У ПРИЁМНИКА. ПОТОК НЕ ОГРАНИЧЕН.", TELE_PERIOD);
                c.text(p.0 + 20, p.1 + 216, &rate, 1, DIM);
                c.text(p.0 + 20, p.1 + 232, "КОД МОЖНО НАБРАТЬ ЦИФРАМИ С КЛАВИАТУРЫ. НЕСКОЛЬКО ПРИЁМНИКОВ ПОЛУЧАЮТ ПО ОЧЕРЕДИ.", 1, DIM);
            }
            Role::Ratio => {
                let (n, m) = ratio_nm(b.cfg);
                c.text(p.0 + 20, p.1 + 44, "СНАЧАЛА N ПРЕДМЕТОВ В ПЕРВЫЙ ВЫХОД, ПОТОМ M ВО ВТОРОЙ", 2, TEXT);
                for (i, (l, v)) in [("ВПЕРЁД", n), ("ВПРАВО", m)].iter().enumerate() {
                    let bx = p.0 + 60 + i as i32 * 330;
                    c.text(bx, p.1 + 96, l, 2, DIM);
                    c.text_center(bx + 220, p.1 + 90, &format!("{}", v), 4, GOLD);
                }
                c.text(p.0 + 20, p.1 + 136, "ЕСЛИ НУЖНЫЙ ВЫХОД ЗАНЯТ, ДЕЛИТЕЛЬ ЖДЁТ - ПРОПОРЦИЯ ТОЧНАЯ.", 2, DIM);
            }
            _ => {}
        }
        for bt in self.cfg_buttons() {
            let hov = inside(bt.r, self.mx, self.my);
            let r = bt.r;
            c.fill_rect(r.0, r.1, r.2, r.3, if hov { hex(0x5A4E6C) } else { hex(0x3A3346) });
            c.rect_outline(r.0, r.1, r.2, r.3, 2, if bt.on { GOLD } else { PANEL_HI });
            match bt.icon {
                Some(it) => {
                    self.draw_item(c, it, r.0 + 5, r.1 + 5, 2);
                    if hov {
                        tip = bt.label.clone();
                    }
                }
                None => {
                    let col = if bt.on { GOLD } else { TEXT };
                    c.text(r.0 + (r.2 - text_w(&bt.label, 2)) / 2, r.1 + (r.3 - 14) / 2, &bt.label, 2, col);
                }
            }
        }
        c.text(p.0 + 20, p.1 + p.3 - 28, &tip, 2, DIM);
    }

    fn draw_item(&self, c: &mut Image, it: Item, x: i32, y: i32, scale: i32) {
        self.assets.draw_item(c, it, x, y, scale);
    }

    /// Набор кода телепорта с клавиатуры: новая цифра вдвигается справа.
    fn type_code(&mut self, digit: usize) {
        if let Some((x, y, b)) = self.cfg_target() {
            if matches!(b.kind.role(), Role::TeleIn | Role::TeleOut) {
                let code = tele_code(b.cfg).unwrap_or(0);
                self.apply_cfg(x, y, tele_cfg(code * 10 % TELE_CODES + digit));
                self.audio.play(560.0, 0.05, 0.1, Wave::Tri, 0.0);
            }
        }
    }

    /// Клавиша F сбрасывает образец фильтра под курсором.
    fn filter_reset_at(&mut self, x: i32, y: i32) {
        let is_filter = self.world.get(x, y).map_or(false, |b| b.kind.role() == Role::Filter);
        if !is_filter {
            return;
        }
        if self.net.mode == Mode::Client {
            self.net.send_cmd(&Cmd::FilterReset(x, y));
        } else if self.world.filter_reset(x, y) {
            self.net.broadcast_op(&Op::FilterReset(x, y));
        }
        self.say("ОБРАЗЕЦ СБРОШЕН: ФИЛЬТР ЗАПОМНИТ СЛЕДУЮЩИЙ ПРЕДМЕТ");
        self.audio.play(560.0, 0.05, 0.1, Wave::Tri, 0.0);
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
        let world = &self.world;
        let all: Vec<(usize, Kind)> = (0..TAB_NAMES.len())
            .flat_map(|t| Kind::in_tab(t).into_iter().map(move |k| (t, k)))
            .filter(|(_, k)| world.unlocked(*k))
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
        let rock = World::inb(x, y) && self.world.rocks[(y * W + x) as usize];
        if rock && self.world.money < ROCK_COST {
            self.error("ПРЕГРАДА: СНОС СТОИТ $2000");
            return false;
        }
        if rock && self.net.mode == Mode::Client {
            self.net.send_cmd(&Cmd::Remove(x, y));
            return true;
        }
        // При открытом слое проводов снимается сначала деталь логики.
        if self.wires_on() && self.remove_node_at(x, y) {
            return true;
        }
        self.remove_building_at(x, y)
    }

    /// Снимает деталь логики с тайла. false - детали там нет.
    fn remove_node_at(&mut self, x: i32, y: i32) -> bool {
        if self.world.node(x, y).is_none() {
            return false;
        }
        if self.net.mode == Mode::Client {
            self.net.send_cmd(&Cmd::LRemove(x, y));
        } else if self.world.remove_logic(x, y) {
            self.net.broadcast_op(&Op::LClear(x, y));
        }
        true
    }

    /// Сносит постройку (или преграду) на тайле, не трогая слой сигналов над ней.
    fn remove_building_at(&mut self, x: i32, y: i32) -> bool {
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
        self.world = own.unwrap_or_else(|| World::new_game(self.time.to_bits() as u64 + 1));
        self.tool = None;
    }

    /// Сетевые события: хост раздаёт мир и исполняет команды, клиент повторяет за хостом.
    fn net_update(&mut self, dt: f32, tx: i32, ty: i32) {
        // Остальные игроки видят курсор, выбранную постройку, рамку выделения и вставку.
        let (mode, rect) = if self.pasting {
            let (w, h) = self.clip_size;
            (2, (tx - w / 2, ty - h / 2, tx - w / 2 + w - 1, ty - h / 2 + h - 1))
        } else if let Some(r) = self.sel {
            (1, r)
        } else {
            (0, (0, 0, 0, 0))
        };
        self.net.set_presence(Presence {
            x: tx,
            y: ty,
            tool: self.tool.map_or(255, |k| k.0),
            dir: self.dir as u8,
            mode,
            rect,
        });
        for ev in self.net.poll(dt) {
            match ev {
                NetEvent::Hello(id, _) => self.net.send_world(id, &self.world.save()),
                NetEvent::Cmd(c) => match c {
                    Cmd::Place(k, x, y, d) => {
                        if self.world.unlocked(k) && self.world.place(k, x, y, d) == Ok(true) {
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
                    Cmd::SetPorts(x, y, p) => self.apply_ports(x, y, p),
                    Cmd::FilterReset(x, y) => {
                        if self.world.filter_reset(x, y) {
                            self.net.broadcast_op(&Op::FilterReset(x, y));
                        }
                    }
                    Cmd::SetCfg(x, y, v) => self.apply_cfg(x, y, v),
                    Cmd::LRemove(x, y) => {
                        if self.world.remove_logic(x, y) {
                            self.net.broadcast_op(&Op::LClear(x, y));
                        }
                    }
                    Cmd::LCfg(x, y, v) => self.apply_lparam(x, y, v),
                    Cmd::Order(i) => {
                        let _ = self.world.take_order(i as usize);
                    }
                    Cmd::Skin(i) => {
                        self.world.set_skin(i);
                    }
                    Cmd::Prestige => self.do_prestige(),
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
                    Op::Ports(x, y, p) => {
                        self.world.set_ports(x, y, p);
                    }
                    Op::FilterReset(x, y) => {
                        self.world.filter_reset(x, y);
                    }
                    Op::Cfg(x, y, v) => {
                        self.world.set_cfg(x, y, v);
                    }
                    Op::LClear(x, y) => self.world.force_clear_logic(x, y),
                    Op::LCfg(x, y, v) => {
                        self.world.set_lparam(x, y, v);
                    }
                },
                NetEvent::Logic(ticks, st) => {
                    if self.world.replica {
                        self.world.apply_logic_state(ticks, &st);
                    }
                }
                NetEvent::State(s) => {
                    if self.world.replica {
                        self.world.apply_state(s.money, s.level as usize, s.delivered, s.upgrades, &s.discovered);
                        self.world.sync_time(s.time);
                        for l in s.extra.lines() {
                            self.world.econ_line(l);
                        }
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
                time: self.world.time,
                discovered: self.world.discovered.clone(),
                // Рынок, гильдии, мегапроект и виток - раз в секунду, а не с каждым состоянием.
                extra: if self.state_n % 3 == 0 { self.world.econ_save() } else { String::new() },
            });
            self.state_n += 1;
            self.net.broadcast_logic(self.world.logic_ticks, &self.world.logic_state());
            self.world.logic_dirty = false;
            self.logic_t = LOGIC_PUSH;
        }
        // Датчик сработал между плановыми рассылками: клиенты узнают об этом сразу, а не
        // через треть секунды, - но не чаще десяти раз в секунду.
        self.logic_t -= dt;
        if self.world.logic_dirty && self.logic_t <= 0.0 {
            self.world.logic_dirty = false;
            if self.net.mode == Mode::Host && !self.net.peers.is_empty() {
                self.logic_t = LOGIC_PUSH;
                self.net.broadcast_logic(self.world.logic_ticks, &self.world.logic_state());
            }
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

        // Открытая библиотека чертежей забирает клавиатуру целиком: в ней набирают имя.
        if self.show_bp {
            for ch in &inp.typed {
                self.bp_type(*ch);
            }
            if inp.wheel != 0.0 {
                let max = self.blueprints.len().saturating_sub(self.bp_rows());
                self.bp_scroll = (self.bp_scroll as i32 - inp.wheel.signum() as i32).clamp(0, max as i32) as usize;
            }
        }
        for k in &inp.pressed {
            if self.show_bp {
                match k {
                    Key::Escape => self.close_modals(),
                    Key::Enter | Key::NumPadEnter => self.bp_save(),
                    Key::Backspace => {
                        self.bp_name.pop();
                    }
                    Key::F11 => self.toggle_fullscreen = true,
                    _ => {}
                }
                continue;
            }
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
                Key::Key0 => 9,
                _ => usize::MAX,
            };
            // Открытое окно настройки забирает клавиатуру: цифры набирают код телепорта.
            if self.show_cfg.is_some() {
                match k {
                    Key::Escape | Key::E => self.show_cfg = None,
                    _ if num < 10 => self.type_code((num + 1) % 10),
                    _ => {}
                }
                continue;
            }
            if self.show_codex && num < 3 {
                self.codex_p = num as u8 + 1;
                continue;
            }
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
                Key::F => self.filter_reset_at(tx, ty),
                Key::E if !self.modal() => {
                    if !self.open_cfg(tx, ty) {
                        self.error("У ЭТОЙ ПОСТРОЙКИ НЕТ НАСТРОЕК");
                    }
                }
                Key::P => self.show_path = !self.show_path,
                Key::G => {
                    let v = !self.show_guild;
                    self.close_modals();
                    self.show_guild = v;
                }
                // CTRL+V - вставка из буфера, она разбирается ниже.
                Key::V if !inp.ctrl => {
                    let v = !self.show_great;
                    self.close_modals();
                    self.show_great = v;
                }
                Key::B => {
                    let v = !self.show_bp;
                    self.close_modals();
                    self.show_bp = v;
                    self.bp_name.clear();
                }
                Key::Z if !inp.ctrl => self.switch_layer(),
                Key::L => {
                    self.show_wires = !self.show_wires;
                    self.say(if self.show_wires { "СЛОЙ ПРОВОДОВ ПОКАЗАН" } else { "СЛОЙ ПРОВОДОВ СКРЫТ" });
                }
                Key::F3 => self.show_fps = !self.show_fps,
                Key::F11 => self.toggle_fullscreen = true,
                Key::Enter if inp.alt => self.toggle_fullscreen = true,
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
                    let v = !self.show_codex;
                    self.close_modals();
                    self.show_codex = v;
                }
                Key::T => {
                    let v = !self.show_stats;
                    self.close_modals();
                    self.show_stats = v;
                    self.stats_scroll = 0;
                }
                Key::F2 => {
                    let v = !self.show_net;
                    self.close_modals();
                    self.show_net = v;
                    if self.show_net {
                        self.net.ensure();
                    }
                }
                Key::Escape => {
                    if self.modal() {
                        self.close_modals();
                    } else if self.pasting || self.sel.is_some() {
                        self.pasting = false;
                        self.sel = None;
                    } else {
                        self.tool = None;
                    }
                }
                Key::F1 | Key::H => {
                    let v = !self.show_help;
                    self.close_modals();
                    self.show_help = v;
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

        if self.show_stats && inp.wheel != 0.0 {
            self.stats_scroll = (self.stats_scroll - inp.wheel.signum() as i32 * 3).max(0);
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
                if self.show_cfg.is_some() && self.cfg_node {
                    self.click_node(inp.mx, inp.my);
                } else if self.show_cfg.is_some() {
                    self.click_cfg(inp.mx, inp.my);
                } else if self.show_net {
                    self.click_net(inp.mx, inp.my);
                } else if self.show_guild {
                    self.click_guild(inp.mx, inp.my);
                } else if self.show_great {
                    self.click_great(inp.mx, inp.my);
                } else if self.show_bp {
                    self.click_bp(inp.mx, inp.my);
                } else if let (true, Some(p)) = (self.show_codex, (0..3).find(|i| inside(self.codex_pbtn(*i), inp.mx, inp.my))) {
                    self.codex_p = p as u8 + 1;
                    self.audio.play(440.0, 0.04, 0.06, Wave::Tri, 0.0);
                } else {
                    self.close_modals();
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
        // Щелчок без инструмента по краю машины переключает режим этой стороны.
        let free = self.tool.is_none() && !self.pasting && !inp.shift && !self.modal();
        if inp.lmb_pressed && free && !self.ui_captured && !self.over_ui(inp.mx, inp.my) {
            self.click_at = Some((inp.mx, inp.my));
        }
        if let Some((cx, cy)) = self.click_at {
            if (inp.mx - cx).abs() + (inp.my - cy).abs() > 6 || !free {
                self.click_at = None;
            } else if !inp.lmb {
                self.click_at = None;
                self.click_port(inp.mx, inp.my);
            }
        }
        self.cam_x = self.cam_x.clamp(0.0, W as f32);
        self.cam_y = self.cam_y.clamp(0.0, H as f32);

        // Сеть, симуляция и события.
        self.net_update(dt, tx, ty);
        self.world.update(dt.min(0.1));
        // Фильтр сам запомнил образец - клиенты должны узнать о нём от хоста.
        for (x, y, v) in std::mem::take(&mut self.world.pending_cfg) {
            self.net.broadcast_op(&Op::Cfg(x, y, v));
        }
        if self.show_cfg.is_some() && self.cfg_target().is_none() && self.node_target().is_none() {
            self.show_cfg = None;
        }
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
        for m in std::mem::take(&mut self.world.pending_msgs) {
            self.say(&m);
            self.audio.play(784.0, 0.3, 0.1, Wave::Tri, 0.0);
        }
        for i in std::mem::take(&mut self.world.pending_ach) {
            let (id, name) = ACH_NAMES[i.min(ACH_NAMES.len() - 1)];
            self.net.achievement(id);
            self.banner = Some(("ДОСТИЖЕНИЕ".to_string(), name.to_string(), 4.0));
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
        } else if self.show_cfg.is_some() && self.cfg_node {
            self.draw_node_cfg(c);
        } else if self.show_cfg.is_some() {
            self.draw_cfg(c);
        } else if self.show_stats {
            self.draw_stats(c);
        } else if self.show_guild {
            self.draw_guild(c);
        } else if self.show_great {
            self.draw_great(c);
        } else if self.show_bp {
            self.draw_bp(c);
        }
    }

    fn draw_world(&self, c: &mut Image) {
        let ts = self.ts();
        let z = self.zoom;
        let (ox, oy) = self.origin();
        let x0 = (-ox).div_euclid(ts).max(0);
        let y0 = (-oy).div_euclid(ts).max(0);
        let x1 = (c.w - 1 - ox).div_euclid(ts).min(W - 1);
        // Рисуется только свой слой: строки поверхности или строки подземелья.
        let base = self.layer * DUNGEON_Y;
        let y0 = y0.max(base);
        let y1 = (c.h - 1 - oy).div_euclid(ts).min(base + H - 1);
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
                let i = (y * W + x) as usize;
                if w.rocks[i] {
                    c.blit(&a.rocks[hsh % 2], ox + x * ts, oy + y * ts, z);
                } else if w.rich[i] {
                    // Жила чистоты II: золотая рамка и искры.
                    c.rect_outline(ox + x * ts + z, oy + y * ts + z, ts - 2 * z, ts - 2 * z, z, 0xC0F0_C24B);
                    for (dx, dy) in [(5, 5), (17, 8), (9, 17)] {
                        c.fill_rect(ox + x * ts + dx * z, oy + y * ts + dy * z, 2 * z, 2 * z, hex(0xFFF6DC));
                    }
                }
            }
        }

        // Под землёй темно: пол и порода приглушены, жилы и постройки остаются яркими.
        if self.layer == 1 {
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let i = (y * W + x) as usize;
                    let dark = if w.rocks[i] { 0xD00C_0A14 } else if w.veins[i] > 0 { 0x500C_0A14 } else { 0xA80C_0A14 };
                    c.fill_rect(ox + x * ts, oy + y * ts, ts, ts, dark);
                }
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
                        // В ленту входят с обоих боков, сзади пусто: хвост за стыком не рисуется.
                        let tee = !back && left && right;
                        let img = if curve_l {
                            &a.belt_l[style][fr]
                        } else if curve_r {
                            &a.belt_r[style][fr]
                        } else if tee {
                            &a.belt_t[style][fr]
                        } else {
                            &a.belt[style][fr]
                        };
                        let rot = b.dir as u8;
                        c.blit_ex(img, sx, sy, z, rot, 255);
                        // Скин гильдии: цветные борта на прямых участках.
                        if w.skin > 0 && !curve_l && !curve_r && !tee {
                            let col = ELEM_COLORS[(w.skin as usize - 1) % 4];
                            if b.dir.dx() == 0 {
                                c.fill_rect(sx + 3 * z, sy, z, ts, col);
                                c.fill_rect(sx + 20 * z, sy, z, ts, col);
                            } else {
                                c.fill_rect(sx, sy + 3 * z, ts, z, col);
                                c.fill_rect(sx, sy + 20 * z, ts, z, col);
                            }
                        }
                        // Стыки: боковые примыкания и заглушки свободных торцов.
                        if left && !curve_l {
                            c.blit_ex(&a.belt_stub_l, sx, sy, z, rot, 255);
                        }
                        if right && !curve_r {
                            c.blit_ex(&a.belt_stub_r, sx, sy, z, rot, 255);
                        }
                        if !back && !curve_l && !curve_r && !tee {
                            c.blit_ex(&a.belt_cap, sx, sy, z, rot, 255);
                        }
                        if !w.accepts_from(x + b.dir.dx(), y + b.dir.dy(), b.dir) {
                            c.blit_ex(&a.belt_cap, sx, sy, z, rot + 2, 255);
                        }
                    }
                    // Настил моста лежит вдоль направления постройки.
                    Role::Bridge => c.blit_ex(self.sprite(b.kind, 0), sx, sy, z, b.dir as u8, 255),
                    Role::Tunnel => {
                        let exit = w.tunnel_is_exit(x, y);
                        c.blit_ex(self.sprite(b.kind, exit as usize), sx, sy, z, b.dir as u8, 255);
                    }
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
                // У пары подземных конвейеров видна только открытая половина клетки.
                let tunnel = b.kind.role() == Role::Tunnel;
                let exit = tunnel && w.tunnel_is_exit(x, y);
                let paired = exit || (tunnel && w.tunnel_exit(x, y).is_some());
                for lane in &b.lanes {
                    for bi in lane {
                        if paired && (bi.p < 0.5) == exit {
                            continue;
                        }
                        let out = if b.kind.is_belt() || tunnel { b.dir } else { bi.d };
                        let (fx, fy) = if bi.p < 0.5 {
                            let t = 0.5 * (1.0 - bi.p * 2.0);
                            (0.5 - bi.d.dx() as f32 * t, 0.5 - bi.d.dy() as f32 * t)
                        } else {
                            let t = bi.p - 0.5;
                            (0.5 + out.dx() as f32 * t, 0.5 + out.dy() as f32 * t)
                        };
                        let px = ox + x * ts + (fx * ts as f32) as i32 - ITEM * z / 2;
                        let py = oy + y * ts + (fy * ts as f32) as i32 - ITEM * z / 2;
                        a.draw_item(c, bi.item, px, py, z);
                    }
                }
            }
        }

        // Машины.
        let hover = Some(self.tile_at(self.mx, self.my)).filter(|_| self.tool.is_none() && !self.pasting);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let b = match w.get(x, y) {
                    Some(b) if !b.kind.is_track() && b.kind != Kind::Hub => b,
                    _ => continue,
                };
                let (sx, sy) = (ox + x * ts, oy + y * ts);
                let frame = if b.working { anim + (x * 3 + y * 5) as usize } else { 0 };
                // У машин с настраиваемыми сторонами корпус рисуется без патрубков: стороны
                // ложатся поверх по настоящей настройке, а не по рисунку.
                let ported = port_modes(b.kind) > 0;
                if ported {
                    let f = &a.bare[b.kind.0 as usize];
                    c.blit_ex(&f[frame % f.len()], sx, sy, z, b.dir as u8, 255);
                } else {
                    c.blit_ex(self.sprite(b.kind, frame), sx, sy, z, b.dir as u8, 255);
                }
                // Уровень машины виден по отделке корпуса: бронза, серебро, золото, золото с камнями.
                if b.level >= 2 {
                    let f = &a.trim[(b.level as usize - 2).min(a.trim.len() - 1)];
                    c.blit(&f[(anim / 3) % f.len()], sx, sy, z);
                }
                // Стороны: выход - светлый патрубок с оранжевой меткой, вход - тёмное гнездо с
                // зелёной, сторона отказа - фиолетовая, закрытая сторона - гладкий корпус.
                // У машины под курсором стороны залиты цветом целиком, закрытые - тёмно-красным.
                let hovered = hover == Some((x, y));
                if ported {
                    for rel in 0..4 {
                        let (rx, ry, rw, rh) = match Dir::from_u8(b.dir as u8 + rel as u8) {
                            Dir::N => (8, 0, 8, 4),
                            Dir::S => (8, 20, 8, 4),
                            Dir::W => (0, 8, 4, 8),
                            Dir::E => (20, 8, 4, 8),
                        };
                        let reject = reject_rel(b) == Some(rel);
                        let (base, mark) = if reject {
                            (hex(0xC860E8), hex(0xC860E8))
                        } else if side_out(b, rel) {
                            (hex(0xD8DCE6), hex(0xFFB03A))
                        } else if side_in(b, rel) {
                            (hex(0x14121A), hex(0x7ED67A))
                        } else if hovered {
                            (hex(0x8A2A2A), hex(0x8A2A2A))
                        } else {
                            continue;
                        };
                        c.fill_rect(sx + rx * z, sy + ry * z, rw * z, rh * z, OUTLINE);
                        let (ix, iy, iw, ih) = (rx + 1, ry + 1, rw - 2, rh - 2);
                        c.fill_rect(sx + ix * z, sy + iy * z, iw * z, ih * z, if hovered { mark } else { base });
                        if !hovered {
                            // Цветная метка посередине патрубка.
                            let (mx, my) = (ix + iw / 2 - 1, iy + ih / 2 - 1);
                            c.fill_rect(sx + mx * z, sy + my * z, 2 * z, 2 * z, mark);
                        }
                    }
                }
                let (cx, cy) = (sx + ts / 2, sy + ts / 2);
                let icon = |c: &mut Image, it: Item| a.draw_item(c, it, cx - ITEM * z / 2, cy - ITEM * z / 2, z);
                match b.kind.role() {
                    Role::Extractor(_) => {
                        let v = w.vein(x, y);
                        if v > 0 {
                            c.fill_circle(cx as f32, cy as f32, 3.1 * z as f32, ELEM_COLORS[v as usize - 1]);
                        }
                    }
                    Role::Filter => {
                        let cond = Cond::from_bits(b.cfg);
                        if !cond.is_any() {
                            icon(c, cond.icon());
                        }
                    }
                    Role::Packer | Role::Unpacker => {
                        if let Some(it) = b.slots[0].filter(|_| b.count > 0).or(b.out) {
                            icon(c, it);
                            if z >= 2 && b.count > 0 {
                                c.text_center(cx, sy + ts - 8 * z, &format!("{}", b.count), z - 1, TEXT);
                            }
                        }
                    }
                    Role::TeleIn | Role::TeleOut => {
                        if let Some(it) = b.lanes[0].first().map(|bi| bi.item).or(b.out) {
                            icon(c, it);
                        }
                        if z >= 2 {
                            let s = tele_code(b.cfg).map_or("----".to_string(), |v| format!("{:04}", v));
                            c.text_center(cx, sy + ts - 8 * z, &s, z - 1, if tele_code(b.cfg).is_some() { GOLD } else { RED });
                        }
                    }
                    Role::Altar => {
                        if let Some(it) = b.slots[0] {
                            icon(c, it);
                            // Полоска: сколько изделию осталось гореть.
                            let bw = ((1.0 - b.progress).clamp(0.0, 1.0) * 16.0) as i32 * z;
                            c.fill_rect(sx + 4 * z, sy + 20 * z, 16 * z, 2 * z, OUTLINE);
                            c.fill_rect(sx + 4 * z, sy + 20 * z, bw, 2 * z, hex(0xFFB03A));
                        }
                    }
                    Role::Ratio => {
                        if z >= 2 {
                            let (n, m) = ratio_nm(b.cfg);
                            c.text_center(cx, cy - 3 * (z - 1), &format!("{}:{}", n, m), z - 1, GOLD);
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
                    Role::Mixer | Role::Distiller | Role::Crucible | Role::Transmuter => {
                        if let Some(it) = b.out {
                            icon(c, it);
                        }
                    }
                    _ => {}
                }
            }
        }

        // Лифты: на другом слое на той же клетке должна стоять пара. Место под пару показано
        // призраком, лифт без пары обведён красным.
        for y in y0..=y1 {
            for x in x0..=x1 {
                let is_lift = |x: i32, y: i32| w.get(x, y).map_or(false, |b| b.kind.role() == Role::Lift);
                let (px, py) = other_layer(x, y);
                let (sx, sy) = (ox + x * ts, oy + y * ts);
                match (is_lift(x, y), is_lift(px, py)) {
                    (false, true) => {
                        if w.get(x, y).is_none() {
                            c.blit_ex(self.sprite(Kind::by_name("ЛИФТ"), 0), sx, sy, z, w.get(px, py).map_or(0, |b| b.dir as u8), 110);
                        }
                        c.rect_outline(sx, sy, ts, ts, z, 0xE058_D4F0);
                    }
                    (true, false) => c.rect_outline(sx, sy, ts, ts, z, RED),
                    _ => {}
                }
            }
        }

        // Остановленные исполнителем постройки помечены всегда; сам слой сигналов - по клавише L.
        let wires = self.wires_on();
        if wires {
            c.fill_rect(0, 0, c.w, c.h, 0x5010_0E16);
        }
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (sx, sy) = (ox + x * ts, oy + y * ts);
                if w.is_halted(x, y) {
                    c.fill_rect(sx + 17 * z, sy + z, 6 * z, 6 * z, OUTLINE);
                    c.fill_rect(sx + 18 * z, sy + 2 * z, 4 * z, 4 * z, RED);
                }
                let nd = match w.node(x, y) {
                    Some(n) if wires => n,
                    _ => continue,
                };
                if nd.lk() == Lk::Wire {
                    let on = w.powered[(y * W + x) as usize];
                    let col = if on { hex(0xFFE25A) } else { hex(0x8A6A2A) };
                    let (cx, cy) = (sx + ts / 2, sy + ts / 2);
                    for d in [Dir::N, Dir::E, Dir::S, Dir::W] {
                        if w.node(x + d.dx(), y + d.dy()).is_some() {
                            let (ex, ey) = (cx + d.dx() * ts / 2, cy + d.dy() * ts / 2);
                            let (ax, ay) = (cx.min(ex) - z, cy.min(ey) - z);
                            c.fill_rect(ax - 1, ay - 1, (cx - ex).abs() + 2 * z + 2, (cy - ey).abs() + 2 * z + 2, OUTLINE);
                            c.fill_rect(ax, ay, (cx - ex).abs() + 2 * z, (cy - ey).abs() + 2 * z, col);
                        }
                    }
                    c.fill_rect(cx - 2 * z, cy - 2 * z, 4 * z, 4 * z, OUTLINE);
                    c.fill_rect(cx - z, cy - z, 2 * z, 2 * z, col);
                } else {
                    c.blit_ex(self.sprite(nd.kind, nd.out as usize), sx, sy, z, nd.dir as u8, 255);
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
            // Радиус ауры: у алтаря под курсором - настоящий, у алтаря в руках - базовый.
            let ring = match (self.tool, w.get(tx, ty)) {
                (Some(k), _) if k.role() == Role::Altar => Some(ALTAR_RADIUS),
                (None, Some(b)) if b.kind.role() == Role::Altar && !self.pasting => Some(
                    w.altars().iter().find(|a| (a.x, a.y) == (tx, ty)).map_or(ALTAR_RADIUS, |a| a.r),
                ),
                _ => None,
            };
            if let Some(r) = ring {
                let (rx, ry, rs) = (sx - r * ts, sy - r * ts, (2 * r + 1) * ts);
                c.fill_rect(rx, ry, rs, rs, 0x22F0_C24B);
                c.rect_outline(rx, ry, rs, rs, z.max(2), GOLD);
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
            for n in &self.clip_nodes {
                let (sx, sy) = (ox + (bx + n.dx) * ts, oy + (by + n.dy) * ts);
                if n.kind.role() == Role::Logic(Lk::Wire) {
                    c.fill_rect(sx + ts / 2 - 2 * z, sy + ts / 2 - 2 * z, 4 * z, 4 * z, 0xB0FF_E25A);
                } else {
                    c.blit_ex(self.sprite(n.kind, 0), sx, sy, z, n.dir as u8, 170);
                }
                if !World::inb(bx + n.dx, by + n.dy) {
                    c.fill_rect(sx, sy, ts, ts, 0x70E8_553A);
                }
            }
            c.rect_outline(ox + bx * ts, oy + by * ts, cw * ts, ch * ts, z.max(2), GOLD);
        }

        // Другие игроки: курсор, постройка в руках, рамка выделения или вставки.
        for p in &self.net.peers {
            let col = [hex(0x58B4F0), hex(0xF07AD8), hex(0x7ED67A), hex(0xFFB03A)][(p.id.raw() % 4) as usize];
            let pr = &p.p;
            // Игрок на другом слое карты здесь не виден.
            if layer_of(pr.y) != self.layer {
                continue;
            }
            let (sx, sy) = (ox + pr.x * ts, oy + pr.y * ts);
            let mut label_y = sy;
            let mut label = p.name.clone();
            if pr.mode > 0 {
                let (rx0, ry0) = (pr.rect.0.clamp(-1, W), pr.rect.1.clamp(base - 1, base + H));
                let (rx1, ry1) = (pr.rect.2.clamp(rx0, W), pr.rect.3.clamp(ry0, base + H));
                let (px, py) = (ox + rx0 * ts, oy + ry0 * ts);
                let (pw, ph) = ((rx1 - rx0 + 1) * ts, (ry1 - ry0 + 1) * ts);
                c.fill_rect(px, py, pw, ph, with_alpha(col, if pr.mode == 1 { 0x38 } else { 0x20 }));
                c.rect_outline(px, py, pw, ph, z.max(2), col);
                label_y = py;
                label.push_str(if pr.mode == 1 { ": ВЫДЕЛЯЕТ" } else { ": ВСТАВЛЯЕТ" });
            } else if let Some(kind) = Kind::from_u8(pr.tool) {
                c.blit_ex(self.sprite(kind, 0), sx, sy, z, pr.dir, 140);
            }
            c.rect_outline(sx, sy, ts, ts, z.max(2), col);
            c.text_center(ox + pr.x * ts + ts / 2, label_y.min(sy) - 18, &label, 2, col);
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

        if self.layer == 1 {
            c.text_center(c.w / 2, TOP_H + 12, "ПОДЗЕМЕЛЬЕ  (Z - НАВЕРХ)", 2, hex(0x58D4F0));
        }
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
        let help = "F2 - СЕТЬ  C - КАТАЛОГ  T - СТАТИСТИКА  F1 - ПОМОЩЬ";
        let hx = c.w - text_w(help, 2) - 14;
        if hx > x + 20 {
            c.text(hx, 15, help, 2, DIM);
        }

        if self.show_fps {
            self.draw_fps(c);
        }

        if let Some(tag) = &self.update_ready {
            let s = format!("ОБНОВЛЕНИЕ {} ГОТОВО: F5 - ПЕРЕЗАПУСК (ИЛИ ВСТАНЕТ ПРИ ВЫХОДЕ)", tag);
            let tw = text_w(&s, 2);
            c.fill_rect((c.w - tw) / 2 - 10, TOP_H + 6, tw + 20, 26, 0xE02B_2433);
            c.text((c.w - tw) / 2, TOP_H + 12, &s, 2, GREEN);
        }
        if let (Some((o, done, deadline)), false) = (w.order, self.modal()) {
            let left = (deadline - w.time).max(0.0) as i32;
            let s = format!("ЗАКАЗ ГИЛЬДИИ: {} {}/{}  {}:{:02}", o.item.name(), done, o.need, left / 60, left % 60);
            let tw = text_w(&s, 2);
            c.fill_rect(c.w - 230 - tw - 20, TOP_H + 8, tw + 20, 26, 0xD02B_2433);
            c.text(c.w - 230 - tw - 10, TOP_H + 14, &s, 2, ELEM_COLORS[o.guild as usize % 4]);
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
            let locked = !w.unlocked(*kind);
            let sel = self.tool == Some(*kind);
            let hov = inside(r, self.mx, self.my);
            c.fill_rect(r.0, r.1, r.2, r.3, if hov && !locked { 0xF04A_4058 } else { PANEL });
            c.rect_outline(r.0, r.1, r.2, r.3, 2, if sel { GOLD } else { PANEL_HI });
            c.blit_ex(self.sprite(*kind, 0), r.0 + 8, r.1 + 6, 2, 0, if locked { 70 } else { 255 });
            c.text(r.0 + 5, r.1 + 5, &format!("{}", (i + 1) % 10), 1, if sel { GOLD } else { DIM });
            if locked {
                c.text_center(r.0 + r.2 / 2, r.1 + 24, "ЗАКРЫТО", 1, RED);
                let when = if kind.role() == Role::GuildPost { "РЕПУТ.10".to_string() } else { format!("КОНТР.{}", kind.unlock_level()) };
                c.text_center(r.0 + r.2 / 2, r.1 + 62, &when, 1, DIM);
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
            let n = self.selected().len() + self.selected_nodes().len();
            Some((format!("ВЫДЕЛЕНО {}: DEL - СНЕСТИ, CTRL+C - КОПИЯ, CTRL+X - ПЕРЕНОС", n), TEXT))
        } else {
            self.tool.map(|k| (format!("{}: ЛКМ - СТРОИТЬ, R - ПОВЕРНУТЬ, ПКМ - ОТМЕНА", k.name()), DIM))
        };
        if let Some((msg, col)) = hint {
            // Длинная подсказка печатается мельче, чтобы не уезжать за края окна.
            let sc = if text_w(&msg, 2) > c.w - 40 { 1 } else { 2 };
            let tw = text_w(&msg, sc);
            c.fill_rect((c.w - tw) / 2 - 10, line_y - 6, tw + 20, 26, 0xD02B_2433);
            c.text((c.w - tw) / 2, line_y + (2 - sc) * 4, &msg, sc, col);
        }

        // Информация о тайле под курсором.
        if !hover_ui && self.tool.is_none() && !self.modal() && !self.pasting {
            let (tx, ty) = self.tile_at(self.mx, self.my);
            let mut lines: Vec<String> = Vec::new();
            if let Some(n) = w.node(tx, ty).filter(|_| self.wires_on()) {
                lines.push(format!("ЛОГИКА: {}", n.kind.name()));
                lines.push(node_text(n));
                if n.lk() == Lk::Wire {
                    let on = w.powered[(ty * W + tx) as usize];
                    lines.push((if on { "ПОД СИГНАЛОМ" } else { "БЕЗ СИГНАЛА" }).to_string());
                } else {
                    lines.push((if n.out { "ВЫХОД: ВКЛЮЧЁН" } else { "ВЫХОД: ВЫКЛЮЧЕН" }).to_string());
                }
                if lk_has_param(n.lk()) {
                    lines.push("E - НАСТРОЙКА, ПКМ - СНЯТЬ".to_string());
                }
            } else if let Some(b) = w.get(tx, ty) {
                lines.push(b.kind.name().to_string());
                if b.kind.role() == Role::Switch {
                    lines.push(match w.level_cost(tx, ty) {
                        Some(cost) => format!("ПЕРИОД {} С. U - ДОЛЬШЕ ЗА ${}", switch_period(b.level), fmt_money(cost)),
                        None => format!("ПЕРИОД {} С - МАКСИМУМ", switch_period(b.level)),
                    });
                } else if World::can_level(b.kind) {
                    lines.push(match w.level_cost(tx, ty) {
                        Some(cost) => {
                            format!("УР.{} (X{}). U - УЛУЧШИТЬ ЗА ${}", b.level, level_mult(b.level), fmt_money(cost))
                        }
                        None => format!("УР.{} (X{}) - МАКСИМУМ", b.level, level_mult(b.level)),
                    });
                }
                if can_reject(b.kind.role()) {
                    lines.push("ЛКМ ПО КРАЮ: ВХОД / ВЫХОД / ЗАКРЫТО / ОТКАЗ".to_string());
                    if let Some(r) = reject_rel(b) {
                        let side = ["ВПЕРЁД", "ВПРАВО", "НАЗАД", "ВЛЕВО"][r];
                        lines.push(format!("ОТКАЗ {}: ТУДА УХОДИТ ТО, ЧТО НЕ ПОДХОДИТ", side));
                    }
                } else if port_modes(b.kind) > 0 {
                    lines.push("ЛКМ ПО КРАЮ: ВХОД / ВЫХОД / ЗАКРЫТО".to_string());
                }
                if has_panel(b.kind) {
                    lines.push("E ИЛИ ЛКМ ПО ЦЕНТРУ - НАСТРОЙКА".to_string());
                }
                if b.kind.role() == Role::Filter {
                    lines.push("F - СБРОСИТЬ ОБРАЗЕЦ".to_string());
                }
                match b.kind.role() {
                    Role::Hub => lines.push("ПРОДАЁТ ВСЁ, ЧТО ПРИВЕЗУТ".to_string()),
                    Role::Belt(_) | Role::Bridge | Role::Trash | Role::Trade => {}
                    Role::Filter => {
                        let cond = Cond::from_bits(b.cfg);
                        lines.push(if cond.is_any() {
                            "ОБРАЗЕЦ НЕ ЗАДАН: ЗАПОМНИТ ПЕРВЫЙ ПРЕДМЕТ".to_string()
                        } else {
                            format!("ПРЯМО ИДЁТ: {}", cond.text())
                        });
                    }
                    Role::Packer => {
                        match b.slots[0].filter(|_| b.count > 0) {
                            Some(s) => lines.push(format!("В ЯЩИК: {} {}/{}", s.name(), b.count, CRATE_SIZE)),
                            None => lines.push(format!("ПУСТ: ЖДЁТ {} ОДИНАКОВЫХ ПРЕДМЕТОВ", CRATE_SIZE)),
                        }
                        if let Some(o) = b.out {
                            lines.push(format!("ГОТОВО: {} (${})", o.name(), fmt_money(o.value() * w.price_mult())));
                        }
                    }
                    Role::Unpacker => match b.slots[0].filter(|_| b.count > 0) {
                        Some(s) => lines.push(format!("В ЯЩИКЕ ОСТАЛОСЬ: {} X{}", s.name(), b.count)),
                        None => lines.push("ПУСТ: ЖДЁТ ЯЩИК".to_string()),
                    },
                    Role::TeleIn | Role::TeleOut => match tele_code(b.cfg) {
                        Some(v) => {
                            let (tx_n, rx_n) = (w_count(w, Role::TeleIn, v), w_count(w, Role::TeleOut, v));
                            lines.push(format!("КОД {:04}: ПЕРЕДАТЧИКОВ {}, ПРИЁМНИКОВ {}", v, tx_n, rx_n));
                            lines.push(format!("ПЕРЕЛЁТ {} С, БЕЗ ОГРАНИЧЕНИЯ ПОТОКА", TELE_PERIOD));
                            if !b.lanes[0].is_empty() {
                                lines.push(format!("В ПУТИ: {}", b.lanes[0].len()));
                            }
                            if b.kind.role() == Role::TeleIn && rx_n == 0 {
                                lines.push("НЕТ ПРИЁМНИКА С ТАКИМ КОДОМ".to_string());
                            }
                        }
                        None => lines.push("КОД НЕ ЗАДАН - НЕ РАБОТАЕТ".to_string()),
                    },
                    Role::Lift => {
                        let (px, py) = other_layer(tx, ty);
                        let pair = w.get(px, py).map_or(false, |p| p.kind.role() == Role::Lift);
                        let to = if layer_of(ty) == 0 { "В ПОДЗЕМЕЛЬЕ" } else { "НА ПОВЕРХНОСТЬ" };
                        if pair {
                            lines.push(format!("ВЕЗЁТ {}: {} С, В ШАХТЕ {} ИЗ {}", to, LIFT_TIME, b.lanes[0].len(), LIFT_QUEUE));
                        } else {
                            lines.push("ПАРЫ НЕТ: ПОСТАВЬ ЛИФТ НА ЭТОЙ ЖЕ КЛЕТКЕ ДРУГОГО СЛОЯ (Z)".to_string());
                        }
                        if let Some(o) = b.out {
                            lines.push(format!("ПРИЕХАЛО: {}", o.name()));
                        }
                    }
                    Role::Sorter => {
                        for (k, side) in ["ВПЕРЁД", "ВПРАВО", "ВЛЕВО"].iter().enumerate() {
                            let cond = sorter_cond(b.cfg, k);
                            let what = if cond.is_any() { "ОСТАЛЬНОЕ".to_string() } else { cond.text() };
                            lines.push(format!("{}: {}", side, what));
                        }
                    }
                    Role::Altar => match b.slots[0] {
                        Some(it) => {
                            let left = ((1.0 - b.progress) * ALTAR_BURN).ceil().max(0.0);
                            lines.push(format!("ГОРИТ: {} (ЕЩЁ {} С)", it.name(), left));
                            if let Some(q) = b.slots[1] {
                                lines.push(format!("В ОЧЕРЕДИ: {}", q.name()));
                            }
                            let s = aura_of(it);
                            let r = w.altars().iter().find(|a| (a.x, a.y) == (tx, ty)).map_or(ALTAR_RADIUS, |a| a.r);
                            lines.extend(aura_lines([s[0], s[1], s[2]], ""));
                            let own = aura_radius_bonus(s[3]);
                            let from = if r > ALTAR_RADIUS + own { " (ЭФИР СОСЕДНЕГО АЛТАРЯ)" } else { "" };
                            lines.push(format!("РАДИУС {} КЛ.: ОБЛАСТЬ {}X{}{}", r, 2 * r + 1, 2 * r + 1, from));
                        }
                        None => {
                            lines.push("ПУСТ: ПОДВЕЗИ ИЗДЕЛИЕ ЛЕНТОЙ (НЕ ЭССЕНЦИЮ)".to_string());
                            lines.push(format!("ИЗДЕЛИЕ ГОРИТ {} С, РАДИУС {} КЛ.", ALTAR_BURN, ALTAR_RADIUS));
                        }
                    },
                    Role::Ratio => {
                        let (n, m) = ratio_nm(b.cfg);
                        lines.push(format!("ПРОПОРЦИЯ {} : {} (ПРОШЛО {} ИЗ {})", n, m, b.count, n + m));
                    }
                    Role::Storage => match b.slots[0].filter(|_| b.count > 0) {
                        Some(s) => lines.push(format!("{} X{}", s.name(), b.count)),
                        None => lines.push("ПУСТО".to_string()),
                    },
                    _ => {
                        if b.kind.role() == Role::Opus {
                            let cells: Vec<String> =
                                (0..4).map(|e| format!("{} {}/{}", ELEM_NAMES[e], b.bins[e], OPUS_CELL)).collect();
                            lines.push(format!("МАГИСТЕРИИ: {}", cells.join(", ")));
                        }
                        for s in b.slots.iter().flatten() {
                            lines.push(format!("ВНУТРИ: {}", s.name()));
                        }
                        if let Some(o) = b.out {
                            lines.push(format!("ГОТОВО: {} (${})", o.name(), fmt_money(o.value() * w.price_mult())));
                        }
                        if let Some(o) = b.rej {
                            lines.push(format!("ОТКАЗ: {}", o.name()));
                        }
                        if aura_affects(b.kind.role()) {
                            lines.extend(aura_lines(w.aura_at(tx, ty), "АУРА: "));
                        }
                    }
                }
            }
            let v = w.vein(tx, ty);
            let ti = (ty * W + tx) as usize;
            if v > 0 {
                let pure = if World::inb(tx, ty) && w.rich[ti] { " (ЧИСТОТА II)" } else { "" };
                lines.push(format!("ЖИЛА: {}{}", ELEM_NAMES[v as usize - 1], pure));
            }
            if World::inb(tx, ty) && w.rocks[ti] {
                lines.push("ПРЕГРАДА".to_string());
                lines.push(format!("ПКМ - СНЕСТИ ЗА ${}", ROCK_COST));
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

    /// Размеры каталога: (панель, начало сетки, клетка, x карточки).
    fn codex_geom(&self) -> ((i32, i32, i32, i32), (i32, i32), i32, i32) {
        let (cell, lw, info_w) = (22, 130, 400);
        let bw = 16 + lw + 15 * cell + 20 + info_w + 16;
        let bh = 46 + N_FORMS as i32 * cell + 16;
        let (bx, by) = ((self.w - bw) / 2, (self.h - bh) / 2);
        let (gx, gy) = (bx + 16 + lw, by + 46);
        ((bx, by, bw, bh), (gx, gy), cell, gx + 15 * cell + 20)
    }

    /// Кнопка выбора чистоты в каталоге.
    fn codex_pbtn(&self, i: usize) -> (i32, i32, i32, i32) {
        let (p, (gx, _), cell, _) = self.codex_geom();
        (gx + 15 * cell - (3 - i as i32) * 48 + 4, p.1 + 10, 44, 28)
    }

    /// Клетка философского камня (он вне сетки) в карточке каталога.
    fn codex_stone(&self, p: usize) -> (i32, i32, i32, i32) {
        let (b, _, _, ix) = self.codex_geom();
        (ix + p as i32 * 44, b.1 + b.3 - 52, 36, 36)
    }

    /// Каталог: таблица предметов (форма x смесь) выбранной чистоты и карточка выбранного.
    fn draw_codex(&self, c: &mut Image) {
        let w = &self.world;
        c.fill_rect(0, 0, c.w, c.h, 0xA010_0E16);
        let ((bx, by, bw, bh), (gx, gy), cell, ix) = self.codex_geom();
        panel(c, (bx, by, bw, bh), GOLD);
        c.text(bx + 16, by + 16, &format!("КАТАЛОГ: {} ИЗ {}", w.catalog_found(), N_ITEM_IDS), 2, GOLD);
        let pur = self.codex_p.clamp(1, 3);
        for (i, l) in ["I", "II", "III"].iter().enumerate() {
            let r = self.codex_pbtn(i);
            let on = pur as usize == i + 1;
            let hov = inside(r, self.mx, self.my);
            c.fill_rect(r.0, r.1, r.2, r.3, if hov { hex(0x5A4E6C) } else { hex(0x3A3346) });
            c.rect_outline(r.0, r.1, r.2, r.3, 2, if on { GOLD } else { PANEL_HI });
            c.text(r.0 + (r.2 - text_w(l, 2)) / 2, r.1 + 7, l, 2, if on { GOLD } else { TEXT });
        }
        let mut hover: Option<Item> = None;
        for form in 0..N_FORMS {
            let y = gy + form as i32 * cell;
            c.text(bx + 16, y + 4, FORM_NAMES[form], 2, DIM);
            for m in 0..15 {
                let x = gx + m * cell;
                let it = Item::new(m as u8 + 1, pur, form as u8);
                let known = w.discovered[it.id()];
                let hov = inside((x, y, cell, cell), self.mx, self.my);
                c.fill_rect(x + 1, y + 1, cell - 2, cell - 2, if known { hex(0x4A4058) } else { hex(0x322C3C) });
                c.blit_ex(&self.assets.items[it.id()], x + 1, y + 1, 2, 0, if known { 255 } else { 70 });
                // Три точки внизу клетки: какие чистоты этого вида уже проданы.
                for p in 1..=3u8 {
                    if w.discovered[Item::new(it.mask, p, it.form).id()] {
                        c.fill_rect(x + 3 + (p as i32 - 1) * 6, y + cell - 4, 4, 2, [TEXT, hex(0xF4F4F8), GOLD][p as usize - 1]);
                    }
                }
                if hov {
                    c.rect_outline(x, y, cell, cell, 2, GOLD);
                    hover = Some(it);
                }
            }
        }
        // Философский камень - вершина вне сетки: три клетки по чистотам внизу карточки.
        c.text(ix, by + bh - 74, "ВНЕ СЕТКИ: ФИЛОСОФСКИЙ КАМЕНЬ", 2, DIM);
        for p in 0..3 {
            let r = self.codex_stone(p);
            let it = Item::new(15, p as u8 + 1, FORM_STONE);
            let known = w.discovered[it.id()];
            c.fill_rect(r.0, r.1, r.2, r.3, if known { hex(0x4A4058) } else { hex(0x322C3C) });
            c.blit_ex(&self.assets.items[it.id()], r.0 + 3, r.1 + 3, 3, 0, if known { 255 } else { 70 });
            if inside(r, self.mx, self.my) {
                c.rect_outline(r.0, r.1, r.2, r.3, 2, GOLD);
                hover = Some(it);
            }
        }
        // Карточка предмета.
        match hover {
            Some(it) => {
                let (machine, from) = form_recipe(it.form);
                c.fill_rect(ix, gy, 96, 96, hex(0x3A3346));
                c.blit(&self.assets.items[it.id()], ix + 8, gy + 8, 8);
                let mut lines = vec![
                    (it.name(), GOLD),
                    (String::new(), TEXT),
                    ("ГДЕ ДЕЛАЮТ:".to_string(), DIM),
                    (machine, TEXT),
                    ("ИЗ ЧЕГО:".to_string(), DIM),
                    (from, TEXT),
                    (String::new(), TEXT),
                    ("ЧИСТОТА: ЦЕНА, ПРЕМИЯ ЗА ПЕРВУЮ ПРОДАЖУ".to_string(), DIM),
                ];
                // У каждой чистоты своя позиция каталога и своя премия.
                for p in 1..=3u8 {
                    let v = Item::new(it.mask, p, it.form);
                    let mark = ["I", "II", "III"][p as usize - 1];
                    let price = fmt_money(v.value());
                    lines.push(if w.discovered[v.id()] {
                        (format!("{:<3} ${}  УЖЕ ПРОДАВАЛОСЬ", mark, price), GREEN)
                    } else {
                        (format!("{:<3} ${}  ПРЕМИЯ ${}", mark, price, fmt_money(v.first_sale_bonus())), GOLD)
                    });
                }
                let market = (w.market_factor(it) * 100.0).round();
                lines.push((format!("РЫНОК СЕЙЧАС: {}% ЦЕНЫ", market), if market < 99.0 { RED } else { DIM }));
                for (i, (l, col)) in lines.iter().enumerate() {
                    c.text(ix, gy + 112 + i as i32 * 22, l, 2, *col);
                }
            }
            None => {
                let by_p = |p: usize| (0..N_ITEM_IDS).filter(|i| i / 15 % 3 == p && w.discovered[*i]).count();
                let lines = [
                    "НАВЕДИ НА ПРЕДМЕТ, ЧТОБЫ УЗНАТЬ".to_string(),
                    "ЦЕНУ И СПОСОБ ПОЛУЧЕНИЯ.".to_string(),
                    String::new(),
                    "СТРОКИ - ФОРМЫ, СТОЛБЦЫ - СМЕСИ.".to_string(),
                    "КНОПКИ I, II, III (ИЛИ 1, 2, 3) -".to_string(),
                    "ЧИСТОТА. ТОЧКИ В КЛЕТКЕ - КАКИЕ".to_string(),
                    "ЧИСТОТЫ ВИДА УЖЕ ПРОДАНЫ.".to_string(),
                    String::new(),
                    format!("ЧИСТОТА I:   {} ИЗ {}", by_p(0), N_KINDS),
                    format!("ЧИСТОТА II:  {} ИЗ {}", by_p(1), N_KINDS),
                    format!("ЧИСТОТА III: {} ИЗ {}", by_p(2), N_KINDS),
                    String::new(),
                    "ЗА ПЕРВУЮ ПРОДАЖУ КАЖДОЙ ПОЗИЦИИ".to_string(),
                    "ЛАВКА ПЛАТИТ ПРЕМИЮ.".to_string(),
                    String::new(),
                    "ЩЕЛЧОК МИМО ИЛИ C - ЗАКРЫТЬ".to_string(),
                ];
                for (i, l) in lines.iter().enumerate() {
                    c.text(ix, gy + i as i32 * 22, l, 2, if i == lines.len() - 1 { GOLD } else { TEXT });
                }
            }
        }
    }

    /// Статистика: что лавка продала за последние минуты и чего не хватает до полного каталога.
    fn draw_stats(&self, c: &mut Image) {
        let w = &self.world;
        c.fill_rect(0, 0, c.w, c.h, 0xA010_0E16);
        let (bw, bh) = (1180.min(c.w - 20), 640.min(c.h - 20));
        let (bx, by) = ((c.w - bw) / 2, (c.h - bh) / 2);
        panel(c, (bx, by, bw, bh), GOLD);
        let rows = ((bh - 110) / 24).max(1) as usize;
        let mid = bx + bw / 2;

        // Слева: продажи в лавке за окно статистики.
        let sales = w.recent_sales();
        let mins = w.stat_window() / 60.0;
        let total: u32 = sales.iter().map(|s| s.1).sum();
        let title = format!("ПРОДАЖИ В ЛАВКЕ ЗА ПОСЛЕДНИЕ {:.1} МИН", mins);
        c.text(bx + 20, by + 16, &title, 2, GOLD);
        let sum = format!("ПОЗИЦИЙ {}, ШТУК {} ({:.0} В МИНУТУ)", sales.len(), total, total as f64 / mins);
        c.text(bx + 20, by + 40, &sum, 2, TEXT);
        c.text(bx + 20, by + 66, "ПОЗИЦИЯ", 1, DIM);
        c.text(mid - 250, by + 66, "ШТУК", 1, DIM);
        c.text(mid - 180, by + 66, "В МИНУТУ", 1, DIM);
        c.text(mid - 100, by + 66, "ЦЕНА РЫНКА", 1, DIM);
        for (i, (id, n)) in sales.iter().take(rows).enumerate() {
            let y = by + 80 + i as i32 * 24;
            let it = Item::from_id(*id);
            c.blit(&self.assets.items[*id], bx + 20, y, 2);
            c.text(bx + 48, y + 3, &it.name(), 2, TEXT);
            c.text(mid - 250, y + 3, &format!("{}", n), 2, TEXT);
            c.text(mid - 180, y + 3, &format!("{:.1}", *n as f64 / mins), 2, DIM);
            // Цена позиции сейчас - с учётом насыщения рынка.
            let f = w.market_factor(it);
            c.text(mid - 100, y + 3, &format!("{}%", (f * 100.0).round()), 2, if f < 0.6 { RED } else { GOLD });
        }
        if sales.is_empty() {
            c.text(bx + 20, by + 84, "ПОКА НИЧЕГО НЕ ПРОДАНО", 2, DIM);
        } else if sales.len() > rows {
            c.text(bx + 20, by + bh - 24, &format!("... И ЕЩЁ ПОЗИЦИЙ: {}", sales.len() - rows), 1, DIM);
        }
        if w.replica {
            c.text(bx + 20, by + bh - 40, "В ГОСТЯХ ЭТО ОЦЕНКА ПО ТВОЕЙ КОПИИ МИРА", 1, DIM);
        }

        // Справа: чего не хватает до полного каталога, от дешёвого к дорогому.
        c.fill_rect(mid - 2, by + 14, 2, bh - 28, PANEL_HI);
        let missing = w.catalog_missing();
        let rx = mid + 18;
        let head = format!("ДО ПОЛНОГО КАТАЛОГА НЕ ХВАТАЕТ: {} ИЗ {}", missing.len(), N_ITEM_IDS);
        c.text(rx, by + 16, &head, 2, GOLD);
        let by_p: Vec<usize> = (1..=3u8).map(|p| missing.iter().filter(|m| m.purity == p).count()).collect();
        let stone = (1..=3u8).filter(|p| w.discovered[Item::new(15, *p, FORM_STONE).id()]).count();
        let sum = format!("ЧИСТОТА I: {}  II: {}  III: {}  КАМЕНЬ: {}/3", by_p[0], by_p[1], by_p[2], stone);
        c.text(rx, by + 40, &sum, 2, TEXT);
        c.text(rx, by + 66, "ПОЗИЦИЯ (КОЛЕСО - ЛИСТАТЬ)", 1, DIM);
        c.text(bx + bw - 250, by + 66, "ГДЕ ДЕЛАЮТ", 1, DIM);
        c.text(bx + bw - 96, by + 66, "ПРЕМИЯ", 1, DIM);
        let top = (self.stats_scroll.max(0) as usize).min(missing.len().saturating_sub(rows));
        for (i, it) in missing.iter().skip(top).take(rows).enumerate() {
            let y = by + 80 + i as i32 * 24;
            c.blit(&self.assets.items[it.id()], rx, y, 2);
            c.text(rx + 28, y + 3, &it.name(), 2, TEXT);
            let machine = recipe_steps(*it).last().map_or(String::new(), |s| s.machine.to_string());
            c.text(bx + bw - 250, y + 7, &machine, 1, DIM);
            c.text(bx + bw - 96, y + 3, &format!("${}", fmt_money(it.first_sale_bonus())), 2, GOLD);
        }
        if missing.is_empty() {
            c.text(rx, by + 84, "КАТАЛОГ СОБРАН ПОЛНОСТЬЮ!", 2, GREEN);
        } else {
            let last = (top + rows).min(missing.len());
            c.text(rx, by + bh - 24, &format!("ПОКАЗАНЫ {}-{} ИЗ {}. ЩЕЛЧОК ИЛИ T - ЗАКРЫТЬ", top + 1, last, missing.len()), 1, DIM);
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
                let x = c.text(p.0 + 20, p.1 + 166, &format!("{} (ТЫ)", self.net.my_name), 2, TEXT);
                if let (Mode::Client, Some(ms)) = (self.net.mode, self.net.ping_ms) {
                    c.text(x + 20, p.1 + 166, &format!("ПИНГ ДО ХОСТА {:.0} МС", ms), 2, ping_color(ms));
                }
                for (i, pl) in self.net.peers.iter().enumerate() {
                    let y = p.1 + 194 + i as i32 * 28;
                    let x = c.text(p.0 + 20, y, &pl.name, 2, TEXT);
                    if let Some(ms) = pl.ping_ms {
                        c.text(x + 20, y, &format!("{:.0} МС", ms), 2, ping_color(ms));
                    }
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

    /// Счётчик кадров в левом нижнем углу: зелёный от 55 FPS, жёлтый от 30, ниже - красный.
    /// В сетевой игре рядом пинг: зелёный до 80 мс, жёлтый до 200, дальше красный.
    fn draw_fps(&self, c: &mut Image) {
        let s = format!("{:.0} FPS  {:.1} МС", self.fps, self.frame_ms);
        let col = if self.fps >= 55.0 {
            GREEN
        } else if self.fps >= 30.0 {
            GOLD
        } else {
            RED
        };
        let ping = self.net.online().then(|| match self.net.ping() {
            Some(ms) => (format!("ПИНГ {:.0} МС", ms), ping_color(ms)),
            None => ("ПИНГ ...".to_string(), DIM),
        });
        // Пинг - строкой выше, чтобы плашка не залезала под панель построек.
        let rows = 1 + ping.is_some() as i32;
        let tw = text_w(&s, 2).max(ping.as_ref().map_or(0, |p| text_w(&p.0, 2)));
        c.fill_rect(8, c.h - 8 - 28 * rows, tw + 16, 28 * rows, 0xE02B_2433);
        c.text(16, c.h - 29, &s, 2, col);
        if let Some((p, pc)) = ping {
            c.text(16, c.h - 57, &p, 2, pc);
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
            "1-9, 0         ВЫБОР ПОСТРОЙКИ НА ВКЛАДКЕ",
            "МЫШЬ 4 / 5     ПРЕДЫДУЩАЯ / СЛЕДУЮЩАЯ ПОСТРОЙКА",
            "ЛКМ            СТРОИТЬ (КОНВЕЙЕР - ТЯНУТЬ МЫШЬЮ)",
            "ПКМ            СНЕСТИ (ДЕНЬГИ ВОЗВРАЩАЮТСЯ)",
            "R              ПОВЕРНУТЬ ПОСТРОЙКУ",
            "Q              ВЗЯТЬ ПОСТРОЙКУ ПОД КУРСОРОМ",
            "U              УЛУЧШИТЬ МАШИНУ ПОД КУРСОРОМ",
            "ЛКМ ПО КРАЮ    СТОРОНА МАШИНЫ: ВХОД / ВЫХОД / ЗАКРЫТО",
            "E              НАСТРОЙКА: ОБРАЗЕЦ, ПРОПОРЦИЯ, КОД",
            "F              СБРОСИТЬ ОБРАЗЕЦ ФИЛЬТРА",
            "SHIFT + ЛКМ    ВЫДЕЛИТЬ РАМКОЙ",
            "DEL            СНЕСТИ ВЫДЕЛЕННОЕ",
            "CTRL+C / X / V КОПИЯ / ПЕРЕНОС / ВСТАВКА (R - ПОВОРОТ)",
            "P / L          ПУТЬ КОНТРАКТА / СЛОЙ ПРОВОДОВ ЛОГИКИ",
            "C / T          КАТАЛОГ ПРЕДМЕТОВ / СТАТИСТИКА ПРОДАЖ",
            "G / V          ГИЛЬДИИ И ЗАКАЗЫ / ВИТРАЖ И НОВЫЙ ВИТОК",
            "F2             СЕТЕВАЯ ИГРА ЧЕРЕЗ STEAM",
            "F3 / F11       СЧЁТЧИК FPS / ПОЛНЫЙ ЭКРАН",
            "Z              ПОДЗЕМЕЛЬЕ / ПОВЕРХНОСТЬ (СВЯЗЬ - ЛИФТЫ)",
            "WASD, КОЛЕСО   КАМЕРА, МАСШТАБ",
            "B              БИБЛИОТЕКА ЧЕРТЕЖЕЙ: БУФЕР ПОД ИМЕНЕМ",
            "M / F1         ЗВУК ВКЛ/ВЫКЛ / ЭТА СПРАВКА",
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
        // Шаг строк ужимается, чтобы справка целиком влезала в невысокое окно.
        let lh = ((c.h - 60) / lines.len() as i32).clamp(16, 22);
        let bh = lines.len() as i32 * lh + 40;
        let (bx, by) = ((c.w - bw) / 2, (c.h - bh) / 2);
        panel(c, (bx, by, bw, bh), GOLD);
        for (i, l) in lines.iter().enumerate() {
            let y = by + 22 + i as i32 * lh;
            if i == 0 || i == lines.len() - 1 {
                c.text_center(c.w / 2, y, l, 2, GOLD);
            } else {
                c.text(bx + 30, y, l, 2, TEXT);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn game() -> Game {
        let mut w = World::new(5);
        w.money = 1e7;
        w.level = FINAL_LEVEL;
        let mut g = Game::new(w, Audio::new(false), false);
        g.persist = false;
        g.update(&Input::default(), DT, 1280, 720);
        g
    }

    /// Настоящий щелчок: кадр с нажатой кнопкой и кадр с отпущенной, через общий разбор ввода.
    fn click_xy(g: &mut Game, mx: i32, my: i32) {
        g.update(&Input { mx, my, lmb: true, lmb_pressed: true, ..Default::default() }, DT, 1280, 720);
        g.update(&Input { mx, my, ..Default::default() }, DT, 1280, 720);
    }

    fn click(g: &mut Game, r: (i32, i32, i32, i32)) {
        click_xy(g, r.0 + r.2 / 2, r.1 + r.3 / 2);
    }

    fn key(g: &mut Game, k: Key, ctrl: bool) {
        g.update(&Input { mx: 640, my: 300, ctrl, pressed: vec![k], ..Default::default() }, DT, 1280, 720);
    }

    fn typed(g: &mut Game, s: &str) {
        g.update(&Input { typed: s.chars().collect(), ..Default::default() }, DT, 1280, 720);
    }

    /// Середина тайла на экране.
    fn tile_px(g: &Game, x: i32, y: i32) -> (i32, i32) {
        let (ox, oy) = g.origin();
        (ox + x * g.ts() + g.ts() / 2, oy + y * g.ts() + g.ts() / 2)
    }

    /// Две ленты и схема над ними: таймер, провод, счётчик, исполнитель - все с настройками.
    fn circuit(g: &mut Game) {
        let k = Kind::by_name;
        let w = &mut g.world;
        w.place(Kind(0), 10, 10, Dir::E).unwrap();
        w.place(Kind(0), 11, 10, Dir::E).unwrap();
        w.place(k("ТАЙМЕР"), 10, 10, Dir::S).unwrap();
        w.set_lparam(10, 10, 70);
        w.place(k("ПРОВОД"), 10, 11, Dir::N).unwrap();
        w.place(k("СЧЁТЧИК"), 11, 11, Dir::E).unwrap();
        w.set_lparam(11, 11, 5);
        w.place(k("ИСПОЛНИТЕЛЬ"), 11, 10, Dir::N).unwrap();
        w.set_lparam(11, 10, 1);
        g.sel = Some((10, 10, 11, 11));
    }

    fn node_at(g: &Game, x: i32, y: i32) -> (&'static str, Dir, u64) {
        let n = g.world.node(x, y).expect("деталь логики на месте");
        (n.kind.name(), n.dir, n.param)
    }

    #[test]
    fn copy_paste_carries_logic() {
        let mut g = game();
        circuit(&mut g);
        g.copy_selection(false);
        assert_eq!((g.clip.len(), g.clip_nodes.len(), g.clip_size), (2, 4, (2, 2)));
        assert!(g.pasting && g.wires_on(), "с деталями в буфере слой проводов виден сам");

        // Вставка переносит оба слоя, настройки деталей и берёт ровно объявленную цену.
        let (money, cost) = (g.world.money, g.clip_cost());
        assert_eq!(g.paste_at(20, 20), 6);
        assert!((money - g.world.money - cost).abs() < 1e-6, "цена вставки {} и {}", money - g.world.money, cost);
        assert_eq!(node_at(&g, 20, 20), ("ТАЙМЕР", Dir::S, 70));
        assert_eq!(node_at(&g, 20, 21).0, "ПРОВОД");
        assert_eq!(node_at(&g, 21, 21), ("СЧЁТЧИК", Dir::E, 5));
        assert_eq!(node_at(&g, 21, 20), ("ИСПОЛНИТЕЛЬ", Dir::N, 1));
        assert!(g.world.get(20, 20).is_some() && g.world.get(21, 20).is_some());

        // Поворот крутит и детали: таймер из левого верхнего угла уходит в правый верхний.
        g.rotate_clip();
        assert_eq!(g.paste_at(30, 30), 6);
        assert_eq!(node_at(&g, 31, 30), ("ТАЙМЕР", Dir::W, 70));
        assert_eq!(node_at(&g, 31, 31), ("ИСПОЛНИТЕЛЬ", Dir::E, 1));
        assert_eq!(node_at(&g, 30, 31), ("СЧЁТЧИК", Dir::S, 5));
        assert_eq!(g.world.get(31, 30).map(|b| b.dir), Some(Dir::S));
        // Четыре поворота возвращают буфер в исходный вид.
        let before = (g.clip.clone(), g.clip_nodes.clone());
        (0..4).for_each(|_| g.rotate_clip());
        assert_eq!(before, (g.clip.clone(), g.clip_nodes.clone()));

        // Перенос (CTRL+X) снимает и постройки, и детали, деньги возвращаются.
        let money = g.world.money;
        g.sel = Some((20, 20, 21, 21));
        g.copy_selection(true);
        assert!((20..22).all(|x| (20..22).all(|y| g.world.node(x, y).is_none() && g.world.get(x, y).is_none())));
        assert!((g.world.money - money - cost).abs() < 1e-6);

        // DEL при открытом слое проводов снимает детали, при закрытом - постройки.
        g.pasting = false;
        g.clip_nodes.clear();
        g.show_wires = true;
        g.sel = Some((30, 30, 31, 31));
        g.delete_selection();
        assert!(g.world.node(31, 30).is_none() && g.world.get(31, 30).is_some());
        g.show_wires = false;
        g.sel = Some((30, 30, 31, 31));
        g.delete_selection();
        assert!(g.world.get(31, 30).is_none() && g.world.get(31, 31).is_none());
    }

    #[test]
    fn paste_as_client_sends_logic_commands() {
        let mut g = game();
        circuit(&mut g);
        g.copy_selection(false);
        g.net.mode = Mode::Client;
        let money = g.world.money;
        // Клиент сам ничего не строит и не платит: он только просит хоста.
        assert_eq!(g.paste_at(60, 60), 6);
        assert!(g.world.node(60, 60).is_none() && g.world.get(60, 60).is_none());
        assert_eq!(g.world.money, money);
    }

    #[test]
    fn blueprint_library_saves_names_and_reloads() {
        let path = std::env::temp_dir().join(format!("alembic_bp_test_{}.txt", std::process::id()));
        let path = path.to_str().unwrap().to_string();
        let _ = std::fs::remove_file(&path);
        let mut g = game();
        g.open_library(&path);
        assert!(g.blueprints.is_empty());
        circuit(&mut g);
        g.copy_selection(false);

        // B открывает окно; буква самой клавиши в имя не попадает.
        g.update(&Input { pressed: vec![Key::B], typed: vec!['и'], ..Default::default() }, DT, 1280, 720);
        assert!(g.show_bp && g.bp_name.is_empty());
        // Пока окно открыто, буквы идут в имя, а не в горячие клавиши.
        g.update(&Input { pressed: vec![Key::G], typed: vec!['g'], ..Default::default() }, DT, 1280, 720);
        assert!(g.show_bp && !g.show_guild && g.bp_name == "G");
        key(&mut g, Key::Backspace, false);
        typed(&mut g, "  Ворота №1\t");
        assert_eq!(g.bp_name, "ВОРОТА 1", "строчные становятся прописными, знаки без глифа пропускаются");
        typed(&mut g, "0123456789012345678901234567890");
        assert_eq!(g.bp_name.chars().count(), BP_NAME_MAX);
        (0..BP_NAME_MAX - 8).for_each(|_| key(&mut g, Key::Backspace, false));
        key(&mut g, Key::Enter, false);
        assert_eq!(g.blueprints.len(), 1);
        assert_eq!((g.blueprints[0].name.as_str(), g.blueprints[0].items.len(), g.blueprints[0].nodes.len()), ("ВОРОТА 1", 2, 4));
        assert!(g.bp_name.is_empty() && g.show_bp);

        // Без имени чертёж получает номер; то же имя заменяет старый чертёж, а не плодит второй.
        { let r = g.bp_save_btn(); click(&mut g, r); }
        assert_eq!(g.blueprints[1].name, "ЧЕРТЁЖ 1");
        g.rotate_clip();
        g.clip.pop();
        typed(&mut g, "ворота 1");
        key(&mut g, Key::Enter, false);
        assert_eq!((g.blueprints.len(), g.blueprints[0].items.len()), (2, 1));

        // Файл читается другой игрой один в один.
        let text = std::fs::read_to_string(&path).expect("файл библиотеки записан");
        assert!(text.starts_with("ALEMBIC-BP1\nB 2 2 ВОРОТА 1\n"), "{}", text);
        let mut g2 = game();
        g2.open_library(&path);
        assert_eq!(g2.blueprints, g.blueprints);

        // Щелчок по строке берёт чертёж в руки; вставка ставит и постройки, и схему.
        g2.show_bp = true;
        { let r = g2.bp_row(1); click(&mut g2, r); }
        assert!(!g2.show_bp && g2.pasting);
        assert_eq!((g2.clip.len(), g2.clip_nodes.len(), g2.clip_size), (2, 4, (2, 2)));
        assert_eq!(g2.paste_at(50, 50), 6);
        assert_eq!(node_at(&g2, 51, 51), ("СЧЁТЧИК", Dir::E, 5));

        // Удаление - в два щелчка; щелчок в другое место снимает подтверждение.
        g2.pasting = false;
        g2.show_bp = true;
        { let r = g2.bp_del_btn(0); click(&mut g2, r); }
        assert_eq!((g2.blueprints.len(), g2.bp_del_arm), (2, Some(0)));
        { let r = g2.bp_name_rect(); click(&mut g2, r); }
        assert_eq!(g2.bp_del_arm, None);
        { let r = g2.bp_del_btn(0); click(&mut g2, r); }
        { let r = g2.bp_del_btn(0); click(&mut g2, r); }
        assert_eq!(g2.blueprints.len(), 1);
        assert_eq!(g2.blueprints[0].name, "ЧЕРТЁЖ 1");
        assert_eq!(bp_from_text(&std::fs::read_to_string(&path).unwrap()), g2.blueprints);
        assert!(g2.show_bp);
        click_xy(&mut g2, 3, 3);
        assert!(!g2.show_bp, "щелчок мимо окна закрывает его");
        g2.show_bp = true;
        key(&mut g2, Key::Escape, false);
        assert!(!g2.show_bp);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn blueprint_file_survives_garbage() {
        assert!(bp_from_text("").is_empty());
        assert!(bp_from_text("мусор\nB 2 2 ИМЯ\nT 0 0 0 1 1 0 0\n").is_empty(), "без заголовка файл не читается");
        let text = "ALEMBIC-BP1\nT 0 0 0 1 1 0 0\nB 2 2 ИМЯ\nT 5 5 0 1 1 0 0\nT 0 0 250 1 1 0 0\nN 0 0 0 1 5\n\
                    T 1 1 0 9 99 0 0\nT 1 1 0 0 1 0 0\nN 0 1 59 2 99999999\nчушь\nB x y z\nT 0 0 0 0 1 0 0\nB 1 1 ПУСТОЙ\nB 3 1 ИМЯ\nT 2 0 0 0 1 0 0\n";
        let list = bp_from_text(text);
        // Клетка за рамкой, неизвестная постройка, лента в слое логики и строки после битого
        // заголовка отброшены; второй чертёж с тем же именем заменил первый.
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].name.as_str(), list[0].size, list[0].items.len(), list[0].nodes.len()), ("ИМЯ", (3, 1), 1, 0));
        let first = bp_from_text(&text[..text.find("B x y z").unwrap()]);
        assert_eq!((first[0].items.len(), first[0].nodes.len()), (1, 1));
        assert_eq!((first[0].items[0].dir, first[0].items[0].level), (Dir::from_u8(1), 1), "направление и уровень приведены к допустимым");
        assert_eq!(first[0].nodes[0].param, 9999, "настройка детали обрезана до допустимой");
        assert_eq!(bp_from_text(&bp_to_text(&first)), first);
    }

    /// Z переключает слой: камера остаётся над той же клеткой, мышь попадает в тайлы подземелья,
    /// порода сносится ПКМ за деньги, лифты ставятся парой на одной клетке двух слоёв.
    #[test]
    fn dungeon_view_switches_and_builds() {
        let mut g = game();
        g.cam_x = 20.5;
        g.cam_y = 20.5;
        g.update(&Input::default(), DT, 1280, 720);
        assert_eq!(g.tile_at(640, 360), (20, 20));
        key(&mut g, Key::Z, false);
        assert_eq!((g.layer, g.tile_at(640, 360)), (1, (20, 20 + DUNGEON_Y)));
        let i = ((20 + DUNGEON_Y) * W + 20) as usize;
        g.world.rocks[i] = true;
        g.world.veins[i] = 0;
        let lift = Kind::by_name("ЛИФТ");

        // В породу лифт не встаёт.
        g.tool = Some(lift);
        click_xy(&mut g, 640, 360);
        assert!(g.world.get(20, 20 + DUNGEON_Y).is_none());
        // ПКМ без постройки в руках сносит породу за ROCK_COST.
        g.tool = None;
        let money = g.world.money;
        g.update(&Input { mx: 640, my: 360, rmb: true, rmb_pressed: true, ..Default::default() }, DT, 1280, 720);
        g.update(&Input { mx: 640, my: 360, ..Default::default() }, DT, 1280, 720);
        assert!(!g.world.rocks[i] && g.world.money == money - ROCK_COST);
        g.tool = Some(lift);
        click_xy(&mut g, 640, 360);
        assert_eq!(g.world.get(20, 20 + DUNGEON_Y).map(|b| b.kind), Some(lift));
        assert!(g.world.get(20, 20).is_none(), "на поверхности пока пусто");

        // Оба слоя рисуются без сбоев: и с лифтом без пары, и с призраком пары наверху.
        let mut c = Image::new(1280, 720);
        g.draw(&mut c);
        key(&mut g, Key::Z, false);
        assert_eq!((g.layer, g.tile_at(640, 360)), (0, (20, 20)));
        g.draw(&mut c);
        g.tool = Some(lift);
        click_xy(&mut g, 640, 360);
        assert_eq!(g.world.get(20, 20).map(|b| b.kind), Some(lift));
        g.draw(&mut c);

        // Буфер обмена переносится между слоями: скопировано наверху, вставлено внизу.
        g.world.place(Kind(0), 21, 20, Dir::E).unwrap();
        g.sel = Some((21, 20, 21, 20));
        g.copy_selection(false);
        key(&mut g, Key::Z, false);
        assert!(g.pasting && g.sel.is_none());
        g.world.rocks[i + 3] = false;
        assert_eq!(g.paste_at(23, 20 + DUNGEON_Y), 1);
        assert!(g.world.get(23, 20 + DUNGEON_Y).is_some());
    }

    #[test]
    fn guild_window_clicks() {
        let mut g = game();
        key(&mut g, Key::G, false);
        assert!(g.show_guild && g.world.offers.len() >= 3);
        // Скин закрыт, пока репутации не хватает.
        { let r = g.skin_btn(2); click(&mut g, r); }
        assert_eq!(g.world.skin, 0);
        g.world.rep[2] = REP_SKIN;
        { let r = g.skin_btn(2); click(&mut g, r); }
        assert_eq!(g.world.skin, 3);
        { let r = g.skin_btn(2); click(&mut g, r); }
        assert_eq!(g.world.skin, 0, "повторный щелчок снимает скин");
        // Заказ берётся щелчком по кнопке карточки; второй заказ поверх первого не берётся.
        let want = g.world.offers[1];
        { let r = g.offer_btn(1); click(&mut g, r); }
        let got = g.world.order.expect("заказ взят").0;
        assert_eq!((got.guild, got.item, got.need), (want.guild, want.item, want.need));
        { let r = g.offer_btn(0); click(&mut g, r); }
        assert_eq!(g.world.order.unwrap().0.need, want.need);
        // Щелчок по самой карточке мимо кнопки ничего не делает; мимо окна - закрывает.
        let c = g.guild_card(0);
        click_xy(&mut g, c.0 + 20, c.1 + 20);
        assert!(g.show_guild);
        click_xy(&mut g, 3, 3);
        assert!(!g.show_guild);
    }

    #[test]
    fn great_work_window_clicks_and_ctrl_v() {
        let mut g = game();
        circuit(&mut g);
        key(&mut g, Key::V, false);
        assert!(g.show_great);
        // Без камня виток не начинается.
        { let r = g.prestige_btn(); click(&mut g, r); }
        { let r = g.prestige_btn(); click(&mut g, r); }
        assert_eq!(g.world.cycle, 0);
        // С камнем: первый щелчок взводит кнопку, щелчок в сторону снимает, два подряд - виток.
        g.world.stones_now = 2;
        { let r = g.prestige_btn(); click(&mut g, r); }
        assert!(g.prestige_arm && g.world.cycle == 0);
        let p = g.wide_panel();
        click_xy(&mut g, p.0 + 30, p.1 + 30);
        assert!(!g.prestige_arm && g.show_great);
        { let r = g.prestige_btn(); click(&mut g, r); }
        { let r = g.prestige_btn(); click(&mut g, r); }
        assert_eq!((g.world.cycle, g.world.stones, g.world.stones_now), (1, 2, 0));
        assert!(!g.show_great && g.world.get(10, 10).is_none() && g.world.node(10, 10).is_none());

        // CTRL+V - это вставка, а не витраж.
        g.world.money = 1e7;
        g.world.level = FINAL_LEVEL;
        circuit(&mut g);
        g.copy_selection(false);
        g.pasting = false;
        key(&mut g, Key::V, true);
        assert!(g.pasting && !g.show_great);
    }

    #[test]
    fn node_settings_window_clicks() {
        let mut g = game();
        let k = Kind::by_name;
        g.cam_x = 12.0;
        g.cam_y = 12.0;
        for (name, x) in [("СЧЁТЧИК", 10), ("ИСПОЛНИТЕЛЬ", 11), ("ТАЙМЕР", 12), ("ДАТЧИК ПРЕДМЕТА", 13), ("ДАТЧИК СКЛАДА", 14), ("ЗАЩЁЛКА", 15)] {
            g.world.place(k(name), x, 12, Dir::E).unwrap();
        }
        g.show_wires = true;
        let open = |g: &mut Game, x: i32| {
            let (mx, my) = tile_px(g, x, 12);
            g.update(&Input { mx, my, pressed: vec![Key::E], ..Default::default() }, DT, 1280, 720);
        };
        let param = |g: &Game, x: i32| g.world.node(x, 12).unwrap().param;
        let btn = |g: &Game, i: usize| g.node_buttons()[i].r;

        // Счётчик: шесть кнопок -100 -10 -1 +1 +10 +100, значение не опускается ниже 1.
        open(&mut g, 10);
        assert!(g.show_cfg == Some((10, 12)) && g.cfg_node);
        assert_eq!(g.node_buttons().len(), 6);
        { let r = btn(&g, 4); click(&mut g, r); }
        assert_eq!(param(&g, 10), 18);
        { let r = btn(&g, 2); click(&mut g, r); }
        assert_eq!(param(&g, 10), 17);
        { let r = btn(&g, 0); click(&mut g, r); }
        assert_eq!(param(&g, 10), 1);
        let r = g.node_rect();
        click_xy(&mut g, r.0 + 4, r.1 + 4);
        assert!(g.show_cfg.is_some(), "щелчок по окну мимо кнопок его не закрывает");
        click_xy(&mut g, 3, 3);
        assert!(g.show_cfg.is_none());

        // Исполнитель: две кнопки режима.
        open(&mut g, 11);
        { let r = btn(&g, 1); click(&mut g, r); }
        assert_eq!(param(&g, 11), 1);
        { let r = btn(&g, 0); click(&mut g, r); }
        assert_eq!(param(&g, 11), 0);
        key(&mut g, Key::Escape, false);
        assert!(g.show_cfg.is_none());

        // Таймер: период в десятых секунды, не короче 0.2 с.
        open(&mut g, 12);
        { let r = btn(&g, 5); click(&mut g, r); }
        assert_eq!(param(&g, 12), 120);
        { let r = btn(&g, 0); click(&mut g, r); }
        { let r = btn(&g, 0); click(&mut g, r); }
        assert_eq!(param(&g, 12), 2);
        click_xy(&mut g, 3, 3);

        // Датчик склада: порог не выше 100%.
        open(&mut g, 14);
        { let r = btn(&g, 5); click(&mut g, r); }
        assert_eq!(param(&g, 14), 100);
        { let r = btn(&g, 1); click(&mut g, r); }
        assert_eq!(param(&g, 14), 90);
        click_xy(&mut g, 3, 3);

        // Датчик предмета: каждая кнопка условия меняет условие и подсвечивается.
        open(&mut g, 13);
        let n = g.node_buttons().len();
        assert!(n > 6, "кнопок условия {}", n);
        let mut seen = std::collections::HashSet::new();
        for i in 0..n {
            { let r = btn(&g, i); click(&mut g, r); }
            assert!(g.node_buttons()[i].on, "кнопка {} включилась", i);
            seen.insert(param(&g, 13));
        }
        assert!(seen.len() > n / 2, "условие меняется: {} разных значений на {} кнопок", seen.len(), n);
        click_xy(&mut g, 3, 3);

        // У защёлки настроек нет: окно не открывается.
        open(&mut g, 15);
        assert!(g.show_cfg.is_none());
    }
}
