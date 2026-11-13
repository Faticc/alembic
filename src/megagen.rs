//! Скрытые режимы для сохранения "мегафабрика": фабрика, которая одновременно производит
//! и продаёт все 390 видов предметов.
//!
//! `--gen-megasave <файл>`     собрать фабрику и записать сохранение;
//! `--verify-megasave <файл>`  проверить сохранение без окна;
//! `--megastats <файл>`        как часто продаётся каждый вид (для настройки);
//! `--megashot <png> [--load <файл>]`  кадр всей карты.
//!
//! Устройство. На каждую из 15 смесей - один модуль 16x9 клеток. Слева в модуль входят
//! эссенции стихий, смесители собирают из них смесь, дальше корневой делитель пополам
//! кормит две "спины" (ряды делителей), от которых эссенцию берут машины первого ряда.
//! Машины стоят вплотную и передают друг другу продукцию напрямую через настроенные
//! стороны; у каждой формы есть выход в торговый пост. Стихии идут к модулям по четырём
//! магистралям в коридоре вдоль колонны модулей, ответвления пересекают их мостами.

use crate::world::*;
use std::collections::VecDeque;

const MW: i32 = 16;
const MH: i32 = 9;

/// Чертёж модуля. Клетка: `.` пусто, `T` торговый пост, `> < ^ v` экспресс-лента,
/// `S:выходы` делитель, `N:выходы` машина, делающая форму N. Входы выводятся сами:
/// сторона открыта на приём, если сосед выдаёт в неё. Колонки 0..2 занимает ввод эссенций.
const MODULE: [&str; MH as usize] = [
    ". . . .     14:e  >     >     21:e  S:es  25:s S:ws 8:w   <     7:w   <    .",
    ". . . .     5:ne  T     12:ew 16:ne T     T    T    6:nw  <     5:n   4:n  T",
    ". . . T     1:wne 3:en  4:n   9:ne  T     .    T    2:nw  3:n   1:wne 3:ne 15:n",
    ". . . S:es  S:en  >     >     S:en  >     >    >    S:en  >     S:en  T    .",
    ". . . >     S:es  >     >     >     S:es  >    >    S:es  >     >     >    S:s",
    ". . . 5:s   1:we  3:es  15:e  19:s  10:we T    5:s  1:we  3:es  11:es 20:s 2:w",
    ". . . 14:es 17:s  4:wes 13:s  >     S:se  T    >    7:s   4:wse T     T    .",
    ". . . T     S:sw  12:e  18:es S:es  23:e  T    T    S:sw  13:se T     .    .",
    ". . . .     >     >     22:e  T     .     .    T    24:w  <     .     .    .",
];

/// Смеси по сторонам карты, сверху вниз. Подобрано так, чтобы каждая стихия была нужна
/// не более чем четырём смешанным модулям на сторону.
const LEFT: [u8; 8] = [5, 6, 9, 10, 11, 13, 1, 2];
const RIGHT: [u8; 7] = [15, 3, 12, 7, 14, 4, 8];
const TOP: i32 = 4;

#[derive(Clone, PartialEq)]
enum Cell {
    Empty,
    Trade,
    Belt(Dir),
    /// Машина, смеситель или делитель: вид постройки и стороны выдачи.
    Mach(u8, Vec<Dir>),
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

fn parse_dirs(s: &str) -> Vec<Dir> {
    s.chars()
        .map(|c| match c {
            'n' => Dir::N,
            'e' => Dir::E,
            's' => Dir::S,
            'w' => Dir::W,
            _ => panic!("плохое направление {}", c),
        })
        .collect()
}

fn idx(x: i32, y: i32) -> usize {
    (y * MW + x) as usize
}

/// Модуль для смеси `mask` (без отражения) и строки, по которым в него входят стихии.
fn build_module(mask: u8) -> (Vec<Cell>, Vec<(u8, i32)>) {
    let mut g = vec![Cell::Empty; (MW * MH) as usize];
    for (y, row) in MODULE.iter().enumerate() {
        let toks: Vec<&str> = row.split_whitespace().collect();
        assert_eq!(toks.len(), MW as usize, "строка {} чертежа", y);
        for (x, t) in toks.iter().enumerate() {
            g[idx(x as i32, y as i32)] = match *t {
                "." => Cell::Empty,
                "T" => Cell::Trade,
                ">" => Cell::Belt(Dir::E),
                "<" => Cell::Belt(Dir::W),
                "^" => Cell::Belt(Dir::N),
                "v" => Cell::Belt(Dir::S),
                t => {
                    let (a, b) = t.split_once(':').expect("клетка чертежа");
                    let k = if a == "S" { kind("ДЕЛИТЕЛЬ") } else { kind_for_form(a.parse().expect("форма")) };
                    Cell::Mach(k, parse_dirs(b))
                }
            };
        }
    }
    let elems: Vec<u8> = (0..4).filter(|e| mask & (1 << e) != 0).collect();
    let mixer = || Cell::Mach(kind("СМЕСИТЕЛЬ"), vec![Dir::E]);
    let mut set = |x: i32, y: i32, c: Cell| {
        assert!(g[idx(x, y)] == Cell::Empty);
        g[idx(x, y)] = c;
    };
    let rows: Vec<i32> = if elems.len() == 1 { vec![3] } else { [2, 4, 1, 5][..elems.len()].to_vec() };
    if elems.len() == 1 {
        for x in 0..3 {
            set(x, 3, Cell::Belt(Dir::E));
        }
    } else {
        set(0, 2, Cell::Belt(Dir::S));
        set(0, 4, Cell::Belt(Dir::N));
        set(0, 3, mixer());
        if elems.len() >= 3 {
            set(0, 1, Cell::Belt(Dir::E));
            set(1, 1, Cell::Belt(Dir::S));
            set(1, 2, Cell::Belt(Dir::S));
            set(1, 3, mixer());
        } else {
            set(1, 3, Cell::Belt(Dir::E));
        }
        if elems.len() == 4 {
            set(0, 5, Cell::Belt(Dir::E));
            set(1, 5, Cell::Belt(Dir::E));
            set(2, 5, Cell::Belt(Dir::N));
            set(2, 4, Cell::Belt(Dir::N));
            set(2, 3, mixer());
        } else {
            set(2, 3, Cell::Belt(Dir::E));
        }
    }
    (g, elems.into_iter().zip(rows).collect())
}

fn cell(g: &[Cell], x: i32, y: i32) -> &Cell {
    const E: Cell = Cell::Empty;
    if x < 0 || y < 0 || x >= MW || y >= MH {
        &E
    } else {
        &g[idx(x, y)]
    }
}

/// Выдаёт ли сосед клетки (x, y) со стороны `side` предметы в неё.
fn fed_from(g: &[Cell], x: i32, y: i32, side: Dir) -> bool {
    match cell(g, x + side.dx(), y + side.dy()) {
        Cell::Belt(d) => *d == side.opp(),
        Cell::Mach(_, outs) => outs.contains(&side.opp()),
        _ => false,
    }
}

/// Проверяет чертёж: каждой машине приходит ровно то, что ей нужно, каждый выход куда-то
/// ведёт, ни на одну ленту не попадает предмет, который с неё некому забрать.
/// Возвращает маску форм, попадающих в торговые посты.
fn validate(g: &[Cell], taps: &[(u8, i32)]) -> u32 {
    let n = (MW * MH) as usize;
    let mut forms = vec![0u32; n];
    let incoming = |forms: &Vec<u32>, x: i32, y: i32| -> u32 {
        let mut m = 0;
        for d in DIRS {
            if fed_from(g, x, y, d) {
                m |= forms[idx(x + d.dx(), y + d.dy())];
            }
        }
        if x == 0 && taps.iter().any(|t| t.1 == y) {
            m |= 1;
        }
        m
    };
    for _ in 0..64 {
        for y in 0..MH {
            for x in 0..MW {
                let inc = incoming(&forms, x, y);
                forms[idx(x, y)] = match cell(g, x, y) {
                    Cell::Belt(_) => inc,
                    Cell::Mach(k, _) => match Kind(*k).role() {
                        Role::Convert(_, to, _) | Role::Assemble(_, _, to, _) => 1 << to,
                        Role::Mixer => 1,
                        _ => inc,
                    },
                    _ => 0,
                };
            }
        }
    }
    let mut sold = 0;
    for y in 0..MH {
        for x in 0..MW {
            let inc = incoming(&forms, x, y);
            let at = format!("клетка ({}, {})", x, y);
            match cell(g, x, y) {
                Cell::Trade => sold |= inc,
                Cell::Belt(d) => {
                    assert!(inc != 0, "{}: пустая лента", at);
                    assert!(!fed_from(g, x, y, *d), "{}: в ленту толкают спереди", at);
                    let need = match cell(g, x + d.dx(), y + d.dy()) {
                        Cell::Mach(k, _) => match Kind(*k).role() {
                            Role::Convert(a, _, _) => 1 << a,
                            Role::Assemble(a, b, _, _) => 1 << a | 1 << b,
                            Role::Mixer => 1,
                            _ => u32::MAX,
                        },
                        Cell::Empty => panic!("{}: лента в никуда", at),
                        _ => u32::MAX,
                    };
                    assert!(inc & !need == 0, "{}: лента везёт лишнее {:b}", at, inc);
                }
                Cell::Mach(k, outs) => {
                    match Kind(*k).role() {
                        Role::Convert(a, _, _) => assert!(inc == 1 << a, "{}: вход {:b}", at, inc),
                        Role::Assemble(a, b, _, _) => assert!(inc == 1 << a | 1 << b, "{}: вход {:b}", at, inc),
                        Role::Mixer => assert!(inc == 1, "{}: вход смесителя", at),
                        _ => assert!(inc != 0, "{}: делитель без входа", at),
                    }
                    assert!(!outs.is_empty(), "{}: нет выхода", at);
                    for d in outs {
                        assert!(!fed_from(g, x, y, *d), "{}: встречные стороны", at);
                        let ok = match cell(g, x + d.dx(), y + d.dy()) {
                            Cell::Trade => true,
                            Cell::Belt(bd) => *bd != d.opp(),
                            Cell::Mach(k2, _) => match Kind(*k2).role() {
                                Role::Convert(a, _, _) => forms[idx(x, y)] & !(1 << a) == 0,
                                Role::Assemble(a, b, _, _) => forms[idx(x, y)] & !(1 << a | 1 << b) == 0,
                                Role::Mixer => forms[idx(x, y)] == 1,
                                _ => true,
                            },
                            Cell::Empty => false,
                        };
                        assert!(ok, "{}: выход {:?} никуда не ведёт", at, d);
                    }
                }
                Cell::Empty => {}
            }
        }
    }
    sold
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

fn stamp(w: &mut World, g: &[Cell], x0: i32, y0: i32, mirror: bool, mix_level: u8) {
    let md = |d: Dir| if mirror && d.dx() != 0 { d.opp() } else { d };
    for cy in 0..MH {
        for cx in 0..MW {
            let (x, y) = (if mirror { x0 + MW - 1 - cx } else { x0 + cx }, y0 + cy);
            match cell(g, cx, cy) {
                Cell::Empty => {}
                Cell::Trade => put(w, kind("ТОРГОВЫЙ ПОСТ"), x, y, Dir::N),
                Cell::Belt(d) => put(w, kind("ЭКСПРЕСС-ЛЕНТА"), x, y, md(*d)),
                Cell::Mach(k, outs) => {
                    put(w, *k, x, y, md(outs[0]));
                    let mut modes = Vec::new();
                    for d in DIRS {
                        if outs.contains(&d) {
                            modes.push((md(d), PORT_OUT));
                        } else if fed_from(g, cx, cy, d) {
                            modes.push((md(d), PORT_IN));
                        }
                    }
                    ports_abs(w, x, y, &modes);
                    if Kind(*k).role() == Role::Mixer {
                        w.force_level(x, y, mix_level);
                    }
                }
            }
        }
    }
}

/// Кратчайший путь от свободной жилы стихии `elem` до клетки `target` по пустым клеткам
/// полосы x0..=x1. Первый элемент - клетка под бур, последний - `target`.
fn route(w: &World, x0: i32, x1: i32, target: (i32, i32), elem: u8) -> Option<Vec<(i32, i32)>> {
    if w.get(target.0, target.1).is_some() {
        return None;
    }
    let mut prev = vec![usize::MAX; (W * H) as usize];
    let ti = (target.1 * W + target.0) as usize;
    prev[ti] = ti;
    let mut q = VecDeque::from([target]);
    while let Some((x, y)) = q.pop_front() {
        let i = (y * W + x) as usize;
        if i != ti && w.vein(x, y) == elem + 1 {
            let mut path = vec![(x, y)];
            let mut c = i;
            while c != ti {
                c = prev[c];
                path.push((c as i32 % W, c as i32 / W));
            }
            return Some(path);
        }
        for d in DIRS {
            let (nx, ny) = (x + d.dx(), y + d.dy());
            if nx < x0 || nx > x1 || ny < 0 || ny >= H || w.get(nx, ny).is_some() {
                continue;
            }
            let ni = (ny * W + nx) as usize;
            if prev[ni] == usize::MAX {
                prev[ni] = i;
                q.push_back((nx, ny));
            }
        }
    }
    None
}

/// Ставит бур и ленту по пути; последняя клетка ленты смотрит в сторону `last`.
fn lay(w: &mut World, path: &[(i32, i32)], last: Dir) {
    for (i, &(x, y)) in path.iter().enumerate() {
        let d = match path.get(i + 1) {
            Some(&(nx, ny)) => Dir::from_delta(nx - x, ny - y),
            None => last,
        };
        put(w, if i == 0 { kind("ГЛУБИННЫЙ БУР") } else { kind("ЭКСПРЕСС-ЛЕНТА") }, x, y, d);
    }
}

/// Одна сторона карты: колонна модулей, коридор магистралей и подводы от жил.
/// `Err` - на этом сиде не нашлось жил или пути. `Ok` - суммарная длина подводов.
fn build_side(w: &mut World, masks: &[u8], left: bool, mix_level: u8) -> Result<usize, String> {
    // Колонки магистралей от внешней к внутренней, край модулей, граница внешней полосы.
    let (cols, mx0, bx, toward) = if left { ([19, 20, 21, 22], 23, 18, Dir::E) } else { ([61, 60, 59, 58], 42, 62, Dir::W) };
    let region = if left { (0, bx - 1) } else { (bx + 1, W - 1) };
    let mut taps: [Vec<i32>; 4] = Default::default();
    let mut pure: Vec<(i32, u8)> = Vec::new();
    for (i, &mask) in masks.iter().enumerate() {
        let y0 = TOP + MH * i as i32;
        let (g, rows) = build_module(mask);
        let sold = validate(&g, &rows);
        assert_eq!(sold, (1 << N_FORMS) - 1, "модуль продаёт не все формы");
        stamp(w, &g, mx0, y0, !left, mix_level);
        for (e, r) in rows {
            if mask.count_ones() == 1 {
                pure.push((y0 + r, e));
            } else {
                taps[e as usize].push(y0 + r);
            }
        }
    }
    let mut total = 0;
    // Чистым стихиям - отдельный бур: ответвление идёт поперёк всего коридора.
    for &(y, e) in &pure {
        let path = route(w, region.0, region.1, (bx, y), e).ok_or("нет пути к чистому модулю")?;
        total += path.len();
        lay(w, &path, toward);
    }
    // Магистрали: каждая подводится сверху или снизу, как ближе.
    let mut extent = [(0, -1); 4];
    let mut flow = [Dir::S; 4];
    for j in 0..4 {
        let (lo, hi) = (*taps[j].iter().min().unwrap(), *taps[j].iter().max().unwrap());
        let (rt, rb) = (TOP - 1 - j as i32, TOP + MH * 8 + j as i32);
        let top = route(w, region.0, region.1, (bx, rt), j as u8);
        let bot = route(w, region.0, region.1, (bx, rb), j as u8);
        let cost = |p: &Option<Vec<(i32, i32)>>, run: i32| p.as_ref().map_or(usize::MAX, |p| p.len() + run as usize);
        let from_top = cost(&top, hi - rt) <= cost(&bot, rb - lo);
        let path = if from_top { top } else { bot }.ok_or("нет пути к магистрали")?;
        total += path.len();
        lay(w, &path, toward);
        let r = if from_top { rt } else { rb };
        for c in &cols[..j] {
            put(w, kind("ЭКСПРЕСС-ЛЕНТА"), *c, r, toward);
        }
        flow[j] = if from_top { Dir::S } else { Dir::N };
        extent[j] = if from_top { (r, hi) } else { (lo, r) };
    }
    // Клетки, где ответвление пересекает коридор.
    let mut cross = vec![false; (W * H) as usize];
    for j in 0..4 {
        for &y in &taps[j] {
            for c in &cols[j + 1..] {
                cross[(y * W + c) as usize] = true;
            }
        }
    }
    for &(y, _) in &pure {
        for c in cols {
            cross[(y * W + c) as usize] = true;
        }
    }
    for j in 0..4 {
        let (lo, hi) = extent[j];
        let last = if flow[j] == Dir::S { hi } else { lo };
        for y in lo..=hi {
            let x = cols[j];
            if taps[j].contains(&y) {
                put(w, kind("ДЕЛИТЕЛЬ"), x, y, flow[j]);
                let mut modes = vec![(flow[j].opp(), PORT_IN), (toward, PORT_OUT)];
                if y != last {
                    modes.push((flow[j], PORT_OUT));
                }
                ports_abs(w, x, y, &modes);
            } else if cross[(y * W + x) as usize] {
                put(w, kind("МОСТ"), x, y, flow[j]);
            } else {
                put(w, kind("ЭКСПРЕСС-ЛЕНТА"), x, y, flow[j]);
            }
        }
    }
    for y in 0..H {
        for c in cols {
            if cross[(y * W + c) as usize] && w.get(c, y).is_none() {
                put(w, kind("ЭКСПРЕСС-ЛЕНТА"), c, y, toward);
            }
        }
    }
    Ok(total)
}

fn build(seed: u64, mix_level: u8) -> Result<(World, usize), String> {
    let mut w = World::new(seed);
    w.money = 1e15;
    w.level = FINAL_LEVEL;
    w.upgrades = UPGRADE_MAX;
    let a = build_side(&mut w, &LEFT, true, mix_level)?;
    let b = build_side(&mut w, &RIGHT, false, mix_level)?;
    w.money = 1e12;
    Ok((w, a + b))
}

fn mix_level(args: &[String]) -> u8 {
    args.iter().position(|a| a == "--mix-level").and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(1)
}

fn kind_name(i: usize) -> String {
    Item::new((i % 15) as u8 + 1, 1, (i / 15) as u8).name()
}

fn found(w: &World) -> usize {
    w.discovered.iter().filter(|d| **d).count()
}

fn missing(w: &World) -> Vec<String> {
    (0..N_KINDS).filter(|i| !w.discovered[*i]).map(kind_name).collect()
}

/// Прогоняет `secs` игровых секунд кадрами по 1/fps; возвращает секунду, на которой каталог
/// стал полным (если стал).
fn run_fps(w: &mut World, secs: u32, fps: u32) -> Option<u32> {
    let mut full = None;
    for s in 0..secs {
        for _ in 0..fps {
            w.update(1.0 / fps as f32);
        }
        w.pending_new.clear();
        if full.is_none() && found(w) == N_KINDS {
            full = Some(s + 1);
        }
    }
    full
}

pub fn generate(path: &str, args: &[String]) {
    let lvl = mix_level(args);
    let mut best: Option<(u64, usize)> = None;
    let mut ok = 0;
    for seed in 1..=3000u64 {
        if let Ok((_, len)) = build(seed, lvl) {
            ok += 1;
            if best.map_or(true, |b| len < b.1) {
                best = Some((seed, len));
            }
        }
    }
    let (seed, len) = best.expect("ни один сид не подошёл");
    let (w, _) = build(seed, lvl).unwrap();
    assert!(!std::path::Path::new(path).file_name().unwrap().to_string_lossy().starts_with("alembic_save"));
    std::fs::write(path, w.save()).expect("запись сохранения");
    println!(
        "megasave: seed={} (подошло сидов {} из 3000), построек={}, подводы={} клеток, уровень смесителей={}",
        seed,
        ok,
        w.tiles.iter().flatten().count() - 9,
        len,
        lvl
    );
}

fn run(w: &mut World, secs: u32) -> Option<u32> {
    run_fps(w, secs, 60)
}

pub fn verify(path: &str, args: &[String]) {
    let fps: u32 = args.iter().position(|a| a == "--fps").and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(60);
    let run = |w: &mut World, secs: u32| run_fps(w, secs, fps);
    let text = std::fs::read_to_string(path).expect("файл сохранения");
    let in_file = text.lines().skip(2).filter(|l| !l.starts_with("D ") && !l.starts_with("U ") && !l.trim().is_empty()).count();
    let mut w = World::load(&text).expect("сохранение не загрузилось");
    let loaded = w.tiles.iter().flatten().count() - 9;
    println!("1. загрузка: построек в файле {}, в мире {} -> {}", in_file, loaded, if in_file == loaded { "OK" } else { "ПОТЕРИ" });
    println!("   деньги={:.0} уровень={} улучшения={:?} seed={} (кадр симуляции 1/{} с)", w.money, w.level, w.upgrades, w.seed, fps);
    let zero = found(&w) == 0;
    let t = run(&mut w, 15 * 60);
    println!(
        "2. каталог в файле {}; после 15 игровых минут открыто {}/{}{} -> {}",
        if zero { "пуст" } else { "НЕ ПУСТ" },
        found(&w),
        N_KINDS,
        t.map_or(String::new(), |t| format!(" (полный на {} с)", t)),
        if zero && found(&w) == N_KINDS { "OK" } else { "НЕТ" }
    );
    if found(&w) != N_KINDS {
        println!("   не хватает: {:?}", missing(&w));
    }
    let sold0 = w.total_sold;
    w.discovered.iter_mut().for_each(|d| *d = false);
    let t = run(&mut w, 10 * 60);
    println!(
        "3. каталог обнулён на разогретой фабрике; за следующие 10 минут открыто {}/{}{}, продано {} шт -> {}",
        found(&w),
        N_KINDS,
        t.map_or(String::new(), |t| format!(" (полный через {} с)", t)),
        w.total_sold - sold0,
        if found(&w) == N_KINDS { "OK" } else { "НЕТ" }
    );
    if found(&w) != N_KINDS {
        println!("   не хватает: {:?}", missing(&w));
    }
}

/// Десять минутных окон на разогретой фабрике: в скольких из них продавался каждый вид.
pub fn stats(path: &str) {
    let text = std::fs::read_to_string(path).expect("файл сохранения");
    let mut w = World::load(&text).expect("сохранение не загрузилось");
    run(&mut w, 300);
    let mut hits = vec![0u32; N_KINDS];
    let mut slowest = vec![0u32; N_KINDS];
    for _ in 0..10 {
        w.discovered.iter_mut().for_each(|d| *d = false);
        for s in 0..60 {
            run(&mut w, 1);
            for i in 0..N_KINDS {
                if !w.discovered[i] {
                    slowest[i] = slowest[i].max(s + 1);
                }
            }
        }
        for i in 0..N_KINDS {
            hits[i] += w.discovered[i] as u32;
        }
    }
    let mut order: Vec<usize> = (0..N_KINDS).collect();
    order.sort_by_key(|i| (hits[*i], u32::MAX - slowest[*i]));
    println!("виды, продававшиеся во всех 10 минутных окнах: {}/{}", hits.iter().filter(|h| **h == 10).count(), N_KINDS);
    for i in order.iter().take(25) {
        println!("  {:<28} окон {:>2}/10, самое долгое ожидание первой продажи в окне {} с", kind_name(*i), hits[*i], slowest[*i]);
    }
}

pub fn shot(png: &str, save: &str) {
    use crate::engine::{audio::Audio, png::save_png, Image, Input};
    let text = std::fs::read_to_string(save).expect("файл сохранения");
    let mut game = crate::game::Game::new(World::load(&text).expect("сохранение"), Audio::new(false), false);
    game.persist = false;
    game.zoom = 1;
    game.cam_x = W as f32 / 2.0;
    game.cam_y = H as f32 / 2.0;
    game.show_path = false;
    let (cw, ch) = (W * 24 + 80, H * 24 + 80);
    let inp = Input { mx: -1, my: -1, ..Default::default() };
    for _ in 0..60 * 120 {
        game.update(&inp, 1.0 / 60.0, cw, ch);
    }
    let mut canvas = Image::new(cw, ch);
    game.draw(&mut canvas);
    save_png(png, &canvas, true).expect("save screenshot");
    println!("кадр {}: открыто {}/{} за 2 минуты", png, found(&game.world), N_KINDS);
}
