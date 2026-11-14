//! Volume Shader BM — порт https://cznull.github.io/vsbm на Rust для Android.
//!
//! Оригинал — одна HTML-страница с WebGL: фрагментный шейдер трассирует
//! мандельбалб в холст 1024x1024, который растягивается на квадрат экрана.
//! Здесь то же самое через NativeActivity + EGL + OpenGL ES 2.0.

#![cfg(target_os = "android")]

mod gl;

use android_activity::input::{InputEvent, MotionAction, MotionEvent};
use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};
use gl::*;
use std::ffi::{c_void, CString};
use std::ptr;
use std::time::{Duration, Instant};

const VSHADER: &str = include_str!("shaders/main.vert");
const FSHADER: &str = include_str!("shaders/main.frag");
const KERNEL: &str = include_str!("shaders/kernel.frag");
const BLIT_VS: &str = include_str!("shaders/blit.vert");
const BLIT_FS: &str = include_str!("shaders/blit.frag");

/// Размер холста оригинала: нагрузка на GPU не зависит от экрана.
const CANVAS: i32 = 1024;

fn log(msg: &str) {
    let tag = CString::new("vsbm").unwrap();
    let text = CString::new(msg.replace('\0', " ")).unwrap();
    unsafe { __android_log_write(4, tag.as_ptr(), text.as_ptr()) };
}

/// Камера и состояние жестов — те же переменные, что в скрипте страницы.
struct Camera {
    len: f64,
    ang1: f64,
    ang2: f64,
    cenx: f64,
    ceny: f64,
    cenz: f64,
    mx: f64,
    my: f64,
    mx1: f64,
    my1: f64,
    lasttimen: usize,
}

impl Camera {
    fn new() -> Self {
        Camera {
            len: 1.6,
            ang1: 2.8,
            ang2: 0.4,
            cenx: 0.0,
            ceny: 0.0,
            cenz: 0.0,
            mx: 0.0,
            my: 0.0,
            mx1: 0.0,
            my1: 0.0,
            lasttimen: 0,
        }
    }

    /// touchstart / touchend: запомнить положение оставшихся пальцев.
    fn touch_set(&mut self, pts: &[(f64, f64)]) {
        if pts.len() == 1 {
            (self.mx, self.my) = pts[0];
        } else if pts.len() == 2 {
            (self.mx, self.my) = pts[0];
            (self.mx1, self.my1) = pts[1];
        }
        self.lasttimen = pts.len();
    }

    /// touchmove: один палец вращает, два — двигают центр и масштабируют.
    /// `side` — сторона квадрата в тех же единицах, что и координаты (cx == cy).
    fn touch_move(&mut self, pts: &[(f64, f64)], side: f64) {
        let n = pts.len();
        if n == 1 && self.lasttimen == 1 {
            let (x, y) = pts[0];
            self.ang1 += (x - self.mx) * 0.002;
            self.ang2 += (y - self.my) * 0.002;
            self.mx = x;
            self.my = y;
        } else if n == 2 {
            let (x0, y0) = pts[0];
            let (x1, y1) = pts[1];
            let l = self.len * 2.0 / (side + side);
            let dx = x0 + x1 - self.mx - self.mx1;
            let dy = y0 + y1 - self.my - self.my1;
            self.cenx += l * (-dx * self.ang1.sin() - dy * self.ang2.sin() * self.ang1.cos());
            self.ceny += l * (dy * self.ang2.cos());
            self.cenz += l * (dx * self.ang1.cos() - dy * self.ang2.sin() * self.ang1.sin());
            let l1 = ((self.mx - self.mx1).powi(2) + (self.my - self.my1).powi(2) + 1.0).sqrt();
            self.mx = x0;
            self.my = y0;
            self.mx1 = x1;
            self.my1 = y1;
            let l2 = ((self.mx - self.mx1).powi(2) + (self.my - self.my1).powi(2) + 1.0).sqrt();
            self.len *= l1 / l2;
        }
        self.lasttimen = n;
    }
}

unsafe fn compile(kind: GLenum, src: &str) -> Result<GLuint, String> {
    let sh = glCreateShader(kind);
    let p = src.as_ptr() as *const GLchar;
    let l = src.len() as GLint;
    glShaderSource(sh, 1, &p, &l);
    glCompileShader(sh);
    let mut ok = 0;
    glGetShaderiv(sh, GL_COMPILE_STATUS, &mut ok);
    if ok == 0 {
        let mut buf = vec![0u8; 4096];
        let mut n = 0;
        glGetShaderInfoLog(sh, buf.len() as GLsizei, &mut n, buf.as_mut_ptr() as *mut GLchar);
        return Err(String::from_utf8_lossy(&buf[..n.max(0) as usize]).into_owned());
    }
    Ok(sh)
}

unsafe fn link(vs: &str, fs: &str) -> Result<GLuint, String> {
    let v = compile(GL_VERTEX_SHADER, vs)?;
    let f = compile(GL_FRAGMENT_SHADER, fs)?;
    let prog = glCreateProgram();
    glAttachShader(prog, v);
    glAttachShader(prog, f);
    glBindAttribLocation(prog, 0, c"position".as_ptr());
    glLinkProgram(prog);
    let mut ok = 0;
    glGetProgramiv(prog, GL_LINK_STATUS, &mut ok);
    if ok == 0 {
        let mut buf = vec![0u8; 4096];
        let mut n = 0;
        glGetProgramInfoLog(prog, buf.len() as GLsizei, &mut n, buf.as_mut_ptr() as *mut GLchar);
        return Err(String::from_utf8_lossy(&buf[..n.max(0) as usize]).into_owned());
    }
    Ok(prog)
}

/// Всё, что живёт вместе с GL-контекстом.
struct Scene {
    prog: GLuint,
    blit: GLuint,
    fbo: GLuint,
    tex: GLuint,
    u_right: GLint,
    u_forward: GLint,
    u_up: GLint,
    u_origin: GLint,
    u_x: GLint,
    u_y: GLint,
    u_len: GLint,
}

impl Scene {
    unsafe fn new() -> Result<Scene, String> {
        let prog = link(VSHADER, &format!("{FSHADER}{KERNEL}"))?;
        let blit = link(BLIT_VS, BLIT_FS)?;

        let positions: [f32; 18] = [
            -1.0, -1.0, 0.0, 1.0, -1.0, 0.0, 1.0, 1.0, 0.0, -1.0, -1.0, 0.0, 1.0, 1.0, 0.0, -1.0,
            1.0, 0.0,
        ];
        let mut buffer = 0;
        glGenBuffers(1, &mut buffer);
        glBindBuffer(GL_ARRAY_BUFFER, buffer);
        glBufferData(
            GL_ARRAY_BUFFER,
            std::mem::size_of_val(&positions) as isize,
            positions.as_ptr() as *const c_void,
            GL_STATIC_DRAW,
        );
        glVertexAttribPointer(0, 3, GL_FLOAT, 0, 0, ptr::null());
        glEnableVertexAttribArray(0);

        // Холст 1024x1024, как <canvas> на странице.
        let mut tex = 0;
        glGenTextures(1, &mut tex);
        glBindTexture(GL_TEXTURE_2D, tex);
        glTexImage2D(
            GL_TEXTURE_2D,
            0,
            GL_RGBA as GLint,
            CANVAS,
            CANVAS,
            0,
            GL_RGBA,
            GL_UNSIGNED_BYTE,
            ptr::null(),
        );
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR as GLint);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR as GLint);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE as GLint);
        glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE as GLint);
        let mut fbo = 0;
        glGenFramebuffers(1, &mut fbo);
        glBindFramebuffer(GL_FRAMEBUFFER, fbo);
        glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D, tex, 0);
        if glCheckFramebufferStatus(GL_FRAMEBUFFER) != GL_FRAMEBUFFER_COMPLETE {
            return Err("framebuffer incomplete".into());
        }
        glBindFramebuffer(GL_FRAMEBUFFER, 0);

        let u = |name: &std::ffi::CStr| glGetUniformLocation(prog, name.as_ptr());
        Ok(Scene {
            prog,
            blit,
            fbo,
            tex,
            u_right: u(c"right"),
            u_forward: u(c"forward"),
            u_up: u(c"up"),
            u_origin: u(c"origin"),
            u_x: u(c"x"),
            u_y: u(c"y"),
            u_len: u(c"len"),
        })
    }

    /// draw() оригинала в холст + вывод холста квадратом по центру экрана.
    unsafe fn draw(&self, cam: &Camera, w: i32, h: i32, fps: u32, dp: f32) {
        let (a1, a2, len) = (cam.ang1, cam.ang2, cam.len);
        glBindFramebuffer(GL_FRAMEBUFFER, self.fbo);
        glViewport(0, 0, CANVAS, CANVAS);
        glUseProgram(self.prog);
        // cx == cy, поэтому x = y = cx*2/(cx+cy) = 1
        glUniform1f(self.u_x, 1.0);
        glUniform1f(self.u_y, 1.0);
        glUniform1f(self.u_len, len as f32);
        glUniform3f(
            self.u_origin,
            (len * a1.cos() * a2.cos() + cam.cenx) as f32,
            (len * a2.sin() + cam.ceny) as f32,
            (len * a1.sin() * a2.cos() + cam.cenz) as f32,
        );
        glUniform3f(self.u_right, a1.sin() as f32, 0.0, -a1.cos() as f32);
        glUniform3f(
            self.u_up,
            (-a2.sin() * a1.cos()) as f32,
            a2.cos() as f32,
            (-a2.sin() * a1.sin()) as f32,
        );
        glUniform3f(
            self.u_forward,
            (-a1.cos() * a2.cos()) as f32,
            (-a2.sin()) as f32,
            (-a1.sin() * a2.cos()) as f32,
        );
        glDrawArrays(GL_TRIANGLES, 0, 6);

        glBindFramebuffer(GL_FRAMEBUFFER, 0);
        glViewport(0, 0, w, h);
        glClearColor(0.075, 0.067, 0.082, 1.0); // body { background: #131115 }
        glClear(GL_COLOR_BUFFER_BIT);
        let side = w.min(h);
        glViewport((w - side) / 2, (h - side) / 2, side, side);
        glUseProgram(self.blit);
        glBindTexture(GL_TEXTURE_2D, self.tex);
        glDrawArrays(GL_TRIANGLES, 0, 6);

        draw_number(fps, h, dp);
        glFinish();
    }
}

/// Счётчик FPS семисегментными цифрами (scissor + clear) в левом верхнем углу.
unsafe fn draw_number(value: u32, screen_h: i32, dp: f32) {
    // биты: a(верх) b c d(низ) e f g(середина)
    const DIGITS: [u8; 10] = [
        0b0111111, 0b0000110, 0b1011011, 0b1001111, 0b1100110, 0b1101101, 0b1111101, 0b0000111,
        0b1111111, 0b1101111,
    ];
    let t = (3.0 * dp).round().max(1.0) as i32; // толщина сегмента
    let w = (14.0 * dp).round() as i32;
    let hh = (13.0 * dp).round() as i32; // половина высоты
    let pad = (10.0 * dp).round() as i32;
    let top = pad + (28.0 * dp).round() as i32; // ниже строки состояния/выреза
    glEnable(GL_SCISSOR_TEST);
    glClearColor(0.2, 1.0, 0.3, 1.0);
    let text = value.to_string();
    let mut x = pad;
    for ch in text.bytes() {
        let m = DIGITS[(ch - b'0') as usize];
        // прямоугольники в координатах «сверху вниз»: (x, y, w, h)
        let segs = [
            (0, 0, w, t),
            (w - t, 0, t, hh),
            (w - t, hh, t, hh),
            (0, 2 * hh - t, w, t),
            (0, hh, t, hh),
            (0, 0, t, hh),
            (0, hh - t / 2, w, t),
        ];
        for (i, (sx, sy, sw, sh)) in segs.iter().enumerate() {
            if m & (1 << i) != 0 {
                glScissor(x + sx, screen_h - (top + sy + sh), *sw, *sh);
                glClear(GL_COLOR_BUFFER_BIT);
            }
        }
        x += w + 2 * t;
    }
    glDisable(GL_SCISSOR_TEST);
}

/// EGL: дисплей и контекст живут всё время, поверхность — пока есть окно.
struct Egl {
    display: EGLDisplay,
    config: EGLConfig,
    context: EGLContext,
    surface: EGLSurface,
}

impl Egl {
    unsafe fn new() -> Result<Egl, String> {
        let display = eglGetDisplay(ptr::null_mut());
        if display.is_null() || eglInitialize(display, ptr::null_mut(), ptr::null_mut()) == 0 {
            return Err("eglInitialize failed".into());
        }
        let attrs = [
            EGL_RENDERABLE_TYPE,
            EGL_OPENGL_ES2_BIT,
            EGL_SURFACE_TYPE,
            EGL_WINDOW_BIT,
            EGL_RED_SIZE,
            8,
            EGL_GREEN_SIZE,
            8,
            EGL_BLUE_SIZE,
            8,
            EGL_NONE,
        ];
        let mut config: EGLConfig = ptr::null_mut();
        let mut n = 0;
        if eglChooseConfig(display, attrs.as_ptr(), &mut config, 1, &mut n) == 0 || n < 1 {
            return Err("eglChooseConfig failed".into());
        }
        let ctx_attrs = [EGL_CONTEXT_CLIENT_VERSION, 2, EGL_NONE];
        let context = eglCreateContext(display, config, ptr::null_mut(), ctx_attrs.as_ptr());
        if context.is_null() {
            return Err("eglCreateContext failed".into());
        }
        Ok(Egl { display, config, context, surface: ptr::null_mut() })
    }

    unsafe fn attach(&mut self, window: *mut c_void) -> bool {
        self.detach();
        let mut format = 0;
        eglGetConfigAttrib(self.display, self.config, EGL_NATIVE_VISUAL_ID, &mut format);
        ANativeWindow_setBuffersGeometry(window, 0, 0, format);
        self.surface = eglCreateWindowSurface(self.display, self.config, window, ptr::null());
        if self.surface.is_null() {
            return false;
        }
        eglMakeCurrent(self.display, self.surface, self.surface, self.context) != 0
    }

    unsafe fn detach(&mut self) {
        if !self.surface.is_null() {
            eglMakeCurrent(self.display, ptr::null_mut(), ptr::null_mut(), ptr::null_mut());
            eglDestroySurface(self.display, self.surface);
            self.surface = ptr::null_mut();
        }
    }

    unsafe fn size(&self) -> (i32, i32) {
        let (mut w, mut h) = (0, 0);
        eglQuerySurface(self.display, self.surface, EGL_WIDTH, &mut w);
        eglQuerySurface(self.display, self.surface, EGL_HEIGHT, &mut h);
        (w, h)
    }
}

fn points(m: &MotionEvent, skip: Option<usize>, scale: f64) -> Vec<(f64, f64)> {
    (0..m.pointer_count())
        .filter(|i| Some(*i) != skip)
        .map(|i| {
            let p = m.pointer_at_index(i);
            (p.x() as f64 / scale, p.y() as f64 / scale)
        })
        .collect()
}

#[no_mangle]
fn android_main(app: AndroidApp) {
    let mut egl = match unsafe { Egl::new() } {
        Ok(e) => e,
        Err(e) => {
            log(&e);
            return;
        }
    };
    let mut scene: Option<Scene> = None;
    let mut failed = false;
    let mut cam = Camera::new();
    let mut running = true;
    let mut resumed = true;
    let mut frames = 0u32;
    let mut fps = 0u32;
    let mut mark = Instant::now();
    // координаты касаний переводим в «CSS-пиксели», как в браузере
    let dp = app.config().density().map(|d| d as f64 / 160.0).unwrap_or(2.0).max(0.5);
    let mut side_css = 400.0f64;

    while running {
        let drawing = !egl.surface.is_null() && resumed;
        let timeout = if drawing { Duration::ZERO } else { Duration::from_millis(250) };
        app.poll_events(Some(timeout), |event| {
            if let PollEvent::Main(ev) = event {
                match ev {
                    MainEvent::InitWindow { .. } => {
                        if let Some(win) = app.native_window() {
                            let ok = unsafe { egl.attach(win.ptr().as_ptr() as *mut c_void) };
                            if !ok {
                                log("cannot create EGL surface");
                            } else if scene.is_none() && !failed {
                                match unsafe { Scene::new() } {
                                    Ok(s) => scene = Some(s),
                                    Err(e) => {
                                        log(&format!("shader error: {e}"));
                                        failed = true;
                                    }
                                }
                            }
                        }
                    }
                    MainEvent::TerminateWindow { .. } => unsafe { egl.detach() },
                    MainEvent::Resume { .. } => resumed = true,
                    MainEvent::Pause => resumed = false,
                    MainEvent::Destroy => running = false,
                    _ => {}
                }
            }
        });

        if let Ok(mut iter) = app.input_events_iter() {
            loop {
                let more = iter.next(|event| {
                    let InputEvent::MotionEvent(m) = event else {
                        return InputStatus::Unhandled;
                    };
                    match m.action() {
                        MotionAction::Down | MotionAction::PointerDown => {
                            cam.touch_set(&points(m, None, dp));
                        }
                        MotionAction::PointerUp => {
                            cam.touch_set(&points(m, Some(m.pointer_index()), dp));
                        }
                        MotionAction::Up | MotionAction::Cancel => cam.touch_set(&[]),
                        MotionAction::Move => cam.touch_move(&points(m, None, dp), side_css),
                        _ => {}
                    }
                    InputStatus::Handled
                });
                if !more {
                    break;
                }
            }
        }

        if !running || egl.surface.is_null() || !resumed {
            continue;
        }
        unsafe {
            let (w, h) = egl.size();
            if w <= 0 || h <= 0 {
                continue;
            }
            side_css = w.min(h) as f64 / dp;
            match &scene {
                Some(s) => {
                    cam.ang1 += 0.01; // ontimer()
                    s.draw(&cam, w, h, fps, dp as f32);
                }
                None => {
                    // шейдер не собрался — красный экран, подробности в logcat
                    glViewport(0, 0, w, h);
                    glClearColor(0.6, 0.0, 0.0, 1.0);
                    glClear(GL_COLOR_BUFFER_BIT);
                }
            }
            eglSwapBuffers(egl.display, egl.surface);
        }
        frames += 1;
        let dt = mark.elapsed();
        if dt >= Duration::from_secs(1) {
            fps = (frames as f64 / dt.as_secs_f64()).round() as u32;
            frames = 0;
            mark = Instant::now();
        }
    }
    unsafe { egl.detach() };
}
