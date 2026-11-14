//! Скрытые режимы для сохранения "мегафабрика 3": фабрика, которая одновременно производит
//! и продаёт в лавке все 1170 предметов (15 смесей x 26 форм x 3 чистоты) без торговых постов.
//!
//! `--gen-megasave3 <файл>`     собрать фабрику и записать сохранение;
//! `--verify-megasave3 <файл>`  проверить сохранение без окна;
//! `--mega3-lab`                один модуль на стенде: сколько чего он продаёт.
//!
//! Устройство. На каждую из 15 смесей - один модуль 22x12. Слева в него входят стихии,
//! смесители собирают смесь чистоты I. Дальше "узел чистоты": малая доля потока идёт дальше
//! как есть, остальное перегоняется в II; малая доля II идёт дальше, остальное - в III.
//! Три чистоты сливаются в одну ленту и расходятся по машинам. Машины переработки сохраняют
//! чистоту, сборочные дают минимум чистоты входов - поэтому из одного модуля выходят все три
//! чистоты каждой формы (поток в основном III, иначе глубокие сборки не бывали бы чистыми).
//! Всё, что раньше уходило в торговые посты, собирают ленты-стоки и везут в лавку.

use crate::world::*;
use std::collections::VecDeque;

pub const MW: i32 = 22;
pub const MH: i32 = 12;
/// Ширина левой части модуля: 3 колонки смесителей и 5 колонок узла чистоты.
const LW: i32 = 8;

/// Машинная часть модуля (14x12). Клетка: `.` пусто, `> < ^ v` экспресс-лента,
/// `S:выходы` делитель, `N:выходы` машина формы N, `tX` вход подземного конвейера,
/// `TX` его выход. Входы машин выводятся сами: сторона открыта на приём, если сосед в неё выдаёт.
const CORE: [&str; MH as usize] = [
    ">    >     >     >     >     >    >    >    >     >     >     >     >    v",
    "^    .     Tn    .     .     .    .    .    .     .     .     .     .    v",
    "^    14:e  >     >     21:e  S:es 25:s S:ws 8:w   <     7:w   <     .    v",
    "^    5:ne  tn    12:ew 16:ne v    <    <    6:nw  <     5:n   4:n   >    v",
    "^    1:wne 3:en  4:n   9:ne  ts   .    ^    2:nw  3:n   1:wne 3:ne  15:n v",
    "S:es S:en  >     >     S:en  >    >    >    S:en  >     S:en  ts    .    v",
    ">    S:es  >     >     >     S:es >    >    S:es  >     >     >     S:s  v",
    "5:s  1:we  3:es  15:e  19:s  10:we v   5:s  1:we  3:es  11:es 20:s  2:w  v",
    "14:es 17:s 4:wes 13:s  >     S:se v    >    7:s   4:wse v     >     >    v",
    "v    S:sw  12:e  18:es S:es  23:e v    v    S:sw  13:se v     Ts    .    v",
    "v    >     >     22:e  v     Ts   v    v    24:w  <     v     v     .    v",
    ">    >     >     >     >     >    >    >    >     >     >     >     >    >",
];

#[derive(Clone, PartialEq, Debug)]
pub enum Cell {
    Empty,
    /// Утилизатор.
    Trash,
    Belt(Dir),
    /// Машина, смеситель, дистиллятор или делитель: вид постройки и стороны выдачи.
    Mach(u8, Vec<Dir>),
    /// Переключатель: уровень (период), куда смотрит, выходы.
    Sw(u8, Dir, Vec<Dir>),
    TunIn(Dir),
    TunOut(Dir),
}

const DIRS: [Dir; 4] = [Dir::N, Dir::E, Dir::S, Dir::W];

fn kind(name: &str) -> u8 {
    Kind::by_name(name).0
}

fn kind_for_form(form: u8) -> u8 {
    KINDS
        .iter()
        .position(|d| match d.role {
            Role::Convert(_, to, _) => to == form,
            Role::Assemble(_, _, to, _) => to == form,
            _ => false,
        })
        .expect("форма без машины") as u8
}

fn dir_of(c: char) -> Dir {
    match c {
        'n' | '^' => Dir::N,
        'e' | '>' => Dir::E,
        's' | 'v' => Dir::S,
        'w' | '<' => Dir::W,
        _ => panic!("плохое направление {}", c),
    }
}

fn parse_cell(t: &str) -> Cell {
    match t {
"." => Cell::Empty,
        "X" => Cell::Trash,
        ">" | "<" | "^" | "v" => Cell::Belt(dir_of(t.chars().next().unwrap())),
        _ => {
            if let Some((a, b)) = t.split_once(':') {
                if let Some(l) = a.strip_prefix('W') {
                    let mut ch = l.chars();
                    let level = ch.next().and_then(|c| c.to_digit(10)).expect("уровень переключателя") as u8;
                    return Cell::Sw(level, dir_of(ch.next().expect("куда смотрит")), b.chars().map(dir_of).collect());
                }
                let k = match a {
                    "S" => kind("ДЕЛИТЕЛЬ"),
                    "F" => kind("ФИЛЬТР"),
                    "M" => kind("СМЕСИТЕЛЬ"),
                    "D" => kind("ДИСТИЛЛЯТОР"),
                    f => kind_for_form(f.parse().expect("форма")),
                };
                Cell::Mach(k, b.chars().map(dir_of).collect())
            } else {
                let mut ch = t.chars();
                match (ch.next(), ch.next()) {
                    (Some('t'), Some(d)) => Cell::TunIn(dir_of(d)),
                    (Some('T'), Some(d)) => Cell::TunOut(dir_of(d)),
                    _ => panic!("клетка чертежа {}", t),
                }
            }
        }
    }
}

/// Прямоугольный чертёж.
#[derive(Clone)]
pub struct Grid {
    pub w: i32,
    pub h: i32,
    pub cells: Vec<Cell>,
    /// Клетки, в которые предметы приходят снаружи чертежа: (x, y, с какой стороны).
    pub ext: Vec<(i32, i32, Dir)>,
}

impl Grid {
    fn new(w: i32, h: i32) -> Grid {
        Grid { w, h, cells: vec![Cell::Empty; (w * h) as usize], ext: Vec::new() }
    }
    fn at(&self, x: i32, y: i32) -> &Cell {
        const E: Cell = Cell::Empty;
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            &E
        } else {
            &self.cells[(y * self.w + x) as usize]
        }
    }
    fn set(&mut self, x: i32, y: i32, c: Cell) {
        assert!(*self.at(x, y) == Cell::Empty, "клетка чертежа ({}, {}) занята", x, y);
        self.cells[(y * self.w + x) as usize] = c;
    }
    fn tok(&mut self, x: i32, y: i32, t: &str) {
        self.set(x, y, parse_cell(t));
    }
    /// Выдаёт ли сосед клетки (x, y) со стороны `side` предметы в неё.
    fn fed_from(&self, x: i32, y: i32, side: Dir) -> bool {
        if self.ext.contains(&(x, y, side)) {
            return true;
        }
        match self.at(x + side.dx(), y + side.dy()) {
            Cell::Belt(d) | Cell::TunOut(d) => *d == side.opp(),
            Cell::Mach(_, outs) | Cell::Sw(_, _, outs) => outs.contains(&side.opp()),
            _ => false,
        }
    }
}

/// Модуль для смеси `mask` и строки, по которым в него слева входят стихии.
pub fn build_module(mask: u8) -> (Grid, Vec<(u8, i32)>) {
    let mut g = Grid::new(MW, MH);
    for (y, row) in CORE.iter().enumerate() {
        let toks: Vec<&str> = row.split_whitespace().collect();
        assert_eq!(toks.len() as i32, MW - LW, "строка {} чертежа", y);
        for (x, t) in toks.iter().enumerate() {
            g.tok(LW + x as i32, y as i32, t);
        }
    }
    let elems: Vec<u8> = (0..4).filter(|e| mask & (1 << e) != 0).collect();
    let k = elems.len() as i32;
    // Линия стихии: колонки 0..2, три строки. Переключатель (период 240 с) два такта из трёх
    // шлёт сырьё в дистиллятор (I -> II), на третий - в обход, как есть.
    for j in 0..k {
        let y = 3 * j + 1;
        g.tok(0, y, "W5e:esn");
        g.ext.push((0, y, Dir::W));
        g.tok(1, y, "D:e");
        g.tok(2, y, ">");
        g.tok(0, y + 1, ">");
        g.tok(1, y + 1, "^");
        g.tok(0, y - 1, ">");
        g.tok(1, y - 1, ">");
        g.tok(2, y - 1, "v");
    }
    // Смесители: колонки 3..4, итог выходит из (4,5) на восток.
    match k {
        1 => g.tok(3, 1, ">"),
        _ => {
            g.tok(3, 1, "v");
            g.tok(3, 2, "v");
            g.tok(3, 3, "M:e");
            g.tok(3, 4, "^");
        }
    }
    for y in (if k == 1 { 1 } else { 3 })..5 {
        g.tok(4, y, "v");
    }
    match k {
        1 | 2 => g.tok(4, 5, ">"),
        3 => {
            g.tok(3, 7, ">");
            g.tok(4, 7, "^");
            g.tok(4, 6, "^");
            g.tok(4, 5, "M:e");
        }
        _ => {
            g.tok(3, 7, "v");
            g.tok(3, 8, "M:e");
            g.tok(3, 9, "^");
            g.tok(3, 10, "^");
            g.tok(4, 8, "^");
            g.tok(4, 7, "^");
            g.tok(4, 6, "^");
            g.tok(4, 5, "M:e");
        }
    }
    // Главный узел: колонки 5..7. Переключатель M (240 с) два такта шлёт смесь во вторую
    // перегонку, третий - в переключатель N (120 с): такт перегонки в отдельном дистилляторе,
    // такт "как есть" (половина в утилизатор, чтобы поток в лавку не удваивался).
    // Программа на 720 с: 0-480 III (II из линий + перегонка), 480-600 II (I из обхода +
    // перегонка), 600-720 I. Длинная непрерывная III-фаза нужна глубоким сборкам: в её начале
    // они доедают остатки прошлой чистоты. Фазам II и I такие остатки не мешают.
    //
    // Дистиллятор берёт только пару одинаковых эссенций: одиночный остаток чужой чистоты
    // запер бы его навсегда. Поэтому там, где на стыке фаз меняется чистота сырья, перед
    // перегонкой стоит фильтр. Образец фильтра - первый предмет после загрузки (в сохранение
    // он не пишется, но после загрузки ленты пусты и время идёт с нуля, так что он всегда
    // один и тот же): у первого фильтра это II из линий, у второго - остатки той же II.
    g.tok(5, 5, "W5e:esn");
    g.tok(6, 5, "F:se");
    g.tok(5, 6, ">");
    g.tok(6, 6, "D:e");
    g.tok(7, 6, "^");
    g.tok(7, 5, ">");
    g.tok(5, 4, "W4e:en");
    // Такт II. Первыми сюда доезжают остатки II из линий - они и становятся образцом
    // второго фильтра и идут мимо перегонки; сырьё I фильтр отдаёт вбок, в дистиллятор.
    g.tok(6, 4, "F:en");
    g.tok(6, 3, "D:e");
    // Такт I: половина в утилизатор.
    g.tok(5, 3, "^");
    g.tok(5, 2, "S:en");
    g.tok(5, 1, "X");
    g.tok(6, 2, ">");
    for y in 2..5 {
        g.tok(7, y, "v");
    }
    let rows: Vec<i32> = (0..k).map(|j| 3 * j + 1).collect();
    (g, elems.into_iter().zip(rows).collect())
}

fn put(w: &mut World, k: u8, x: i32, y: i32, d: Dir) {
    assert!(w.get(x, y).is_none(), "клетка ({}, {}) уже занята", x, y);
    w.place(Kind(k), x, y, d).expect("постройка");
    w.force_level(x, y, MAX_LEVEL);
}

/// Задаёт стороны постройки по сторонам света; не названные закрыты.
fn ports_abs(w: &mut World, x: i32, y: i32, modes: &[(Dir, u8)]) {
    let facing = w.get(x, y).expect("постройка").dir;
    let mut p = [PORT_OFF; 4];
    for (d, m) in modes {
        p[rel_side(facing, *d)] = *m;
    }
    assert!(w.set_ports(x, y, pack_ports(p)));
}

/// Ставит чертёж в мир; `mirror` отражает его слева направо.
pub fn stamp(w: &mut World, g: &Grid, x0: i32, y0: i32, mirror: bool) {
    let md = |d: Dir| if mirror && d.dx() != 0 { d.opp() } else { d };
    let belt = kind("ЭКСПРЕСС-ЛЕНТА");
    let tun = kind("ПОДЗЕМНЫЙ КОНВЕЙЕР");
    let pos = |cx: i32, cy: i32| (if mirror { x0 + g.w - 1 - cx } else { x0 + cx }, y0 + cy);
    for cy in 0..g.h {
        for cx in 0..g.w {
            let (x, y) = pos(cx, cy);
            match g.at(cx, cy) {
                Cell::Empty => {}
                Cell::Trash => put(w, kind("УТИЛИЗАТОР"), x, y, Dir::N),
                Cell::Belt(d) => put(w, belt, x, y, md(*d)),
                Cell::TunIn(d) | Cell::TunOut(d) => put(w, tun, x, y, md(*d)),
                Cell::Sw(level, f, outs) => {
                    // Фазы идут в порядке сторон: перёд, право, лево. Отражение поменяло бы
                    // этот порядок, поэтому модули с переключателями не отражаются.
                    assert!(!mirror, "переключатели нельзя отражать");
                    put(w, kind("ПЕРЕКЛЮЧАТЕЛЬ"), x, y, *f);
                    w.force_level(x, y, *level);
                    let mut modes: Vec<(Dir, u8)> = outs.iter().map(|d| (md(*d), PORT_OUT)).collect();
                    for d in DIRS {
                        if !outs.contains(&d) && g.fed_from(cx, cy, d) {
                            modes.push((md(d), PORT_IN));
                        }
                    }
                    ports_abs(w, x, y, &modes);
                }
                Cell::Mach(k, outs) => {
                    put(w, *k, x, y, md(outs[0]));
                    let mut modes = Vec::new();
                    for d in DIRS {
                        if outs.contains(&d) {
                            modes.push((md(d), PORT_OUT));
                        } else if g.fed_from(cx, cy, d) {
                            modes.push((md(d), PORT_IN));
                        }
                    }
                    ports_abs(w, x, y, &modes);
                }
            }
        }
    }
    // Подземные пары должны сложиться так, как нарисовано.
    for cy in 0..g.h {
        for cx in 0..g.w {
            let (x, y) = pos(cx, cy);
            match g.at(cx, cy) {
                Cell::TunIn(d) => {
                    let e = w.tunnel_exit(x, y).expect("у входа под землю нет выхода");
                    let (ex, ey) = (e.0 - x0, e.1 - y0);
                    let ex = if mirror { g.w - 1 - ex } else { ex };
                    assert!(*g.at(ex, ey) == Cell::TunOut(*d), "вход ({}, {}) спарился не с тем выходом", cx, cy);
                }
                Cell::TunOut(_) => assert!(w.tunnel_is_exit(x, y), "выход ({}, {}) не выход", cx, cy),
                _ => {}
            }
        }
    }
}

fn new_world(seed: u64) -> World {
    let mut w = World::new(seed);
    w.money = 1e15;
    w.level = FINAL_LEVEL;
    w.upgrades = UPGRADE_MAX;
    w
}

fn run_secs(w: &mut World, secs: u32, fps: u32) {
    for _ in 0..secs * fps {
        w.update(1.0 / fps as f32);
    }
    w.pending_new.clear();
}

fn arg_num(args: &[String], name: &str, default: i32) -> i32 {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// Стенд: один модуль с искусственными жилами, сток ведёт прямо в лавку.
pub fn lab(args: &[String]) {
    let mask = arg_num(args, "--mask", 15) as u8;
    let drill = arg_num(args, "--drill", 5) as u8;
    let mins = arg_num(args, "--mins", 15) as u32;
    let fps = arg_num(args, "--fps", 60) as u32;
    let mut w = new_world(1);
    w.veins.iter_mut().for_each(|v| *v = 0);
    let (g, taps) = build_module(mask);
    let (x0, y0) = (10, 20);
    stamp(&mut w, &g, x0, y0, false);
    let belt = kind("ЭКСПРЕСС-ЛЕНТА");
    for (e, r) in taps {
        w.veins[((y0 + r) * W + x0 - 2) as usize] = e + 1;
        put(&mut w, kind("ГЛУБИННЫЙ БУР"), x0 - 2, y0 + r, Dir::E);
        w.force_level(x0 - 2, y0 + r, drill);
        put(&mut w, belt, x0 - 1, y0 + r, Dir::E);
    }
    // Сток: вправо до x=38, вниз до лавки.
    let ey = y0 + MH - 1;
    for x in x0 + MW..38 {
        put(&mut w, belt, x, ey, Dir::E);
    }
    for y in ey..39 {
        put(&mut w, belt, 38, y, Dir::S);
    }
    put(&mut w, belt, 38, 39, Dir::E);
    let _ = mins;
    let mut wins: Vec<Vec<u32>> = Vec::new();
    let mut t0 = w.total_sold;
    for (name, secs) in [("0-20 мин", 1200u32), ("20-35 мин", 900), ("35-50 мин", 900)] {
        w.sold_by_id.iter_mut().for_each(|c| *c = 0);
        run_secs(&mut w, secs, fps);
        println!("смесь {} бур ур.{} окно {}: продано {} шт ({:.1}/с)", mask, drill, name, w.total_sold - t0, (w.total_sold - t0) as f64 / secs as f64);
        t0 = w.total_sold;
        wins.push((0..N_FORMS as u8).flat_map(|f| (1..=3).map(move |p| (f, p))).map(|(f, p)| w.sold_by_id[Item::new(mask, p, f).id()]).collect());
    }
    for f in 0..N_FORMS {
        let c: Vec<String> = wins.iter().map(|v| format!("{:>5} {:>5} {:>5}", v[f * 3], v[f * 3 + 1], v[f * 3 + 2])).collect();
        println!("  {:>2} {:<12} {}", f, FORM_NAMES[f], c.join("  |"));
    }
    for (i, v) in wins.iter().enumerate() {
        println!("  окно {}: нулевых позиций {}, минимум {}", i, v.iter().filter(|c| **c == 0).count(), v.iter().min().unwrap());
    }
}

// ====================== вся фабрика ======================

const COL_X: [i32; 3] = [4, 31, 57];
const ROW_Y: [i32; 6] = [2, 14, 26, 43, 55, 67];

fn belt_line(w: &mut World, cells: &[(i32, i32, Dir)]) {
    let belt = kind("ЭКСПРЕСС-ЛЕНТА");
    for &(x, y, d) in cells {
        put(w, belt, x, y, d);
    }
}

/// Магистраль из колонки модулей в лавку: вдоль улицы `lx` от строки `from` до строки
/// поворота `turn`, по ней до лавки.
fn lane(w: &mut World, lx: i32, from: i32, turn: i32) {
    let mut c = Vec::new();
    let down = turn > from;
    let mut y = from;
    while y != turn {
        c.push((lx, y, if down { Dir::S } else { Dir::N }));
        y += if down { 1 } else { -1 };
    }
    if lx < HUB_X {
        for x in lx..HUB_X {
            c.push((x, turn, Dir::E));
        }
    } else {
        // Строки над и под лавкой заходят в неё сверху и снизу, остальные - сбоку.
        let side = turn == HUB_Y - 1 || turn == HUB_Y + 3;
        let end = if side { HUB_X + 2 } else { HUB_X + 3 };
        let mut x = lx;
        while x > end {
            c.push((x, turn, Dir::W));
            x -= 1;
        }
        c.push((end, turn, if turn == HUB_Y - 1 { Dir::S } else if turn == HUB_Y + 3 { Dir::N } else { Dir::W }));
    }
    belt_line(w, &c);
}

/// Шаг подвода: что поставить на клетку.
#[derive(Clone, Copy, PartialEq)]
enum Piece {
    Drill(Dir),
    Belt(Dir),
    TunIn(Dir),
    TunOut(Dir),
}

/// Цена нырка под землю в шагах: подвод ныряет, только когда обход заметно длиннее.
const HOP_COST: u32 = 8;

/// Самый дешёвый подвод от свободной жилы стихии `elem` до клетки `target` (она выдаёт на
/// восток). Идёт по пустым клеткам, а под занятыми может пройти подземным конвейером.
fn route(w: &World, blocked: &[bool], target: (i32, i32), elem: u8) -> Option<Vec<(i32, i32, Piece)>> {
    use std::cmp::Reverse;
    use std::collections::BinaryHeap;
    let n = (W * H) as usize;
    let idx = |x: i32, y: i32| (y * W + x) as usize;
    let free = |x: i32, y: i32| World::inb(x, y) && w.get(x, y).is_none() && !blocked[idx(x, y)];
    let mut dist = vec![u32::MAX; n];
    // next[c] - следующая клетка пути к цели; hop[c] - клетка выхода, если c - вход под землю.
    let mut next = vec![usize::MAX; n];
    let mut hop = vec![usize::MAX; n];
    let ti = idx(target.0, target.1);
    dist[ti] = 0;
    let mut heap = BinaryHeap::from([Reverse((0u32, ti))]);
    let mut goal = None;
    while let Some(Reverse((d0, i))) = heap.pop() {
        if d0 > dist[i] {
            continue;
        }
        let (x, y) = (i as i32 % W, i as i32 / W);
        if i != ti && hop[i] == usize::MAX && w.vein(x, y) == elem + 1 {
            goal = Some(i);
            break;
        }
        for d in DIRS {
            // Обычный шаг: сосед будет лентой, толкающей в эту клетку. Во вход под землю
            // нельзя толкать спереди.
            let (nx, ny) = (x - d.dx(), y - d.dy());
            let front_of_entry = hop[i] != usize::MAX && Dir::from_delta(hop[i] as i32 % W - x, hop[i] as i32 / W - y) == d.opp();
            if free(nx, ny) && !front_of_entry && d0 + 1 < dist[idx(nx, ny)] {
                let ni = idx(nx, ny);
                dist[ni] = d0 + 1;
                next[ni] = i;
                hop[ni] = usize::MAX;
                heap.push(Reverse((d0 + 1, ni)));
            }
            // Нырок: выход встаёт позади этой клетки и толкает в неё вдоль d.
            let (ex, ey) = (x - d.dx(), y - d.dy());
            if !free(ex, ey) || idx(ex, ey) == next[i] || i == ti && d != Dir::E {
                continue;
            }
            if hop[i] != usize::MAX && Dir::from_delta(hop[i] as i32 % W - x, hop[i] as i32 / W - y) == d.opp() {
                continue;
            }
            for k in 2..=TUNNEL_RANGE {
                let (sx, sy) = (ex - d.dx() * k, ey - d.dy() * k);
                if !free(sx, sy) {
                    continue;
                }
                let si = idx(sx, sy);
                let c = d0 + HOP_COST + k as u32;
                if c < dist[si] {
                    dist[si] = c;
                    next[si] = i;
                    hop[si] = idx(ex, ey);
                    heap.push(Reverse((c, si)));
                }
            }
        }
    }
    let mut path = Vec::new();
    let mut c = goal?;
    let mut first = true;
    let mut seen = vec![false; n];
    loop {
        let (x, y) = (c as i32 % W, c as i32 / W);
        if std::mem::replace(&mut seen[c], true) {
            return None;
        }
        if c == ti {
            path.push((x, y, Piece::Belt(Dir::E)));
            return Some(path);
        }
        if hop[c] != usize::MAX {
            let (ex, ey) = (hop[c] as i32 % W, hop[c] as i32 / W);
            if std::mem::replace(&mut seen[hop[c]], true) {
                return None;
            }
            let d = Dir::from_delta(ex - x, ey - y);
            path.push((x, y, Piece::TunIn(d)));
            path.push((ex, ey, Piece::TunOut(d)));
        } else {
            let (nx, ny) = (next[c] as i32 % W, next[c] as i32 / W);
            let d = Dir::from_delta(nx - x, ny - y);
            path.push((x, y, if first { Piece::Drill(d) } else { Piece::Belt(d) }));
        }
        first = false;
        c = next[c];
    }
}

/// Все ли подземные пары сложились так, как задумано.
fn tunnels_ok(w: &World, pairs: &[((i32, i32), (i32, i32))]) -> bool {
    pairs.iter().all(|&(a, b)| w.tunnel_exit(a.0, a.1).map(|e| (e.0, e.1)) == Some(b) && w.tunnel_is_exit(b.0, b.1))
}

/// Собирает фабрику: `slots[i]` - смесь в i-м месте (0 - пусто), места идут по колонкам.
/// `Ok` - суммарная длина подводов.
fn build(seed: u64, slots: &[u8; 18]) -> Result<(World, usize), String> {
    let mut w = new_world(seed);
    let mut pairs: Vec<((i32, i32), (i32, i32))> = Vec::new();
    let mut inputs: Vec<(i32, i32, u8)> = Vec::new();
    for (i, &mask) in slots.iter().enumerate() {
        if mask == 0 {
            continue;
        }
        let (x0, y0) = (COL_X[i / 6], ROW_Y[i % 6]);
        let (g, taps) = build_module(mask);
        stamp(&mut w, &g, x0, y0, false);
        for cy in 0..g.h {
            for cx in 0..g.w {
                if let Cell::TunIn(_) = g.at(cx, cy) {
                    let e = w.tunnel_exit(x0 + cx, y0 + cy).unwrap();
                    pairs.push(((x0 + cx, y0 + cy), (e.0, e.1)));
                }
            }
        }
        for (e, r) in taps {
            inputs.push((x0 - 1, y0 + r, e));
        }
    }
    for c in 0..3 {
        let lx = COL_X[c] + MW;
        let used = |r: usize| slots[c * 6 + r] != 0;
        if let Some(r) = (0..3).find(|r| used(*r)) {
            lane(&mut w, lx, ROW_Y[r] + MH - 1, [HUB_Y, HUB_Y - 1, HUB_Y][c]);
        }
        if let Some(r) = (3..6).rev().find(|r| used(*r)) {
            lane(&mut w, lx, ROW_Y[r] + MH - 1, [HUB_Y + 2, HUB_Y + 3, HUB_Y + 2][c]);
        }
    }
    // Подводы: входные клетки заняты заранее, чтобы чужой подвод не лёг поперёк входа.
    let mut blocked = vec![false; (W * H) as usize];
    for &(x, y, _) in &inputs {
        blocked[(y * W + x) as usize] = true;
    }
    let mut total = 0;
    // Каждый раз кладём самый короткий из оставшихся подводов: длинные ищут обход сами.
    let mut left = inputs.clone();
    while !left.is_empty() {
        let mut best: Option<(usize, Vec<(i32, i32, Piece)>)> = None;
        for (n, &(x, y, e)) in left.iter().enumerate() {
            blocked[(y * W + x) as usize] = false;
            let r = route(&w, &blocked, (x, y), e);
            blocked[(y * W + x) as usize] = true;
            match r {
                Some(p) => {
                    if best.as_ref().map_or(true, |b| p.len() < b.1.len()) {
                        best = Some((n, p));
                    }
                }
                None => {
                    if std::env::var("MEGA3_DEBUG").is_ok() {
                        eprintln!("seed {}: осталось {} из {}, нет пути ко входу ({}, {}) стихии {}", seed, left.len(), inputs.len(), x, y, e);
                    }
                    return Err("нет пути к жиле".to_string());
                }
            }
        }
        let (n, path) = best.unwrap();
        left.swap_remove(n);
        total += path.len();
        let mut entry = None;
        for &(px, py, piece) in &path {
            match piece {
                Piece::Drill(d) => put(&mut w, kind("ГЛУБИННЫЙ БУР"), px, py, d),
                Piece::Belt(d) => put(&mut w, kind("ЭКСПРЕСС-ЛЕНТА"), px, py, d),
                Piece::TunIn(d) => {
                    put(&mut w, kind("ПОДЗЕМНЫЙ КОНВЕЙЕР"), px, py, d);
                    entry = Some((px, py));
                }
                Piece::TunOut(d) => {
                    put(&mut w, kind("ПОДЗЕМНЫЙ КОНВЕЙЕР"), px, py, d);
                    pairs.push((entry.take().unwrap(), (px, py)));
                }
            }
        }
        if !tunnels_ok(&w, &pairs) {
            return Err("подземные конвейеры перепутались".to_string());
        }
    }
    w.money = 1e12;
    w.discovered.iter_mut().for_each(|d| *d = false);
    Ok((w, total))
}

struct Lcg(u64);
impl Lcg {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) % n as u64) as usize
    }
}

pub fn generate(path: &str, args: &[String]) {
    let seeds = arg_num(args, "--seeds", 400) as u64;
    let tries = arg_num(args, "--tries", 30);
    let mut best: Option<(usize, u64, [u8; 18])> = None;
    let mut ok = 0;
    let mut rng = Lcg(12345);
    for seed in 1..=seeds {
        for _ in 0..tries {
            let mut slots = [0u8; 18];
            for (i, s) in slots.iter_mut().enumerate().take(15) {
                *s = i as u8 + 1;
            }
            for i in (1..18).rev() {
                slots.swap(i, rng.below(i + 1));
            }
            if let Ok((_, len)) = build(seed, &slots) {
                ok += 1;
                if best.map_or(true, |b| len < b.0) {
                    best = Some((len, seed, slots));
                }
            }
        }
    }
    let (len, seed, slots) = best.expect("ни одна раскладка не подошла");
    let (w, _) = build(seed, &slots).unwrap();
    let name = std::path::Path::new(path).file_name().unwrap().to_string_lossy().to_string();
    assert!(!name.starts_with("alembic_save") && name != "megafactory_save.txt", "чужие сохранения не трогаем");
    std::fs::write(path, w.save()).expect("запись сохранения");
    println!(
        "megasave3: seed={} раскладка={:?} (подошло {} из {}), построек={}, подводы={} клеток",
        seed,
        slots,
        ok,
        seeds * tries as u64,
        w.tiles.iter().flatten().count() - 9,
        len
    );
}

fn item_of_id(id: usize) -> Item {
    Item::new((id % 15) as u8 + 1, (id / 15 % 3) as u8 + 1, (id / 45) as u8)
}

/// Гонит `secs` секунд; возвращает секунду, на которой проданы все 1170 позиций.
fn run_until_full(w: &mut World, secs: u32, fps: u32) -> Option<u32> {
    let mut full = None;
    for s in 0..secs {
        run_secs(w, 1, fps);
        if full.is_none() && w.sold_by_id.iter().all(|c| *c > 0) {
            full = Some(s + 1);
        }
    }
    full
}

fn report(w: &World) {
    let n = w.sold_by_id.iter().filter(|c| **c > 0).count();
    if n < N_ITEM_IDS {
        let miss: Vec<String> = (0..N_ITEM_IDS).filter(|i| w.sold_by_id[*i] == 0).map(|i| item_of_id(i).name()).collect();
        println!("   не продано {}: {:?}", miss.len(), miss);
    }
    let mut order: Vec<usize> = (0..N_ITEM_IDS).collect();
    order.sort_by_key(|i| w.sold_by_id[*i]);
    for p in 1..=3u8 {
        let worst = order.iter().find(|i| item_of_id(**i).purity == p).unwrap();
        let under5 = order.iter().filter(|i| item_of_id(**i).purity == p && w.sold_by_id[**i] < 5).count();
        println!("   чистота {}: минимум x{} ({}), позиций меньше 5 шт: {}", ["I", "II", "III"][p as usize - 1], w.sold_by_id[*worst], item_of_id(*worst).name(), under5);
    }
    let rare: Vec<String> = order.iter().take(12).map(|i| format!("{} x{}", item_of_id(*i).name(), w.sold_by_id[*i])).collect();
    println!("   самые редкие: {}", rare.join(", "));
}

pub fn verify(path: &str, args: &[String]) {
    let fps = arg_num(args, "--fps", 60) as u32;
    let text = std::fs::read_to_string(path).expect("файл сохранения");
    let rows: Vec<Vec<i64>> = text
        .lines()
        .skip(2)
        .filter(|l| !l.starts_with("D ") && !l.starts_with("U ") && !l.trim().is_empty())
        .map(|l| l.split_whitespace().filter_map(|v| v.parse().ok()).collect())
        .collect();
    let mut w = World::load(&text).expect("сохранение не загрузилось");
    let loaded = w.tiles.iter().flatten().count() - 9;
    println!("1. загрузка: построек в файле {}, в мире {} -> {}", rows.len(), loaded, if rows.len() == loaded { "OK" } else { "ПОТЕРИ" });
    println!("   деньги={:.0} уровень={} улучшения={:?} seed={} (кадр симуляции 1/{} с)", w.money, w.level, w.upgrades, w.seed, fps);
    let trade = Kind::by_name("ТОРГОВЫЙ ПОСТ").0 as i64;
    let posts_file = rows.iter().filter(|r| r.get(2) == Some(&trade)).count();
    let posts_world = w.tiles.iter().flatten().filter(|b| b.kind.role() == Role::Trade).count();
    println!("2. торговых постов: в файле {}, в мире {} -> {}", posts_file, posts_world, if posts_file + posts_world == 0 { "OK" } else { "ЕСТЬ" });
    let mut by_kind = std::collections::BTreeMap::new();
    for b in w.tiles.iter().flatten() {
        *by_kind.entry(b.kind.name()).or_insert(0) += 1;
    }
    println!("   состав: {:?}", by_kind);
    let zero = w.discovered.iter().all(|d| !*d);
    let t = run_until_full(&mut w, 20 * 60, fps);
    let n = w.sold_by_id.iter().filter(|c| **c > 0).count();
    println!(
        "3. каталог в файле {}; за 20 игровых минут в лавке продано {}/{} позиций{}, всего {} шт -> {}",
        if zero { "пуст" } else { "НЕ ПУСТ" },
        n,
        N_ITEM_IDS,
        t.map_or(String::new(), |t| format!(" (полный набор на {} с)", t)),
        w.total_sold,
        if zero && n == N_ITEM_IDS { "OK" } else { "НЕТ" }
    );
    report(&w);
    let sold0 = w.total_sold;
    w.sold_by_id.iter_mut().for_each(|c| *c = 0);
    let t = run_until_full(&mut w, 15 * 60, fps);
    let n = w.sold_by_id.iter().filter(|c| **c > 0).count();
    println!(
        "4. счётчик обнулён на разогретой фабрике; за следующие 15 минут продано {}/{} позиций{}, всего {} шт ({:.0}/с) -> {}",
        n,
        N_ITEM_IDS,
        t.map_or(String::new(), |t| format!(" (полный набор через {} с)", t)),
        w.total_sold - sold0,
        (w.total_sold - sold0) as f64 / 900.0,
        if n == N_ITEM_IDS { "OK" } else { "НЕТ" }
    );
    report(&w);
}
