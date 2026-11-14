//! Минимальные привязки к EGL, OpenGL ES 2.0 и паре функций NDK.
#![allow(non_snake_case, non_camel_case_types, dead_code)]

use std::ffi::{c_char, c_void};

pub type GLenum = u32;
pub type GLuint = u32;
pub type GLint = i32;
pub type GLsizei = i32;
pub type GLfloat = f32;
pub type GLchar = c_char;
pub type GLboolean = u8;

pub const GL_TRIANGLES: GLenum = 0x0004;
pub const GL_COLOR_BUFFER_BIT: GLenum = 0x4000;
pub const GL_SCISSOR_TEST: GLenum = 0x0C11;
pub const GL_TEXTURE_2D: GLenum = 0x0DE1;
pub const GL_UNSIGNED_BYTE: GLenum = 0x1401;
pub const GL_FLOAT: GLenum = 0x1406;
pub const GL_RGBA: GLenum = 0x1908;
pub const GL_LINEAR: GLenum = 0x2601;
pub const GL_TEXTURE_MAG_FILTER: GLenum = 0x2800;
pub const GL_TEXTURE_MIN_FILTER: GLenum = 0x2801;
pub const GL_TEXTURE_WRAP_S: GLenum = 0x2802;
pub const GL_TEXTURE_WRAP_T: GLenum = 0x2803;
pub const GL_CLAMP_TO_EDGE: GLenum = 0x812F;
pub const GL_ARRAY_BUFFER: GLenum = 0x8892;
pub const GL_STATIC_DRAW: GLenum = 0x88E4;
pub const GL_FRAGMENT_SHADER: GLenum = 0x8B30;
pub const GL_VERTEX_SHADER: GLenum = 0x8B31;
pub const GL_COMPILE_STATUS: GLenum = 0x8B81;
pub const GL_LINK_STATUS: GLenum = 0x8B82;
pub const GL_FRAMEBUFFER: GLenum = 0x8D40;
pub const GL_COLOR_ATTACHMENT0: GLenum = 0x8CE0;
pub const GL_FRAMEBUFFER_COMPLETE: GLenum = 0x8CD5;

#[link(name = "GLESv2")]
extern "C" {
    pub fn glCreateShader(kind: GLenum) -> GLuint;
    pub fn glShaderSource(s: GLuint, n: GLsizei, src: *const *const GLchar, len: *const GLint);
    pub fn glCompileShader(s: GLuint);
    pub fn glGetShaderiv(s: GLuint, pname: GLenum, out: *mut GLint);
    pub fn glGetShaderInfoLog(s: GLuint, max: GLsizei, len: *mut GLsizei, log: *mut GLchar);
    pub fn glCreateProgram() -> GLuint;
    pub fn glAttachShader(p: GLuint, s: GLuint);
    pub fn glBindAttribLocation(p: GLuint, index: GLuint, name: *const GLchar);
    pub fn glLinkProgram(p: GLuint);
    pub fn glGetProgramiv(p: GLuint, pname: GLenum, out: *mut GLint);
    pub fn glGetProgramInfoLog(p: GLuint, max: GLsizei, len: *mut GLsizei, log: *mut GLchar);
    pub fn glUseProgram(p: GLuint);
    pub fn glGetUniformLocation(p: GLuint, name: *const GLchar) -> GLint;
    pub fn glUniform1f(loc: GLint, x: GLfloat);
    pub fn glUniform3f(loc: GLint, x: GLfloat, y: GLfloat, z: GLfloat);
    pub fn glGenBuffers(n: GLsizei, out: *mut GLuint);
    pub fn glBindBuffer(target: GLenum, b: GLuint);
    pub fn glBufferData(target: GLenum, size: isize, data: *const c_void, usage: GLenum);
    pub fn glVertexAttribPointer(
        index: GLuint,
        size: GLint,
        kind: GLenum,
        normalized: GLboolean,
        stride: GLsizei,
        ptr: *const c_void,
    );
    pub fn glEnableVertexAttribArray(index: GLuint);
    pub fn glGenTextures(n: GLsizei, out: *mut GLuint);
    pub fn glBindTexture(target: GLenum, t: GLuint);
    pub fn glTexImage2D(
        target: GLenum,
        level: GLint,
        internal: GLint,
        w: GLsizei,
        h: GLsizei,
        border: GLint,
        format: GLenum,
        kind: GLenum,
        data: *const c_void,
    );
    pub fn glTexParameteri(target: GLenum, pname: GLenum, value: GLint);
    pub fn glGenFramebuffers(n: GLsizei, out: *mut GLuint);
    pub fn glBindFramebuffer(target: GLenum, f: GLuint);
    pub fn glFramebufferTexture2D(
        target: GLenum,
        attachment: GLenum,
        textarget: GLenum,
        texture: GLuint,
        level: GLint,
    );
    pub fn glCheckFramebufferStatus(target: GLenum) -> GLenum;
    pub fn glViewport(x: GLint, y: GLint, w: GLsizei, h: GLsizei);
    pub fn glScissor(x: GLint, y: GLint, w: GLsizei, h: GLsizei);
    pub fn glEnable(cap: GLenum);
    pub fn glDisable(cap: GLenum);
    pub fn glClearColor(r: GLfloat, g: GLfloat, b: GLfloat, a: GLfloat);
    pub fn glClear(mask: GLenum);
    pub fn glDrawArrays(mode: GLenum, first: GLint, count: GLsizei);
    pub fn glFinish();
}

pub type EGLDisplay = *mut c_void;
pub type EGLConfig = *mut c_void;
pub type EGLContext = *mut c_void;
pub type EGLSurface = *mut c_void;
pub type EGLint = i32;
pub type EGLBoolean = u32;

pub const EGL_NONE: EGLint = 0x3038;
pub const EGL_RED_SIZE: EGLint = 0x3024;
pub const EGL_GREEN_SIZE: EGLint = 0x3023;
pub const EGL_BLUE_SIZE: EGLint = 0x3022;
pub const EGL_SURFACE_TYPE: EGLint = 0x3033;
pub const EGL_WINDOW_BIT: EGLint = 0x0004;
pub const EGL_RENDERABLE_TYPE: EGLint = 0x3040;
pub const EGL_OPENGL_ES2_BIT: EGLint = 0x0004;
pub const EGL_CONTEXT_CLIENT_VERSION: EGLint = 0x3098;
pub const EGL_NATIVE_VISUAL_ID: EGLint = 0x302E;
pub const EGL_WIDTH: EGLint = 0x3057;
pub const EGL_HEIGHT: EGLint = 0x3056;

#[link(name = "EGL")]
extern "C" {
    pub fn eglGetDisplay(native: *mut c_void) -> EGLDisplay;
    pub fn eglInitialize(d: EGLDisplay, major: *mut EGLint, minor: *mut EGLint) -> EGLBoolean;
    pub fn eglChooseConfig(
        d: EGLDisplay,
        attrs: *const EGLint,
        configs: *mut EGLConfig,
        size: EGLint,
        num: *mut EGLint,
    ) -> EGLBoolean;
    pub fn eglGetConfigAttrib(d: EGLDisplay, c: EGLConfig, attr: EGLint, out: *mut EGLint)
        -> EGLBoolean;
    pub fn eglCreateContext(
        d: EGLDisplay,
        c: EGLConfig,
        share: EGLContext,
        attrs: *const EGLint,
    ) -> EGLContext;
    pub fn eglCreateWindowSurface(
        d: EGLDisplay,
        c: EGLConfig,
        window: *mut c_void,
        attrs: *const EGLint,
    ) -> EGLSurface;
    pub fn eglMakeCurrent(d: EGLDisplay, draw: EGLSurface, read: EGLSurface, ctx: EGLContext)
        -> EGLBoolean;
    pub fn eglSwapBuffers(d: EGLDisplay, s: EGLSurface) -> EGLBoolean;
    pub fn eglDestroySurface(d: EGLDisplay, s: EGLSurface) -> EGLBoolean;
    pub fn eglQuerySurface(d: EGLDisplay, s: EGLSurface, attr: EGLint, out: *mut EGLint)
        -> EGLBoolean;
}

#[link(name = "android")]
extern "C" {
    pub fn ANativeWindow_setBuffersGeometry(
        window: *mut c_void,
        width: i32,
        height: i32,
        format: i32,
    ) -> i32;
}

#[link(name = "log")]
extern "C" {
    pub fn __android_log_write(prio: i32, tag: *const c_char, text: *const c_char) -> i32;
}
