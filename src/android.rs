//! Alembic на Android: NativeActivity без строчки Java. Кадр рисуется тем же программным
//! рендером в буфер окна, касания переводятся в ввод мыши (см. game/touch.rs).
//!
//! Сборка и установка: scripts/android.sh. На остальных платформах эта библиотека пустая.

#![cfg(target_os = "android")]
#![allow(dead_code)]

mod assets;
mod engine;
mod game;
mod net;
mod world;

use android_activity::input::{Axis, InputEvent, KeyAction, Keycode, MotionAction, Source};
use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent, WindowManagerFlags};
use engine::audio::Audio;
use engine::{btn, Image, Input, Key};
use game::{Game, SAVE_FILE};
use ndk::hardware_buffer_format::HardwareBufferFormat;
use ndk::native_window::NativeWindow;
use std::time::{Duration, Instant};
use world::World;

/// Высота кадра в точках игры: интерфейс рассчитан на 720, ширина берётся по экрану.
const VIEW_H: i32 = 720;
/// Палец должен пролежать столько, чтобы стать нажатием: за это время успевает лечь второй
/// палец, и тогда это жест камеры, а не постройка под первым пальцем.
const PRESS_DELAY: f32 = 0.06;

#[link(name = "log")]
extern "C" {
    fn __android_log_write(prio: i32, tag: *const u8, text: *const u8) -> i32;
}

fn log(text: &str) {
    let mut bytes = text.as_bytes().to_vec();
    bytes.retain(|b| *b != 0);
    bytes.push(0);
    // 4 = ANDROID_LOG_INFO
    unsafe { __android_log_write(4, b"alembic\0".as_ptr(), bytes.as_ptr()) };
}

/// Что сейчас делают пальцы.
#[derive(Clone, Copy, PartialEq)]
enum Gesture {
    Idle,
    /// Палец лёг, но нажатием ещё не стал.
    Pending(f32),
    /// Короткое касание: нажатие уже отдано игре, в следующем кадре - отпускание.
    Tap,
    Down,
    /// Два пальца: камера и масштаб.
    Multi,
    /// После жеста двумя пальцами один ещё на экране: ждём, пока уберут оба.
    Lifted,
}

struct Fingers {
    /// Пальцы на экране: (номер, x, y) в точках игры.
    pts: Vec<(i32, f32, f32)>,
    gesture: Gesture,
    /// Где палец лёг.
    start: (i32, i32),
    cursor: (i32, i32),
    prev: (i32, i32),
    /// Расстояние между пальцами, от которого считается следующий шаг масштаба.
    pinch: f32,
}

impl Fingers {
    /// Ввод за кадр: один палец - мышь с левой кнопкой, два - средняя кнопка и колесо.
    fn frame(&mut self, dt: f32, back: bool) -> Input {
        let n = self.pts.len();
        let first = self.pts.first().map(|p| (p.1 as i32, p.2 as i32));
        let mut inp = Input::default();
        let was = self.gesture;
        self.gesture = match (was, n) {
            (Gesture::Idle, 1) => {
                self.start = first.unwrap();
                Gesture::Pending(0.0)
            }
            (Gesture::Idle, 0) => Gesture::Idle,
            (Gesture::Pending(_), 0) => Gesture::Tap,
            (Gesture::Pending(t), 1) => {
                let p = first.unwrap();
                let far = (p.0 - self.start.0).abs() + (p.1 - self.start.1).abs() > 8;
                if t + dt >= PRESS_DELAY || far {
                    Gesture::Down
                } else {
                    Gesture::Pending(t + dt)
                }
            }
            (Gesture::Tap, _) => {
                if n == 0 {
                    Gesture::Idle
                } else {
                    Gesture::Lifted
                }
            }
            (Gesture::Down, 1) => Gesture::Down,
            (Gesture::Multi, 1) | (Gesture::Lifted, 1) => Gesture::Lifted,
            (_, 0) => Gesture::Idle,
            _ => Gesture::Multi,
        };
        match self.gesture {
            Gesture::Tap => {
                self.cursor = self.start;
                inp.lmb = true;
                inp.lmb_pressed = true;
            }
            Gesture::Down => {
                inp.lmb = true;
                if was != Gesture::Down {
                    // Нажатие приходится на точку, где палец лёг: с неё начинается лента.
                    self.cursor = self.start;
                    inp.lmb_pressed = true;
                } else {
                    self.cursor = first.unwrap();
                    inp.dmx = self.cursor.0 - self.prev.0;
                    inp.dmy = self.cursor.1 - self.prev.1;
                }
            }
            Gesture::Multi => {
                let (a, b) = (self.pts[0], self.pts[1]);
                let mid = (((a.1 + b.1) / 2.0) as i32, ((a.2 + b.2) / 2.0) as i32);
                let dist = ((a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)).sqrt().max(1.0);
                inp.mmb = true;
                if was == Gesture::Multi {
                    inp.dmx = mid.0 - self.prev.0;
                    inp.dmy = mid.1 - self.prev.1;
                    if dist > self.pinch * 1.3 {
                        inp.wheel = 1.0;
                        self.pinch = dist;
                    } else if dist < self.pinch / 1.3 {
                        inp.wheel = -1.0;
                        self.pinch = dist;
                    }
                } else {
                    self.pinch = dist;
                }
                self.cursor = mid;
            }
            _ => {}
        }
        self.prev = self.cursor;
        (inp.mx, inp.my) = self.cursor;
        if back {
            inp.pressed.push(Key::Escape);
        }
        inp
    }
}

/// Окно и размер кадра под него: высота всегда VIEW_H, ширина - по пропорциям экрана.
struct Screen {
    window: NativeWindow,
    /// Размер окна в пикселях экрана и размер кадра в точках игры.
    phys: (i32, i32),
    view: (i32, i32),
}

impl Screen {
    fn new(window: NativeWindow) -> Screen {
        // Сброс возвращает настоящий размер окна, иначе он равен заданному ранее размеру кадра.
        let _ = window.set_buffers_geometry(0, 0, None);
        let phys = (window.width().max(1), window.height().max(1));
        let w = (VIEW_H as i64 * phys.0 as i64 / phys.1 as i64) as i32;
        let view = (w.clamp(1280, 2400) & !1, VIEW_H);
        let _ = window.set_buffers_geometry(view.0, view.1, Some(HardwareBufferFormat::R8G8B8X8_UNORM));
        log(&format!("окно {}x{}, кадр {}x{}", phys.0, phys.1, view.0, view.1));
        Screen { window, phys, view }
    }

    /// Копирует кадр в окно. У игры цвет 0xAARRGGBB, у окна байты идут R, G, B, X.
    fn present(&self, canvas: &Image) {
        let mut buf = match self.window.lock(None) {
            Ok(b) => b,
            Err(_) => return,
        };
        let (w, h) = (buf.width().min(canvas.w as usize), buf.height().min(canvas.h as usize));
        let stride = buf.stride();
        let bits = buf.bits() as *mut u32;
        if bits.is_null() {
            return;
        }
        for y in 0..h {
            let src = &canvas.px[y * canvas.w as usize..y * canvas.w as usize + w];
            let dst = unsafe { std::slice::from_raw_parts_mut(bits.add(y * stride), w) };
            for (d, s) in dst.iter_mut().zip(src) {
                *d = (s & 0xFF00_FF00) | ((s >> 16) & 0xFF) | ((s & 0xFF) << 16);
            }
        }
    }
}

const HAT_MASK: u32 = btn::UP | btn::DOWN | btn::LEFT | btn::RIGHT;

/// Бит кнопки геймпада по коду клавиши Android; 0 - это не кнопка геймпада.
fn pad_bit(k: Keycode) -> u32 {
    match k {
        Keycode::ButtonA | Keycode::DpadCenter => btn::A,
        Keycode::ButtonB => btn::B,
        Keycode::ButtonX => btn::X,
        Keycode::ButtonY => btn::Y,
        Keycode::ButtonL1 => btn::LB,
        Keycode::ButtonR1 => btn::RB,
        Keycode::ButtonL2 => btn::LT,
        Keycode::ButtonR2 => btn::RT,
        Keycode::ButtonSelect => btn::BACK,
        Keycode::ButtonStart | Keycode::ButtonMode => btn::START,
        Keycode::ButtonThumbl => btn::L3,
        Keycode::ButtonThumbr => btn::R3,
        Keycode::DpadUp => btn::UP,
        Keycode::DpadDown => btn::DOWN,
        Keycode::DpadLeft => btn::LEFT,
        Keycode::DpadRight => btn::RIGHT,
        _ => 0,
    }
}

#[no_mangle]
fn android_main(app: AndroidApp) {
    std::panic::set_hook(Box::new(|info| log(&format!("ПАНИКА: {}", info))));
    // Сохранения лежат в личной папке приложения: остальной код пишет файлы по короткому имени.
    if let Some(dir) = app.internal_data_path() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::env::set_current_dir(&dir);
    }
    app.set_window_flags(WindowManagerFlags::FULLSCREEN | WindowManagerFlags::KEEP_SCREEN_ON, WindowManagerFlags::empty());

    let loaded = std::fs::read_to_string(SAVE_FILE).ok().as_deref().and_then(World::load);
    let fresh = loaded.is_none();
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(42, |d| d.as_nanos() as u64);
    let mut game = Game::new(loaded.unwrap_or_else(|| World::new_game(seed)), Audio::new(true), fresh);
    game.touch.on = true;
    game.show_fps = false;
    game.open_library(game::BP_FILE);

    let mut pad = engine::Pad::default();
    let mut screen: Option<Screen> = None;
    let mut canvas = Image::new(1280, VIEW_H);
    let mut fingers = Fingers { pts: Vec::new(), gesture: Gesture::Idle, start: (0, 0), cursor: (-9999, 9999), prev: (0, 0), pinch: 1.0 };
    let (mut focused, mut quit) = (true, false);
    let mut last = Instant::now();
    let (mut fps_frames, mut fps_t) = (0u32, Instant::now());

    while !quit {
        // Без окна рисовать некуда: ждём события, не тратя батарею.
        let active = screen.is_some() && focused;
        let timeout = if active { Duration::ZERO } else { Duration::from_millis(250) };
        let mut resized = false;
        app.poll_events(Some(timeout), |ev| {
            if let PollEvent::Main(ev) = ev {
                match ev {
                    MainEvent::InitWindow { .. } => resized = true,
                    MainEvent::WindowResized { .. } | MainEvent::ConfigChanged { .. } => resized = true,
                    MainEvent::TerminateWindow { .. } => screen = None,
                    MainEvent::GainedFocus => focused = true,
                    MainEvent::LostFocus => focused = false,
                    // Приложение уходит с экрана: система может закрыть его без предупреждения.
                    MainEvent::Pause | MainEvent::Stop => game.save(),
                    MainEvent::Destroy => quit = true,
                    _ => {}
                }
            }
        });
        if resized {
            screen = app.native_window().map(Screen::new);
            if let Some(s) = &screen {
                canvas = Image::new(s.view.0, s.view.1);
            }
        }

        // Касания и кнопка "назад".
        let mut back = false;
        let mut pad_axes: Option<[f32; 8]> = None;
        let scale = screen.as_ref().map_or((1.0, 1.0), |s| (s.view.0 as f32 / s.phys.0 as f32, s.view.1 as f32 / s.phys.1 as f32));
        if let Ok(mut events) = app.input_events_iter() {
            loop {
                let more = events.next(|ev| match ev {
                    InputEvent::MotionEvent(m) if m.source() == Source::Joystick => {
                        if let Some(p) = m.pointers().next() {
                            let a = |x| p.axis_value(x);
                            pad_axes = Some([
                                a(Axis::X),
                                a(Axis::Y),
                                a(Axis::Z),
                                a(Axis::Rz),
                                a(Axis::Ltrigger).max(a(Axis::Brake)),
                                a(Axis::Rtrigger).max(a(Axis::Gas)),
                                a(Axis::HatX),
                                a(Axis::HatY),
                            ]);
                        }
                        InputStatus::Handled
                    }
                    InputEvent::KeyEvent(k) if pad_bit(k.key_code()) != 0 => {
                        let bit = pad_bit(k.key_code());
                        match k.action() {
                            KeyAction::Down => pad.held |= bit,
                            KeyAction::Up => pad.held &= !bit,
                            _ => {}
                        }
                        pad.connected = true;
                        InputStatus::Handled
                    }
                    InputEvent::MotionEvent(m) => {
                        let gone = match m.action() {
                            MotionAction::Up | MotionAction::Cancel => Some(usize::MAX),
                            MotionAction::PointerUp => Some(m.pointer_index()),
                            _ => None,
                        };
                        fingers.pts.clear();
                        if gone != Some(usize::MAX) {
                            for p in m.pointers() {
                                if Some(p.pointer_index()) != gone {
                                    fingers.pts.push((p.pointer_id(), p.x() * scale.0, p.y() * scale.1));
                                }
                            }
                        }
                        InputStatus::Handled
                    }
                    InputEvent::KeyEvent(k) if k.key_code() == Keycode::Back => {
                        if k.action() == KeyAction::Up {
                            back = true;
                        }
                        InputStatus::Handled
                    }
                    _ => InputStatus::Unhandled,
                });
                if !more {
                    break;
                }
            }
        }

        let now = Instant::now();
        let dt = (now - last).as_secs_f32().min(0.1);
        last = now;
        let s = match &screen {
            Some(s) if focused => s,
            _ => continue,
        };
        fps_frames += 1;
        let span = (now - fps_t).as_secs_f32();
        if span >= 0.5 {
            game.fps = fps_frames as f32 / span;
            game.frame_ms = span * 1000.0 / fps_frames as f32;
            fps_frames = 0;
            fps_t = now;
        }
        if let Some(a) = pad_axes {
            pad.connected = true;
            (pad.lx, pad.ly, pad.rx, pad.ry, pad.lt, pad.rt) = (a[0], a[1], a[2], a[3], a[4], a[5]);
            // Крестовина у многих пультов приходит осями, а не кнопками.
            let hat = [(a[7] < -0.5, btn::UP), (a[7] > 0.5, btn::DOWN), (a[6] < -0.5, btn::LEFT), (a[6] > 0.5, btn::RIGHT)];
            pad.held &= !(HAT_MASK << 16);
            for (on, b) in hat {
                if on {
                    pad.held |= b << 16;
                }
            }
        }
        let mut inp = fingers.frame(dt, back);
        inp.pad = pad;
        // Крестовина-оси хранится в старших битах, игре отдаётся вместе с кнопками.
        inp.pad.held = (pad.held & 0xFFFF) | (pad.held >> 16);
        let (lt, rt) = (pad.lt > 0.5, pad.rt > 0.5);
        inp.pad.held |= if lt { btn::LT } else { 0 } | if rt { btn::RT } else { 0 };
        game.update(&inp, dt, s.view.0, s.view.1);
        game.draw(&mut canvas);
        s.present(&canvas);
        // Экран может обновляться и 120 раз в секунду, игре хватает 60.
        let spent = now.elapsed();
        if spent < Duration::from_millis(15) {
            std::thread::sleep(Duration::from_millis(15) - spent);
        }
    }
    game.net.leave();
    game.save();
}
