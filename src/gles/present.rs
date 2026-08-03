/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! Utilities for presenting frames to the window using an abstract OpenGL ES
//! implementation.

use super::gles11_raw as gles11; // constants and types only
use super::GLES;
use crate::matrix::Matrix;
use std::time::{Duration, Instant};

use std::sync::OnceLock;
use std::sync::Mutex;

pub struct FpsCounter {
    time: std::time::Instant,
    frames: u32,
}

// Global FPS text cache updated by FpsCounter so present_frame can draw it.
static LAST_FPS_TEXT: OnceLock<Mutex<String>> = OnceLock::new();
// Per-process cached GL glyph textures. Created lazily on first overlay draw.
static GLYPH_TEXTURES: OnceLock<Mutex<Option<Vec<u32>>>> = OnceLock::new();
// Runtime-controlled flag to enable the on-screen FPS overlay without requiring
// an environment variable. Use set_onscreen_fps_enabled(true/false) to control it
// from other parts of the runtime (e.g., the app picker or window input).
use std::sync::atomic::{AtomicBool, Ordering};
static ONSCREEN_FPS_ENABLED: OnceLock<AtomicBool> = OnceLock::new();

impl FpsCounter {
    pub fn start() -> Self {
        LAST_FPS_TEXT.get_or_init(|| Mutex::new(String::new()));
        GLYPH_TEXTURES.get_or_init(|| Mutex::new(None));
        FpsCounter {
            time: Instant::now(),
            frames: 0,
        }
    }

    pub fn count_frame(&mut self, label: std::fmt::Arguments<'_>) {
        self.frames += 1;
        let now = Instant::now();
        let duration = now - self.time;
        if duration >= Duration::from_secs(1) {
            self.time = now;
            let fps = std::mem::take(&mut self.frames) as f32 / duration.as_secs_f32();
            echo!(
                "touchHLE: {} FPS: {:.2}",
                label,
                fps
            );
            // Update global text cache for on-screen overlay if enabled via
            // environment variable or the runtime flag.
            let onscreen_env = std::env::var_os("TOUCHHLE_ONSCREEN_FPS").is_some();
            let onscreen_runtime = ONSCREEN_FPS_ENABLED
                .get()
                .map(|b| b.load(Ordering::SeqCst))
                .unwrap_or(false);
            if onscreen_env || onscreen_runtime {
                let text = format!("FPS: {:.1}", fps);
                if let Some(mutex) = LAST_FPS_TEXT.get() {
                    if let Ok(mut s) = mutex.lock() {
                        *s = text;
                    }
                }
            }
        }
    }
}

/// Runtime API: enable/disable the on-screen FPS overlay at runtime.
pub fn set_onscreen_fps_enabled(enabled: bool) {
    ONSCREEN_FPS_ENABLED.get_or_init(|| AtomicBool::new(false)).store(enabled, Ordering::SeqCst);
}

/// Present the the latest frame (e.g. the app's splash screen or rendering
/// output), provided as a texture bound to `GL_TEXTURE_2D`, by drawing it on
/// the window. It may be rotated, scaled and/or letterboxed as necessary. The
/// virtual cursor is also drawn if it should be currently visible.
///
/// The provided context must be current.
pub unsafe fn present_frame(
    gles: &mut dyn GLES,
    viewport: (u32, u32, u32, u32),
    rotation_matrix: Matrix<2>,
    virtual_cursor_visible_at: Option<(f32, f32, bool)>,
) {
    // While this is a generic utility, it is closely tied to
    // crate::frameworks::opengles::eagl::present_renderbuffer, which handles
    // backing up and restoring OpenGL ES state that this function might touch,
    // so these need to be updated in tandem.

    use gles11::types::*;

    // Draw the quad
    gles.Viewport(
        viewport.0 as _,
        viewport.1 as _,
        viewport.2 as _,
        viewport.3 as _,
    );
    gles.ClearColor(0.0, 0.0, 0.0, 1.0);
    gles.Clear(gles11::COLOR_BUFFER_BIT | gles11::DEPTH_BUFFER_BIT | gles11::STENCIL_BUFFER_BIT);
    gles.BindBuffer(gles11::ARRAY_BUFFER, 0);
    // Stretch the full rendered frame to fill the active host viewport.
    //
    // This does NOT crop or shift the texture. The whole renderbuffer is
    // sampled from normal 0..1 texture coordinates and mapped to a full-screen
    // quad. This is the correct "fill the current window" behavior for
    // PotatoGold-style landscape tests.
    if std::env::var_os("TOUCHHLE_PRESENT_STRETCH_TO_VIEWPORT").is_some() {
        log_once!(
            "TOUCHHLE_PRESENT_STRETCH_TO_VIEWPORT=1: stretching full rendered frame to the active viewport [this log will only be shown once]"
        );
    }

    let vertices: [f32; 12] = [
        -1.0, -1.0, -1.0, 1.0, 1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0,
    ];
    gles.EnableClientState(gles11::VERTEX_ARRAY);
    gles.VertexPointer(2, gles11::FLOAT, 0, vertices.as_ptr() as *const GLvoid);

    let tex_coords: [f32; 12] = [
        0.0, 0.0,
        0.0, 1.0,
        1.0, 0.0,
        1.0, 0.0,
        0.0, 1.0,
        1.0, 1.0,
    ];
    gles.EnableClientState(gles11::TEXTURE_COORD_ARRAY);
    gles.TexCoordPointer(2, gles11::FLOAT, 0, tex_coords.as_ptr() as *const GLvoid);
    // Apply the device-rotation matrix to the TEXTURE matrix, but rotate
    // around the centre of the tex coord square (0.5, 0.5) instead of the
    // origin. The naive `LoadMatrixf(rotation_matrix)` rotates around (0, 0),
    // which sends standard [0, 1]² UVs out of range — e.g. for a 90° rotation
    // (v, -u) reaches v' = -u ∈ [-1, 0]. On lenient drivers (Mesa, Apple
    // PowerVR) GL_REPEAT wrap quietly maps that back into [0, 1], but
    // strict drivers (Qualcomm Adreno's native ES 1.1 path) treat the
    // resulting sample of an NPOT texture (renderbuffer is typically
    // 320x480) as undefined and produce a mangled / black presented
    // frame. Pre- and post-translating by (0.5, 0.5) keeps tex coords in
    // [0, 1]² for any 90°/180°/270°/identity device rotation while
    // producing the same visual output as before on lenient drivers.
    let r = Matrix::<4>::from(&rotation_matrix);
    let to_center = Matrix::<4>::translate_3d(-0.5, -0.5, 0.0);
    let from_center = Matrix::<4>::translate_3d(0.5, 0.5, 0.0);
    // Note: Matrix::multiply(&other) computes `other · self` in
    // linear-algebra terms (other is applied AFTER self), so to get
    // `from_center · r · to_center` we chain in the order
    // to_center.multiply(&r).multiply(&from_center).
    let centered_rotation = to_center.multiply(&r).multiply(&from_center);
    gles.MatrixMode(gles11::TEXTURE);
    gles.LoadMatrixf(centered_rotation.columns().as_ptr() as *const _);
    gles.Enable(gles11::TEXTURE_2D);
    gles.DrawArrays(gles11::TRIANGLES, 0, 6);
    // clean this up so we don't need to worry about it in e.g. Core Animation
    gles.LoadIdentity();

    // Display virtual cursor
    if let Some((x, y, pressed)) = virtual_cursor_visible_at {
        let (vx, vy, vw, vh) = viewport;
        let x = x - vx as f32;
        let y = y - vy as f32;

        gles.DisableClientState(gles11::TEXTURE_COORD_ARRAY);
        gles.Disable(gles11::TEXTURE_2D);

        gles.Enable(gles11::BLEND);
        gles.BlendFunc(gles11::ONE, gles11::ONE_MINUS_SRC_ALPHA);
        gles.Color4f(0.0, 0.0, 0.0, if pressed { 2.0 / 3.0 } else { 1.0 / 3.0 });

        let radius = 10.0;

        let mut vertices = vertices;
        for i in (0..vertices.len()).step_by(2) {
            vertices[i] = (vertices[i] * radius + x) / (vw as f32 / 2.0) - 1.0;
            vertices[i + 1] = 1.0 - (vertices[i + 1] * radius + y) / (vh as f32 / 2.0);
        }
        gles.VertexPointer(2, gles11::FLOAT, 0, vertices.as_ptr() as *const GLvoid);
        gles.DrawArrays(gles11::TRIANGLES, 0, 6);
    }

    // On-screen FPS overlay (simple bitmap font). Enabled by env var
    // TOUCHHLE_ONSCREEN_FPS=1 or by the runtime flag set via
    // crate::gles::present::set_onscreen_fps_enabled(true).
    let onscreen_env = std::env::var_os("TOUCHHLE_ONSCREEN_FPS").is_some();
    let onscreen_runtime = ONSCREEN_FPS_ENABLED
        .get()
        .map(|b| b.load(Ordering::SeqCst))
        .unwrap_or(false);
    if onscreen_env || onscreen_runtime {
        if let Some(mutex) = LAST_FPS_TEXT.get() {
            if let Ok(s) = mutex.lock() {
                if !s.is_empty() {
                    draw_onscreen_text(gles, viewport, &s);
                }
            }
        }
    }
}

// --- Tiny bitmap font & overlay drawing implementation ---
const GLYPH_W: u32 = 8;
const GLYPH_H: u32 = 8;

// Glyphs available in this tiny font: "0123456789:.FPS"
const GLYPH_CHARS: &str = "0123456789:.FPS";
// Each glyph is 8 bytes, each bit is a pixel (MSB left).
const GLYPH_BITMAPS: &[[u8; 8]] = &[
    // 0
    [0x3C,0x66,0x6E,0x7E,0x76,0x66,0x3C,0x00],
    // 1
    [0x18,0x38,0x18,0x18,0x18,0x18,0x7E,0x00],
    // 2
    [0x3C,0x66,0x06,0x0C,0x18,0x30,0x7E,0x00],
    // 3
    [0x3C,0x66,0x06,0x1C,0x06,0x66,0x3C,0x00],
    // 4
    [0x0C,0x1C,0x3C,0x6C,0x7E,0x0C,0x1E,0x00],
    // 5
    [0x7E,0x60,0x7C,0x06,0x06,0x66,0x3C,0x00],
    // 6
    [0x3C,0x66,0x60,0x7C,0x66,0x66,0x3C,0x00],
    // 7
    [0x7E,0x66,0x0C,0x18,0x18,0x18,0x18,0x00],
    // 8
    [0x3C,0x66,0x66,0x3C,0x66,0x66,0x3C,0x00],
    // 9
    [0x3C,0x66,0x66,0x3E,0x06,0x66,0x3C,0x00],
    // : (colon)
    [0x00,0x18,0x18,0x00,0x00,0x18,0x18,0x00],
    // . (dot)
    [0x00,0x00,0x00,0x00,0x00,0x18,0x18,0x00],
    // F
    [0x7E,0x60,0x60,0x7C,0x60,0x60,0x60,0x00],
    // P
    [0x7C,0x66,0x66,0x7C,0x60,0x60,0x60,0x00],
    // S
    [0x3C,0x66,0x30,0x1C,0x06,0x66,0x3C,0x00],
];

fn glyph_index(ch: char) -> Option<usize> {
    GLYPH_CHARS.chars().position(|c| c == ch)
}

unsafe fn ensure_glyph_textures(gles: &mut dyn GLES) -> Option<Vec<u32>> {
    use gles11::types::*;
    let lock = GLYPH_TEXTURES.get().unwrap().lock().unwrap();
    if lock.is_some() {
        return lock.clone();
    }
    drop(lock);

    let mut guard = GLYPH_TEXTURES.get().unwrap().lock().unwrap();
    if guard.is_some() {
        return guard.clone();
    }

    let count = GLYPH_BITMAPS.len();
    let mut texs = Vec::with_capacity(count);
    for i in 0..count {
        let mut tex: GLuint = 0;
        gles.GenTextures(1, &mut tex);
        gles.BindTexture(gles11::TEXTURE_2D, tex);
        // Build RGBA data from bitmap
        let mut data = vec![0u8; (GLYPH_W * GLYPH_H * 4) as usize];
        let bmp = GLYPH_BITMAPS[i];
        for y in 0..GLYPH_H {
            let row = bmp[y as usize];
            for x in 0..GLYPH_W {
                let bit = (row >> (7 - x)) & 1;
                let idx = ((y * GLYPH_W + x) * 4) as usize;
                if bit != 0 {
                    data[idx] = 255; // R
                    data[idx + 1] = 255;
                    data[idx + 2] = 255;
                    data[idx + 3] = 255; // A
                } else {
                    data[idx] = 0;
                    data[idx + 1] = 0;
                    data[idx + 2] = 0;
                    data[idx + 3] = 0;
                }
            }
        }
        gles.TexImage2D(
            gles11::TEXTURE_2D,
            0,
            gles11::RGBA as _,
            GLYPH_W as _,
            GLYPH_H as _,
            0,
            gles11::RGBA,
            gles11::UNSIGNED_BYTE,
            data.as_ptr() as *const _,
        );
        gles.TexParameteri(gles11::TEXTURE_2D, gles11::TEXTURE_MIN_FILTER, gles11::NEAREST as _);
        gles.TexParameteri(gles11::TEXTURE_2D, gles11::TEXTURE_MAG_FILTER, gles11::NEAREST as _);
        gles.TexParameteri(gles11::TEXTURE_2D, gles11::TEXTURE_WRAP_S, gles11::CLAMP_TO_EDGE as _);
        gles.TexParameteri(gles11::TEXTURE_2D, gles11::TEXTURE_WRAP_T, gles11::CLAMP_TO_EDGE as _);
        texs.push(tex);
    }
    *guard = Some(texs.clone());
    Some(texs)
}

unsafe fn draw_onscreen_text(gles: &mut dyn GLES, viewport: (u32, u32, u32, u32), text: &str) {
    use gles11::types::*;
    let (vx, vy, vw, vh) = viewport;
    // Pixel size per glyph
    let scale = 2u32; // 8x8 * 2 = 16px high font
    let gw = (GLYPH_W * scale) as f32;
    let gh = (GLYPH_H * scale) as f32;

    // Ensure textures
    let texs_opt = ensure_glyph_textures(gles);
    if texs_opt.is_none() { return; }
    let texs = texs_opt.unwrap();

    // Save state
    let mut old_active_texture: GLint = 0;
    gles.GetIntegerv(gles11::ACTIVE_TEXTURE, &mut old_active_texture);
    let mut old_texture: GLint = 0;
    gles.GetIntegerv(gles11::TEXTURE_BINDING_2D, &mut old_texture);

    // Setup orthographic projection in pixels
    gles.MatrixMode(gles11::PROJECTION);
    gles.PushMatrix();
    gles.LoadIdentity();
    gles.Orthof(0.0, vw as _, vh as _, 0.0, -1.0, 1.0);
    gles.MatrixMode(gles11::MODELVIEW);
    gles.PushMatrix();
    gles.LoadIdentity();

    // Prepare arrays
    gles.EnableClientState(gles11::VERTEX_ARRAY);
    gles.EnableClientState(gles11::TEXTURE_COORD_ARRAY);
    gles.Enable(gles11::TEXTURE_2D);
    gles.Enable(gles11::BLEND);
    gles.BlendFunc(gles11::SRC_ALPHA, gles11::ONE_MINUS_SRC_ALPHA);

    // Draw text at top-left with small margin
    let mut x_px = vx as f32 + 8.0;
    let y_px = vy as f32 + 8.0;

    for ch in text.chars() {
        if let Some(idx) = glyph_index(ch) {
            let tex = texs[idx] as GLint;
            gles.BindTexture(gles11::TEXTURE_2D, tex as _);

            // Quad: two triangles
            let x0 = x_px;
            let y0 = y_px;
            let x1 = x_px + gw;
            let y1 = y_px + gh;
            let verts: [f32; 8] = [x0, y0, x0, y1, x1, y0, x1, y1];
            let texcoords: [f32; 8] = [0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0];
            gles.VertexPointer(2, gles11::FLOAT, 0, verts.as_ptr() as *const GLvoid);
            gles.TexCoordPointer(2, gles11::FLOAT, 0, texcoords.as_ptr() as *const GLvoid);
            gles.DrawArrays(gles11::TRIANGLE_STRIP, 0, 4);

            x_px += gw + 2.0;
        } else {
            // Unknown char -> space
            x_px += gw / 2.0;
        }
    }

    // Restore state
    gles.BindTexture(gles11::TEXTURE_2D, old_texture as _);
    gles.ActiveTexture(old_active_texture as _);
    gles.Disable(gles11::BLEND);
    gles.Disable(gles11::TEXTURE_2D);
    gles.DisableClientState(gles11::TEXTURE_COORD_ARRAY);
    gles.DisableClientState(gles11::VERTEX_ARRAY);

    gles.MatrixMode(gles11::MODELVIEW);
    gles.PopMatrix();
    gles.MatrixMode(gles11::PROJECTION);
    gles.PopMatrix();
    gles.MatrixMode(gles11::TEXTURE);
    gles.LoadIdentity();
}
