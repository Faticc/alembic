//! Alembic - алхимический фабричный тайкун.
//!
//! Запуск: `cargo run --release`
//! Доп. режимы: `--new` (новая игра), `--export-assets <png>` (атлас спрайтов),
//! `--screenshot <png>` (кадр демо-фабрики без окна; `--tab N`, `--codex`, `--tool`),
//! `--demo06 <png>` (кадр сцены обновления 0.6, см. demo06_shot), `--smoke` (короткий прогон окна).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod engine;
mod game;
mod megagen;
mod megagen3;
mod net;
mod update;
mod world;

use engine::audio::Audio;
use engine::{Image, Input, Key};
use game::{Game, SAVE_FILE};
use minifb::{KeyRepeat, MouseButton, MouseMode, Window, WindowOptions};
use world::{Dir, Kind, World};

#[link(name = "user32")]
extern "system" {
    /// minifb не знает боковых кнопок мыши, поэтому спрашиваем Windows напрямую.
    fn GetAsyncKeyState(vk: i32) -> i16;
    /// Размер основного монитора для полноэкранного режима.
    fn GetSystemMetrics(index: i32) -> i32;
}

/// Настройки окна, которые помнятся между запусками (отдельно от сохранения мира).
const SETTINGS_FILE: &str = "alembic_settings.txt";

fn load_settings() -> (bool, bool) {
    let text = std::fs::read_to_string(SETTINGS_FILE).unwrap_or_default();
    let flag = |name: &str, def: bool| {
        text.lines()
            .filter_map(|l| l.strip_prefix(name)?.trim().parse::<u8>().ok())
            .last()
            .map_or(def, |v| v != 0)
    };
    (flag("fullscreen", false), flag("fps", true))
}

fn save_settings(fullscreen: bool, fps: bool) {
    let _ = std::fs::write(SETTINGS_FILE, format!("fullscreen {}\nfps {}\n", fullscreen as u8, fps as u8));
}

/// Обычное окно с рамкой или окно без рамки во весь основной монитор.
fn open_window(fullscreen: bool) -> Window {
    let (w, h, opts) = if fullscreen {
        // SM_CXSCREEN, SM_CYSCREEN
        let (sw, sh) = unsafe { (GetSystemMetrics(0), GetSystemMetrics(1)) };
        (sw.max(640) as usize, sh.max(360) as usize, WindowOptions { none: true, ..WindowOptions::default() })
    } else {
        (1280, 720, WindowOptions { resize: true, ..WindowOptions::default() })
    };
    let mut window = Window::new(TITLE, w, h, opts).expect("не удалось создать окно");
    if fullscreen {
        window.set_position(0, 0);
    }
    window.set_target_fps(60);
    window.set_input_callback(Box::new(CharSink));
    window
}

/// Знаки, набранные с клавиатуры с прошлого кадра: minifb отдаёт их через обратный вызов.
static TYPED: std::sync::Mutex<Vec<char>> = std::sync::Mutex::new(Vec::new());

struct CharSink;

impl minifb::InputCallback for CharSink {
    fn add_char(&mut self, uni_char: u32) {
        if let (Some(c), Ok(mut buf)) = (char::from_u32(uni_char), TYPED.lock()) {
            buf.push(c);
        }
    }
}

const TITLE: &str = "Alembic - алхимический тайкун";

/// Собирает показательную фабрику с разными видами построек (для скриншота и самопроверки).
fn demo_world() -> World {
    use Dir::{E, N, S, W};
    let mut w = World::new(1);
    w.money = 1e8;
    w.level = world::FINAL_LEVEL;
    let veins = [(30, 34, 1u8), (34, 31, 2), (30, 44, 3), (36, 36, 4), (50, 36, 1), (44, 36, 1), (50, 44, 2)];
    for (x, y, e) in veins.into_iter().chain([(52, 31, 2), (54, 34, 1), (36, 32, 4)]) {
        w.veins[(y * world::W + x) as usize] = e;
    }
    let mut put = |name: &str, x: i32, y: i32, d: Dir| {
        w.place(Kind::by_name(name), x, y, d).expect("demo placement");
    };
    let belt = "КОНВЕЙЕР";
    // Огонь + вода -> пар -> чистый пар -> кристалл -> лавка.
    put("ЭКСТРАКТОР", 30, 34, E);
    for x in 31..34 {
        put(belt, x, 34, E);
    }
    put("ЭКСТРАКТОР", 34, 31, S);
    put(belt, 34, 32, S);
    put(belt, 34, 33, S);
    put("СМЕСИТЕЛЬ", 34, 34, E);
    put(belt, 35, 34, E);
    put("ДИСТИЛЛЯТОР", 36, 34, E);
    put(belt, 37, 34, E);
    put("КРИСТАЛЛИЗАТОР", 38, 34, E);
    put(belt, 39, 34, E);
    for y in 34..39 {
        put(belt, 40, y, S);
    }
    // Земля через делитель двумя путями, эфир пересекает поток по мосту.
    put("ЭКСТРАКТОР", 30, 44, E);
    for x in 31..34 {
        put(belt, x, 44, E);
    }
    put("ДЕЛИТЕЛЬ", 34, 44, E);
    for x in 35..38 {
        put(belt, x, 44, E);
    }
    for y in 41..45 {
        put(belt, 38, y, N);
    }
    put(belt, 38, 40, E);
    for y in 41..44 {
        put(belt, 34, y, N);
    }
    put(belt, 34, 40, E);
    put(belt, 35, 40, E);
    put("МОСТ", 36, 40, N);
    put(belt, 37, 40, E);
    put("ЭКСТРАКТОР", 36, 36, S);
    for y in 37..40 {
        put(belt, 36, y, S);
    }
    for y in 41..44 {
        put(belt, 36, y, S);
    }
    // Зелье + порошок -> эликсир; машины стоят вплотную, дальше экспресс-лента.
    put("ЭКСТРАКТОР", 50, 36, W);
    put("РАЗЛИВОЧНАЯ", 49, 36, W);
    put(belt, 48, 36, W);
    put("ЭКСТРАКТОР", 44, 36, E);
    put("КРИСТАЛЛИЗАТОР", 45, 36, E);
    put("МЕЛЬНИЦА", 46, 36, E);
    put("АЛХИМ. СТОЛ", 47, 36, S);
    for y in 37..40 {
        put(belt, 47, y, S);
    }
    for x in 42..48 {
        put("ЭКСПРЕСС-ЛЕНТА", x, 40, W);
    }
    // Кристалл -> самоцвет.
    put("ЭКСТРАКТОР", 50, 44, W);
    put("КРИСТАЛЛИЗАТОР", 49, 44, W);
    put("ГРАНИЛЬНЯ", 48, 44, W);
    for y in 42..45 {
        put(belt, 47, y, N);
    }
    for x in 42..48 {
        put(belt, x, 41, W);
    }
    // Логистика: склад, фильтр, торговый пост, утилизатор, бур с тупиковой лентой.
    put("ЭКСТРАКТОР", 52, 31, W);
    put(belt, 51, 31, W);
    put("СКЛАД", 50, 31, W);
    put(belt, 49, 31, W);
    put("ФИЛЬТР", 48, 31, W);
    put(belt, 47, 31, W);
    put("ТОРГОВЫЙ ПОСТ", 46, 31, N);
    put(belt, 48, 30, N);
    put("УТИЛИЗАТОР", 48, 29, N);
    put("ГЛУБИННЫЙ БУР", 54, 34, S);
    put(belt, 54, 35, S);
    put(belt, 54, 36, S);
    put("ПЕРЕЛИВ", 53, 39, N);
    put("ОБСЕРВАТОРИЯ", 54, 39, N);
    put("ГОЛЕМНАЯ", 55, 39, N);
    put("ВЕЛИКОЕ ДЕЛАНИЕ", 56, 39, N);
    // Подземный конвейер проходит под двумя лентами и выходит на мост, повёрнутый вдоль потока.
    put("ЭКСТРАКТОР", 36, 32, E);
    put(belt, 37, 32, E);
    put("ПОДЗЕМНЫЙ КОНВЕЙЕР", 38, 32, E);
    put(belt, 39, 32, S);
    put(belt, 40, 32, S);
    put("ПОДЗЕМНЫЙ КОНВЕЙЕР", 41, 32, E);
    put("МОСТ", 42, 32, E);
    put(belt, 43, 32, E);
    put("ТОРГОВЫЙ ПОСТ", 44, 32, N);
    w.money = 5000.0;
    w
}

/// Показательная сцена обновления 0.6: все новые постройки в работе (для кадров самопроверки).
fn demo06_world() -> World {
    use world::{pack_ports, ratio_cfg, sorter_set, tele_cfg, Cond, Item, PORT_AUTO, PORT_IN, PORT_OFF};
    use Dir::{E, N, S};
    let mut w = World::new(1);
    w.veins.iter_mut().for_each(|v| *v = 0);
    w.money = 1e12;
    w.level = world::FINAL_LEVEL;
    for (x, y, e) in [(20, 12, 1u8), (27, 16, 1), (20, 21, 3), (20, 26, 1), (21, 25, 2)] {
        w.veins[(y * world::W + x) as usize] = e;
    }
    fn put(w: &mut World, name: &str, x: i32, y: i32, d: Dir) {
        w.place(Kind::by_name(name), x, y, d).expect("demo placement");
    }
    // Склад-источник: бесконечный запас предмета для показа.
    fn src(w: &mut World, x: i32, y: i32, d: Dir, it: Item) {
        put(w, "СКЛАД", x, y, d);
        let b = w.tiles[(y * world::W + x) as usize].as_mut().unwrap();
        b.slots[0] = Some(it);
        b.count = 100_000;
    }
    let belt = "КОНВЕЙЕР";
    let post = "ТОРГОВЫЙ ПОСТ";
    // 1. Делитель с пропорцией 3:1.
    put(&mut w, "ЭКСТРАКТОР", 20, 12, E);
    put(&mut w, "ДЕЛИТЕЛЬ С ПРОПОРЦИЕЙ", 21, 12, E);
    w.set_cfg(21, 12, ratio_cfg(3, 1));
    for x in 22..25 {
        put(&mut w, belt, x, 12, E);
    }
    put(&mut w, post, 25, 12, N);
    put(&mut w, belt, 21, 13, S);
    put(&mut w, "УТИЛИЗАТОР", 21, 14, N);
    // 2. Дистиллятор со стороной отказа: огонь перегоняется, вода уходит вбок.
    src(&mut w, 20, 16, E, Item::base(0));
    src(&mut w, 21, 15, S, Item::base(1));
    put(&mut w, "ДИСТИЛЛЯТОР", 21, 16, E);
    w.set_cfg(21, 16, 2);
    put(&mut w, belt, 22, 16, E);
    put(&mut w, post, 23, 16, N);
    put(&mut w, belt, 21, 17, S);
    put(&mut w, "УТИЛИЗАТОР", 21, 18, N);
    // 3. Алтарь с магистерием лавы III: скорость и двойной выход машинам вокруг.
    src(&mut w, 28, 13, S, Item::new(5, 3, 25));
    put(&mut w, "АЛТАРЬ", 28, 14, N);
    put(&mut w, "ЭКСТРАКТОР", 27, 16, E);
    put(&mut w, "КРИСТАЛЛИЗАТОР", 28, 16, E);
    put(&mut w, belt, 29, 16, E);
    put(&mut w, post, 30, 16, N);
    // 4. Тигель: слиток огня + слиток воды = слиток пара. Трансмутатор: огонь -> земля.
    src(&mut w, 33, 12, E, Item::new(1, 2, 4));
    src(&mut w, 34, 11, S, Item::new(2, 2, 4));
    put(&mut w, "ТИГЕЛЬ", 34, 12, E);
    put(&mut w, belt, 35, 12, E);
    put(&mut w, post, 36, 12, N);
    src(&mut w, 33, 15, E, Item::base(0));
    put(&mut w, "ТРАНСМУТАТОР", 34, 15, E);
    put(&mut w, belt, 35, 15, E);
    put(&mut w, post, 36, 15, N);
    // 5. Атанор: четыре передатчика на одном коде шлют магистерии по очереди (ровно 1:1:1:1),
    // приёмник выкладывает их на одну ленту. Лишнее уходит в сторону отказа, камень - телепортом в лавку.
    for e in 0..4 {
        src(&mut w, 30 + e, 18, S, Item::new(1 << e, 3, 25));
        put(&mut w, "ТЕЛЕПОРТ-ПЕРЕДАТЧИК", 30 + e, 19, N);
        w.set_cfg(30 + e, 19, tele_cfg(100));
    }
    put(&mut w, "ТЕЛЕПОРТ-ПРИЁМНИК", 31, 21, E);
    w.set_cfg(31, 21, tele_cfg(100));
    put(&mut w, belt, 32, 21, E);
    put(&mut w, belt, 33, 21, E);
    put(&mut w, "АТАНОР", 34, 21, E);
    w.set_ports(34, 21, pack_ports([PORT_AUTO, PORT_OFF, PORT_IN, PORT_OFF]));
    w.set_cfg(34, 21, 2);
    put(&mut w, "УТИЛИЗАТОР", 34, 22, N);
    put(&mut w, belt, 35, 21, E);
    put(&mut w, "ТЕЛЕПОРТ-ПЕРЕДАТЧИК", 36, 21, N);
    w.set_cfg(36, 21, tele_cfg(7));
    put(&mut w, "ТЕЛЕПОРТ-ПРИЁМНИК", 38, 40, E);
    w.set_cfg(38, 40, tele_cfg(7));
    // 6. Упаковщик -> телепорт 0042 -> два приёмника по очереди: распаковщик и прямо в лавку.
    put(&mut w, "ЭКСТРАКТОР", 20, 21, E);
    w.force_level(20, 21, 5);
    put(&mut w, "УПАКОВЩИК", 21, 21, E);
    put(&mut w, belt, 22, 21, E);
    put(&mut w, "ТЕЛЕПОРТ-ПЕРЕДАТЧИК", 23, 21, N);
    w.set_cfg(23, 21, tele_cfg(42));
    put(&mut w, "ТЕЛЕПОРТ-ПРИЁМНИК", 27, 22, E);
    w.set_cfg(27, 22, tele_cfg(42));
    put(&mut w, belt, 28, 22, E);
    put(&mut w, "РАСПАКОВЩИК", 29, 22, E);
    put(&mut w, belt, 30, 22, E);
    put(&mut w, post, 31, 22, N);
    put(&mut w, "ТЕЛЕПОРТ-ПРИЁМНИК", 38, 39, E);
    w.set_cfg(38, 39, tele_cfg(42));
    // 7. Сортировочный узел: огонь вперёд, вода вправо, остальное влево. За ним фильтр по чистоте.
    put(&mut w, "ЭКСТРАКТОР", 20, 26, E);
    put(&mut w, "ЭКСТРАКТОР", 21, 25, S);
    for x in 21..24 {
        put(&mut w, belt, x, 26, E);
    }
    put(&mut w, "СОРТИРОВОЧНЫЙ УЗЕЛ", 24, 26, E);
    let fire = Cond { mask: 1, ..Cond::default() };
    w.set_cfg(24, 26, sorter_set(sorter_set(0, 0, fire), 1, Cond { mask: 2, ..Cond::default() }));
    put(&mut w, belt, 25, 26, E);
    put(&mut w, "ФИЛЬТР", 26, 26, E);
    w.set_cfg(26, 26, Cond { purity: 2, form: 1, ..Cond::default() }.bits());
    put(&mut w, belt, 27, 26, E);
    put(&mut w, post, 28, 26, N);
    put(&mut w, belt, 26, 27, S);
    put(&mut w, post, 26, 28, N);
    put(&mut w, belt, 24, 27, S);
    put(&mut w, post, 24, 28, N);
    // 8. Логика: таймер открывает и закрывает ворота на ленте; датчик считает предметы,
    // каждый пятый через счётчик включает защёлку, а та (через НЕ) держит машину остановленной.
    put(&mut w, "ТАЙМЕР", 22, 10, S);
    w.set_lparam(22, 10, 40);
    put(&mut w, "ПРОВОД", 22, 11, N);
    put(&mut w, "ИСПОЛНИТЕЛЬ", 22, 12, N);
    put(&mut w, "ДАТЧИК ПРЕДМЕТА", 24, 12, N);
    put(&mut w, "СЧЁТЧИК", 24, 11, N);
    w.set_lparam(24, 11, 5);
    put(&mut w, "ЗАЩЁЛКА", 24, 10, E);
    for x in 25..29 {
        put(&mut w, "ПРОВОД", x, 10, N);
    }
    for y in 11..16 {
        put(&mut w, "ПРОВОД", 28, y, N);
    }
    put(&mut w, "ИСПОЛНИТЕЛЬ", 28, 16, N);
    put(&mut w, "ДАТЧИК ЗАТОРА", 29, 16, E);
    put(&mut w, "ВЕНТИЛЬ ИЛИ", 30, 15, N);
    put(&mut w, "ВЕНТИЛЬ НЕ", 30, 14, N);
    put(&mut w, "ВЕНТИЛЬ И", 30, 13, N);
    w.rep = [4, 7, 1, 10];
    w.money = 2.5e7;
    w
}

/// Кадр сцены 0.6: `--cam X Y`, `--hover X Y` (тайл под курсором), `--panel X Y` (окно настройки),
/// `--codex N` (каталог чистоты N), `--stats`, `--tab N`, `--tool ИМЯ`, `--secs N`, `--fps N`.
fn demo06_shot(path: &str, args: &[String]) {
    let nums = |flag: &str| -> Option<Vec<f32>> {
        let i = args.iter().position(|a| a == flag)?;
        Some(args[i + 1..].iter().take(2).map_while(|v| v.parse().ok()).collect())
    };
    let has = |flag: &str| args.iter().any(|a| a == flag);
    let mut game = Game::new(demo06_world(), Audio::new(false), false);
    game.persist = false;
    let cam = nums("--cam").filter(|v| v.len() == 2).unwrap_or(vec![28.0, 16.0]);
    game.cam_x = cam[0];
    game.cam_y = cam[1];
    game.tab = nums("--tab").map_or(5, |v| v[0] as usize);
    let fps = nums("--fps").map_or(60, |v| v[0] as u32);
    let secs = nums("--secs").map_or(90, |v| v[0] as u32);
    let mut inp = Input { mx: 5, my: 5, ..Default::default() };
    for _ in 0..secs * fps {
        game.update(&inp, 1.0 / fps as f32, 1280, 720);
    }
    if let Some(h) = nums("--hover").filter(|v| v.len() == 2) {
        inp.mx = 640 + ((h[0] + 0.5 - cam[0]) * 48.0) as i32;
        inp.my = 360 + ((h[1] + 0.5 - cam[1]) * 48.0) as i32;
    }
    if let Some(name) = args.iter().position(|a| a == "--tool").and_then(|i| args.get(i + 1)) {
        game.tool = Some(Kind::by_name(name));
    }
    if let Some(p) = nums("--panel").filter(|v| v.len() == 2) {
        game.show_cfg = Some((p[0] as i32, p[1] as i32));
        game.cfg_tab = nums("--slot").map_or(0, |v| v[0] as usize);
    }
    if let Some(p) = nums("--codex") {
        game.show_codex = true;
        game.codex_p = p.first().map_or(1, |v| *v as u8);
    }
    game.show_stats = has("--stats");
    game.show_wires = has("--wires");
    game.show_guild = has("--guild");
    game.show_great = has("--great");
    if has("--library") {
        // Для кадра библиотеки: два чертежа из показательной фабрики (второй - со схемой логики).
        game.world.money = 1e9;
        for (name, sel) in [("ЛИНИЯ СОРТИРОВКИ", (24, 26, 28, 28)), ("ВОРОТА ПО ТАЙМЕРУ", (22, 10, 30, 16))] {
            game.sel = Some(sel);
            game.copy_selection(false);
            inp.typed = name.chars().collect();
            game.show_bp = true;
            game.update(&inp, 1.0 / fps as f32, 1280, 720);
            game.bp_save();
        }
        inp.typed = "МОЯ ФАБРИКА".chars().collect();
        game.pasting = false;
        inp.mx = 500;
        inp.my = 250;
    }
    if has("--great") {
        // Для кадра витража: часть каталога уже сдана (первые формы целиком и россыпь дальше).
        for id in 0..world::N_ITEM_IDS {
            if id < 300 || id % 7 == 0 || id % 11 == 3 {
                game.world.great[id] = true;
            }
        }
    }
    if has("--guild") {
        let _ = game.world.take_order(0);
        game.world.set_skin(2);
    }
    game.fps = fps as f32;
    game.frame_ms = 1000.0 / fps as f32;
    game.update(&inp, 1.0 / fps as f32, 1280, 720);
    let mut canvas = Image::new(1280, 720);
    game.draw(&mut canvas);
    engine::png::save_png(path, &canvas, true).expect("save screenshot");
    let w = &game.world;
    let stones = (1..=3u8).filter(|p| w.discovered[world::Item::new(15, *p, world::FORM_STONE).id()]).count();
    println!(
        "{} к/с, {} с: продано {} шт, каталог {}/{}, камней в каталоге {}, деньги {:.0}",
        fps, secs, w.total_sold, w.catalog_found(), world::N_ITEM_IDS, stones, w.money
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let arg_after = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let has = |flag: &str| args.iter().any(|a| a == flag);

    if let Some(path) = arg_after("--export-assets") {
        let atlas = assets::Assets::generate().atlas();
        engine::png::save_png(&path, &atlas, false).expect("save atlas");
        return;
    }

    if let Some(path) = arg_after("--demo06") {
        demo06_shot(&path, &args);
        return;
    }

    if let Some(path) = arg_after("--screenshot") {
        let mut game = Game::new(demo_world(), Audio::new(false), false);
        game.persist = false;
        game.cam_x = 43.0;
        game.cam_y = 37.5;
        game.tab = arg_after("--tab").and_then(|t| t.parse().ok()).unwrap_or(0);
        if let Some(l) = arg_after("--level").and_then(|t| t.parse().ok()) {
            game.world.level = l;
        }
        game.world.money = 1e6;
        let _ = game.world.level_up(34, 34);
        let _ = game.world.level_up(34, 34);
        let _ = game.world.level_up(36, 34);
        game.world.money = 5000.0;
        let mut inp = Input { mx: 150, my: 420, ..Default::default() };
        let money0 = game.world.money;
        for _ in 0..60 * 60 {
            game.update(&inp, 1.0 / 60.0, 1280, 720);
        }
        if has("--codex") {
            inp.pressed.push(Key::C);
            inp.mx = 500;
            inp.my = 240;
        }
        game.show_net = has("--netmenu");
        game.show_help = has("--helpscreen");
        game.fps = 60.0;
        game.frame_ms = 16.7;
        if has("--peers") {
            // Самопроверка отрисовки других игроков: один выделяет, второй вставляет, третий строит.
            let p = |x, y, tool, mode, rect| net::Presence { x, y, tool, dir: 1, mode, rect };
            game.net.fake_peer(1, "Fatic", p(37, 31, 255, 1, (33, 30, 37, 31)));
            game.net.fake_peer(2, "Drug", p(47, 33, 255, 2, (45, 32, 49, 34)));
            game.net.fake_peer(3, "Tretiy", p(42, 43, 2, 0, (0, 0, 0, 0)));
            for (pl, ms) in game.net.peers.iter_mut().zip([34.0, 118.0, 260.0]) {
                pl.ping_ms = Some(ms);
            }
            game.net.mode = net::Mode::Host;
        }
        if has("--ports") {
            // Самопроверка сторон: у смесителя выход вправо, у делителя закрыта левая сторона.
            use world::{pack_ports, PORT_AUTO, PORT_IN, PORT_OFF, PORT_OUT};
            game.world.set_ports(34, 34, pack_ports([PORT_IN, PORT_OUT, PORT_AUTO, PORT_AUTO]));
            game.world.set_ports(34, 44, pack_ports([PORT_AUTO, PORT_AUTO, PORT_AUTO, PORT_OFF]));
            game.world.set_ports(46, 36, pack_ports([PORT_AUTO, PORT_AUTO, PORT_AUTO, PORT_OFF]));
            inp.mx = 950;
            inp.my = 312;
        }
        if has("--select") || has("--paste") {
            // Самопроверка выделения: рамка вокруг цепочки эликсира, копия и вставка в пустое место.
            game.sel = Some((44, 36, 50, 40));
            if has("--paste") {
                let count = |g: &Game| g.world.tiles.iter().flatten().count();
                let before = count(&game);
                game.world.money = 1e6;
                game.copy_selection(false);
                game.rotate_clip();
                let placed = game.paste_at(36, 26);
                println!("paste: placed={} buildings {} -> {}", placed, before, count(&game));
                inp.mx = 760;
                inp.my = 250;
            }
        }
        if has("--tool") {
            inp.pressed.push(Key::Key3);
        }
        game.update(&inp, 1.0 / 60.0, 1280, 720);
        let mut canvas = Image::new(1280, 720);
        game.draw(&mut canvas);
        engine::png::save_png(&path, &canvas, true).expect("save screenshot");
        let found: Vec<String> = (0..world::N_ALL_IDS)
            .filter(|i| game.world.discovered[*i])
            .map(|i| world::Item::from_id(i).name())
            .collect();
        println!("sold={} earned={:.0} found={:?}", game.world.total_sold, game.world.money - money0, found);
        return;
    }

    // Скрытые режимы мегафабрики (см. megagen.rs).
    if let Some(path) = arg_after("--gen-megasave") {
        megagen::generate(&path, &args);
        return;
    }
    if let Some(path) = arg_after("--verify-megasave") {
        megagen::verify(&path, &args);
        return;
    }
    if let Some(path) = arg_after("--gen-megasave3") {
        megagen3::generate(&path, &args);
        return;
    }
    if let Some(path) = arg_after("--verify-megasave3") {
        megagen3::verify(&path, &args);
        return;
    }
    if has("--mega3-lab") {
        megagen3::lab(&args);
        return;
    }
    if let Some(path) = arg_after("--megastats") {
        megagen::stats(&path);
        return;
    }
    if let Some(path) = arg_after("--megashot") {
        megagen::shot(&path, &arg_after("--load").unwrap_or_else(|| "megafactory_save.txt".to_string()));
        return;
    }

    if let Some(path) = arg_after("--checksave") {
        // Читает сохранение и печатает сводку, ничего не меняя на диске.
        match std::fs::read_to_string(&path).ok().as_deref().and_then(World::load) {
            Some(w) => println!(
                "save ok: money={:.0} level={} upgrades={:?} buildings={} found={}",
                w.money,
                w.level,
                w.upgrades,
                w.tiles.iter().flatten().count() - 9,
                w.discovered.iter().filter(|d| **d).count()
            ),
            None => println!("save FAILED to load: {}", path),
        }
        return;
    }

    if has("--update-check") {
        // Самопроверка автообновления: проверить, скачать, подменить exe и выйти.
        update::cleanup();
        match update::check_and_download() {
            Ok(Some(tag)) => println!("update {} downloaded, apply: {:?}", tag, update::apply(false)),
            Ok(None) => println!("up to date ({})", update::VERSION),
            Err(e) => println!("update check failed: {}", e),
        }
        return;
    }

    if has("--nettest") {
        // Самопроверка сети: подключиться к Steam, создать комнату и найти её в списке.
        let mut net = net::Net::new();
        println!("steam ok: {} / {}", net.ensure(), net.status);
        net.host_game();
        let mut spin = |net: &mut net::Net, secs: u32| {
            for _ in 0..secs * 20 {
                net.poll(0.05);
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        };
        spin(&mut net, 4);
        println!("after host: mode={:?} status={}", net.mode, net.status);
        net.refresh();
        spin(&mut net, 4);
        let names: Vec<String> = net.lobbies.iter().map(|l| format!("{} ({})", l.name, l.members)).collect();
        println!("lobbies: {:?} status={}", names, net.status);
        net.leave();
        spin(&mut net, 1);
        return;
    }

    let smoke = has("--smoke");
    let saved = if has("--new") || smoke { None } else { std::fs::read_to_string(SAVE_FILE).ok() };
    let loaded = saved.as_deref().and_then(World::load);
    let fresh = loaded.is_none();
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(42);
    let world = loaded.unwrap_or_else(|| World::new_game(seed));
    let mut game = Game::new(world, Audio::new(!smoke), fresh);
    game.persist = !smoke;
    if !smoke {
        game.open_library(game::BP_FILE);
    }
    let updater = if smoke || has("--no-update") { None } else { update::start() };

    let (mut fullscreen, show_fps) = load_settings();
    if has("--fullscreen") {
        fullscreen = true;
    }
    if has("--windowed") {
        fullscreen = false;
    }
    game.show_fps = show_fps;
    let mut window = open_window(fullscreen);
    let (mut w, mut h) = window.get_size();
    let mut canvas = Image::new(w as i32, h as i32);
    // Счётчик кадров: сколько кадров прошло за последние полсекунды.
    let (mut fps_frames, mut fps_t) = (0u32, std::time::Instant::now());
    let mut last = std::time::Instant::now();
    let (mut pl, mut pr) = (false, false);
    let (mut px1, mut px2) = (false, false);
    let (mut pmx, mut pmy) = (0, 0);
    let mut frames = 0u32;

    while window.is_open() {
        let (nw, nh) = window.get_size();
        if nw < 16 || nh < 16 {
            window.update();
            continue;
        }
        if (nw, nh) != (w, h) {
            w = nw;
            h = nh;
            canvas = Image::new(w as i32, h as i32);
        }
        let now = std::time::Instant::now();
        let dt = (now - last).as_secs_f32().min(0.1);
        last = now;
        fps_frames += 1;
        let span = (now - fps_t).as_secs_f32();
        if span >= 0.5 {
            game.fps = fps_frames as f32 / span;
            game.frame_ms = span * 1000.0 / fps_frames as f32;
            fps_frames = 0;
            fps_t = now;
        }

        let (mx, my) = window.get_mouse_pos(MouseMode::Pass).unwrap_or((-1.0, -1.0));
        let (mx, my) = (mx as i32, my as i32);
        let lmb = window.get_mouse_down(MouseButton::Left);
        let rmb = window.get_mouse_down(MouseButton::Right);
        // VK_XBUTTON1 = 5 ("назад"), VK_XBUTTON2 = 6 ("вперёд"); только пока окно активно.
        let active = window.is_active();
        let x1 = active && unsafe { GetAsyncKeyState(5) } < 0;
        let x2 = active && unsafe { GetAsyncKeyState(6) } < 0;
        let down = |a: Key, b: Key| window.is_key_down(a) || window.is_key_down(b);
        let axis = |neg: bool, pos: bool| (pos as i32 - neg as i32) as f32;
        let inp = Input {
            mx,
            my,
            dmx: mx - pmx,
            dmy: my - pmy,
            lmb,
            rmb,
            mmb: window.get_mouse_down(MouseButton::Middle),
            lmb_pressed: lmb && !pl,
            rmb_pressed: rmb && !pr,
            x1_pressed: x1 && !px1,
            x2_pressed: x2 && !px2,
            shift: down(Key::LeftShift, Key::RightShift),
            ctrl: down(Key::LeftCtrl, Key::RightCtrl),
            alt: down(Key::LeftAlt, Key::RightAlt),
            wheel: window.get_scroll_wheel().map_or(0.0, |s| s.1),
            pressed: window.get_keys_pressed(KeyRepeat::No),
            typed: TYPED.lock().map(|mut b| std::mem::take(&mut *b)).unwrap_or_default(),
            pan_x: axis(down(Key::A, Key::Left), down(Key::D, Key::Right)),
            pan_y: axis(down(Key::W, Key::Up), down(Key::S, Key::Down)),
        };
        pl = lmb;
        pr = rmb;
        px1 = x1;
        px2 = x2;
        pmx = mx;
        pmy = my;

        if let Some(tag) = updater.as_ref().and_then(|rx| rx.try_recv().ok()) {
            game.notify_update(tag);
        }
        game.update(&inp, dt, w as i32, h as i32);
        game.draw(&mut canvas);
        if game.restart {
            break;
        }
        if window.update_with_buffer(&canvas.px, w, h).is_err() {
            break;
        }
        if game.toggle_fullscreen {
            // minifb не меняет стиль окна на ходу, поэтому окно пересоздаётся.
            game.toggle_fullscreen = false;
            fullscreen = !fullscreen;
            drop(window);
            window = open_window(fullscreen);
            (pl, pr) = (true, true);
            last = std::time::Instant::now();
            if !smoke {
                save_settings(fullscreen, game.show_fps);
            }
            continue;
        }
        frames += 1;
        if smoke && frames > 90 {
            println!("smoke ok: {} frames", frames);
            break;
        }
    }
    game.net.leave();
    game.save();
    if !smoke {
        save_settings(fullscreen, game.show_fps);
    }
    // Скачанное обновление ставится при выходе; по F5 игра ещё и перезапускается.
    if game.update_ready.is_some() {
        drop(window);
        let _ = update::apply(game.restart);
    }
}
