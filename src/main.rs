//! Alembic - алхимический фабричный тайкун.
//!
//! Запуск: `cargo run --release`
//! Доп. режимы: `--new` (новая игра), `--export-assets <png>` (атлас спрайтов),
//! `--screenshot <png>` (кадр демо-фабрики без окна; `--tab N`, `--codex`, `--tool`),
//! `--smoke` (короткий прогон окна).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod engine;
mod game;
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
}

const TITLE: &str = "Alembic - алхимический тайкун";

/// Собирает показательную фабрику с разными видами построек (для скриншота и самопроверки).
fn demo_world() -> World {
    use Dir::{E, N, S, W};
    let mut w = World::new(1);
    w.money = 1e8;
    w.level = world::FINAL_LEVEL;
    let veins = [(30, 34, 1u8), (34, 31, 2), (30, 44, 3), (36, 36, 4), (50, 36, 1), (44, 36, 1), (50, 44, 2)];
    for (x, y, e) in veins.into_iter().chain([(52, 31, 2), (54, 34, 1)]) {
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
    w.money = 5000.0;
    w
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
        let found: Vec<String> = (0..world::N_KINDS)
            .filter(|i| game.world.discovered[*i])
            .map(|i| world::Item::new((i % 15) as u8 + 1, 1, (i / 15) as u8).name())
            .collect();
        println!("sold={} earned={:.0} found={:?}", game.world.total_sold, game.world.money - money0, found);
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
    let world = loaded.unwrap_or_else(|| World::new(seed));
    let mut game = Game::new(world, Audio::new(!smoke), fresh);
    game.persist = !smoke;
    let updater = if smoke || has("--no-update") { None } else { update::start() };

    let (mut w, mut h) = (1280usize, 720usize);
    let mut window = Window::new(TITLE, w, h, WindowOptions { resize: true, ..WindowOptions::default() })
        .expect("не удалось создать окно");
    window.set_target_fps(60);
    let mut canvas = Image::new(w as i32, h as i32);
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
            wheel: window.get_scroll_wheel().map_or(0.0, |s| s.1),
            pressed: window.get_keys_pressed(KeyRepeat::No),
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
        frames += 1;
        if smoke && frames > 90 {
            println!("smoke ok: {} frames", frames);
            break;
        }
    }
    game.net.leave();
    game.save();
    // Скачанное обновление ставится при выходе; по F5 игра ещё и перезапускается.
    if game.update_ready.is_some() {
        drop(window);
        let _ = update::apply(game.restart);
    }
}
