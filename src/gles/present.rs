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

pub struct FpsCounter {
    time: std::time::Instant,
    frames: u32,
}
impl FpsCounter {
    pub fn start() -> Self {
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
            echo!(
                "touchHLE: {} FPS: {:.2}",
                label,
                std::mem::take(&mut self.frames) as f32 / duration.as_secs_f32()
            );
        }
    }
}

/// Present the the latest frame (e.g. the app's splash screen or rendering
/// output), provided as a texture bound to `GL_TEXTURE_2D`, by drawing it on
/// the window. It may be rotated, scaled and/or letterboxed as necessary. The
/// virtual cursor is also drawn if it should be currently visible.
///
/// The provided context must be current.
// ContextSafePresentFix
pub unsafe fn present_frame(
    gles: &mut dyn GLES,
    viewport: (u32, u32, u32, u32),
    rotation_matrix: Matrix<2>,
    virtual_cursor_visible_at: Option<(f32, f32, bool)>,
) {
    use gles11::types::*;
    //DebugPresentArgs
    let is_gles2 = gles.is_gles2();
    log!(
        "DEBUG_PRESENT: present_frame called! viewport: {:?}, is_gles2: {}",
        viewport,
        is_gles2
    );
    let m = rotation_matrix.columns();
    log!("DEBUG_PRESENT: rotation_matrix: [{:?}, {:?}]", m[0], m[1]);

    let mut old_prog: GLint = 0;
    let mut old_array_buf: GLint = 0;
    let mut old_elem_buf: GLint = 0;
    let mut old_cull: GLboolean = 0;
    let mut old_depth: GLboolean = 0;
    let mut old_scissor: GLboolean = 0;
    let mut old_blend: GLboolean = 0;
    let mut old_stencil: GLboolean = 0;
    let mut old_dither: GLboolean = 0;
    let mut old_color_mask = [0u8; 4];
    let mut old_depth_mask: GLboolean = 0;
    let mut old_attribs = [0u8; 8];

    if is_gles2 {
        gles.GetIntegerv(0x8B8D, &mut old_prog);
        gles.GetIntegerv(0x8894, &mut old_array_buf);
        gles.GetIntegerv(0x8895, &mut old_elem_buf);
        gles.GetBooleanv(gles11::CULL_FACE, &mut old_cull);
        gles.GetBooleanv(gles11::DEPTH_TEST, &mut old_depth);
        gles.GetBooleanv(gles11::SCISSOR_TEST, &mut old_scissor);
        gles.GetBooleanv(gles11::BLEND, &mut old_blend);
        gles.GetBooleanv(gles11::STENCIL_TEST, &mut old_stencil);
        gles.GetBooleanv(gles11::DITHER, &mut old_dither);
        gles.GetBooleanv(
            gles11::COLOR_WRITEMASK,
            old_color_mask.as_mut_ptr() as *mut _,
        );
        gles.GetBooleanv(gles11::DEPTH_WRITEMASK, &mut old_depth_mask);

        for i in 0..8 {
            let mut status: GLint = 0;
            gles.GetVertexAttribiv(i, 0x8622, &mut status);
            old_attribs[i as usize] = status as u8;
            gles.DisableVertexAttribArray(i);
        }

        gles.ColorMask(1, 1, 1, 1);
        gles.DepthMask(1);
        gles.Disable(gles11::CULL_FACE);
        gles.Disable(gles11::DEPTH_TEST);
        gles.Disable(gles11::SCISSOR_TEST);
        gles.Disable(gles11::BLEND);
        gles.Disable(gles11::STENCIL_TEST);
        gles.Disable(gles11::DITHER);
        gles.BindBuffer(gles11::ELEMENT_ARRAY_BUFFER, 0);
    }

    gles.Viewport(
        viewport.0 as _,
        viewport.1 as _,
        viewport.2 as _,
        viewport.3 as _,
    );

    if is_gles2 {
        gles.ClearColor(0.2, 0.0, 0.0, 1.0);
    } else {
        gles.ClearColor(0.0, 0.0, 0.0, 1.0);
    }
    gles.Clear(gles11::COLOR_BUFFER_BIT | gles11::DEPTH_BUFFER_BIT | gles11::STENCIL_BUFFER_BIT);

    let vertices: [f32; 12] = [
        -1.0, -1.0, -1.0, 1.0, 1.0, -1.0, 1.0, -1.0, -1.0, 1.0, 1.0, 1.0,
    ];
    let tex_coords: [f32; 12] = [0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0];
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

    // DebugPresentGeom
    log!("DEBUG_PRESENT: Quad Vertices: {:?}", vertices);
    log!("DEBUG_PRESENT: Quad TexCoords: {:?}", tex_coords);
    log!("DEBUG_PRESENT: Applied TexMatrix: {:?}", matrix.columns());

    if is_gles2 {
        let vs_src = "attribute vec4 position;\nattribute vec2 texCoord;\nuniform mat4 texMatrix;\nvarying vec2 v_texCoord;\nvoid main() {\n    gl_Position = position;\n    v_texCoord = (texMatrix * vec4(texCoord, 0.0, 1.0)).xy;\n}\0";
        // RemoveDebugTint
        let fs_src = "precision mediump float;\nvarying vec2 v_texCoord;\nuniform sampler2D tex;\nuniform vec4 color;\nvoid main() {\n    vec4 texColor = texture2D(tex, v_texCoord);\n    gl_FragColor = vec4(mix(texColor.rgb, color.rgb, color.a), 1.0);\n}\0";

        let vs = gles.CreateShader(0x8B31);
        let vs_ptr = [vs_src.as_ptr() as *const std::ffi::c_char];
        let vs_len = [vs_src.len() as GLint - 1];
        gles.ShaderSource(vs, 1, vs_ptr.as_ptr(), vs_len.as_ptr());
        gles.CompileShader(vs);

        let fs = gles.CreateShader(0x8B30);
        let fs_ptr = [fs_src.as_ptr() as *const std::ffi::c_char];
        let fs_len = [fs_src.len() as GLint - 1];
        gles.ShaderSource(fs, 1, fs_ptr.as_ptr(), fs_len.as_ptr());
        gles.CompileShader(fs);

        let prog = gles.CreateProgram();
        gles.AttachShader(prog, vs);
        gles.AttachShader(prog, fs);
        // SafeAttribLocation
        gles.BindAttribLocation(prog, 6, c"position".as_ptr() as *const _);
        gles.BindAttribLocation(prog, 7, c"texCoord".as_ptr() as *const _);
        gles.LinkProgram(prog);

        let pos = gles.GetAttribLocation(prog, c"position".as_ptr() as *const _);
        let tex = gles.GetAttribLocation(prog, c"texCoord".as_ptr() as *const _);
        let mat = gles.GetUniformLocation(prog, c"texMatrix".as_ptr() as *const _);
        let col = gles.GetUniformLocation(prog, c"color".as_ptr() as *const _);
        let sampler = gles.GetUniformLocation(prog, c"tex".as_ptr() as *const _);

        gles.UseProgram(prog);
        gles.Uniform4f(col, 0.0, 0.0, 0.0, 0.0);
        gles.UniformMatrix4fv(mat, 1, 0, matrix.columns().as_ptr() as *const _);

        let mut active_tex: GLint = 0;
        gles.GetIntegerv(gles11::ACTIVE_TEXTURE, &mut active_tex);
        let tex_unit = active_tex - gles11::TEXTURE0 as GLint;
        if sampler >= 0 {
            gles.Uniform1i(sampler, tex_unit);
        }

        gles.BindBuffer(gles11::ARRAY_BUFFER, 0);
        if pos >= 0 {
            gles.EnableVertexAttribArray(pos as GLuint);
            gles.VertexAttribPointer(
                pos as GLuint,
                2,
                gles11::FLOAT,
                0,
                0,
                vertices.as_ptr() as *const _,
            );
        }
        if tex >= 0 {
            gles.EnableVertexAttribArray(tex as GLuint);
            gles.VertexAttribPointer(
                tex as GLuint,
                2,
                gles11::FLOAT,
                0,
                0,
                tex_coords.as_ptr() as *const _,
            );
        }

        //DebugDrawArrays
        while gles.GetError() != 0 {}
        gles.DrawArrays(gles11::TRIANGLES, 0, 6);
        let draw_err = gles.GetError();
        if draw_err != 0 {
            log!("DEBUG_PRESENT: ERROR after DrawArrays: {:#x}", draw_err);
        }

        if let Some((x, y, pressed)) = virtual_cursor_visible_at {
            let (vx, vy, vw, vh) = viewport;
            let x = x - vx as f32;
            let y = y - vy as f32;
            gles.Enable(gles11::BLEND);
            gles.BlendFunc(gles11::ONE, gles11::ONE_MINUS_SRC_ALPHA);
            let radius = 10.0;
            let mut cursor_vertices = vertices;
            for i in (0..cursor_vertices.len()).step_by(2) {
                cursor_vertices[i] = (cursor_vertices[i] * radius + x) / (vw as f32 / 2.0) - 1.0;
                cursor_vertices[i + 1] =
                    1.0 - (cursor_vertices[i + 1] * radius + y) / (vh as f32 / 2.0);
            }
            gles.Uniform4f(
                col,
                0.0,
                0.0,
                0.0,
                if pressed { 2.0 / 3.0 } else { 1.0 / 3.0 },
            );
            if tex >= 0 {
                gles.DisableVertexAttribArray(tex as GLuint);
            }
            if pos >= 0 {
                gles.VertexAttribPointer(
                    pos as GLuint,
                    2,
                    gles11::FLOAT,
                    0,
                    0,
                    cursor_vertices.as_ptr() as *const _,
                );
            }
            gles.DrawArrays(gles11::TRIANGLES, 0, 6);
        }

        if pos >= 0 {
            gles.DisableVertexAttribArray(pos as GLuint);
        }
        if tex >= 0 {
            gles.DisableVertexAttribArray(tex as GLuint);
        }

        gles.UseProgram(old_prog as GLuint);
        gles.DeleteProgram(prog);
        gles.DeleteShader(vs);
        gles.DeleteShader(fs);

        gles.BindBuffer(gles11::ARRAY_BUFFER, old_array_buf as GLuint);
        gles.BindBuffer(gles11::ELEMENT_ARRAY_BUFFER, old_elem_buf as GLuint);
        // StateRestoreFix
        if old_cull != 0 {
            gles.Enable(gles11::CULL_FACE);
        } else {
            gles.Disable(gles11::CULL_FACE);
        }
        if old_depth != 0 {
            gles.Enable(gles11::DEPTH_TEST);
        } else {
            gles.Disable(gles11::DEPTH_TEST);
        }
        if old_scissor != 0 {
            gles.Enable(gles11::SCISSOR_TEST);
        } else {
            gles.Disable(gles11::SCISSOR_TEST);
        }
        if old_blend != 0 {
            gles.Enable(gles11::BLEND);
        } else {
            gles.Disable(gles11::BLEND);
        }
        if old_stencil != 0 {
            gles.Enable(gles11::STENCIL_TEST);
        } else {
            gles.Disable(gles11::STENCIL_TEST);
        }
        if old_dither != 0 {
            gles.Enable(gles11::DITHER);
        } else {
            gles.Disable(gles11::DITHER);
        }
        gles.ColorMask(
            old_color_mask[0],
            old_color_mask[1],
            old_color_mask[2],
            old_color_mask[3],
        );
        gles.DepthMask(old_depth_mask);
        for i in 0..8 {
            if old_attribs[i as usize] != 0 {
                gles.EnableVertexAttribArray(i);
            }
        }
    } else {
        gles.BindBuffer(gles11::ARRAY_BUFFER, 0);
        gles.EnableClientState(gles11::VERTEX_ARRAY);
        gles.VertexPointer(2, gles11::FLOAT, 0, vertices.as_ptr() as *const GLvoid);
        gles.EnableClientState(gles11::TEXTURE_COORD_ARRAY);
        gles.TexCoordPointer(2, gles11::FLOAT, 0, tex_coords.as_ptr() as *const GLvoid);
        gles.MatrixMode(gles11::TEXTURE);
        gles.LoadMatrixf(matrix.columns().as_ptr() as *const _);
        gles.Enable(gles11::TEXTURE_2D);
        gles.DrawArrays(gles11::TRIANGLES, 0, 6);
        gles.LoadIdentity();

        if let Some((x, y, pressed)) = virtual_cursor_visible_at {
            let (vx, vy, vw, vh) = viewport;
            let x = x - vx as f32;
            let y = y - vy as f32;
            gles.Enable(gles11::BLEND);
            gles.BlendFunc(gles11::ONE, gles11::ONE_MINUS_SRC_ALPHA);
            let radius = 10.0;
            let mut cursor_vertices = vertices;
            for i in (0..cursor_vertices.len()).step_by(2) {
                cursor_vertices[i] = (cursor_vertices[i] * radius + x) / (vw as f32 / 2.0) - 1.0;
                cursor_vertices[i + 1] =
                    1.0 - (cursor_vertices[i + 1] * radius + y) / (vh as f32 / 2.0);
            }
            gles.DisableClientState(gles11::TEXTURE_COORD_ARRAY);
            gles.Disable(gles11::TEXTURE_2D);
            gles.Color4f(0.0, 0.0, 0.0, if pressed { 2.0 / 3.0 } else { 1.0 / 3.0 });
            gles.VertexPointer(
                2,
                gles11::FLOAT,
                0,
                cursor_vertices.as_ptr() as *const GLvoid,
            );
            gles.DrawArrays(gles11::TRIANGLES, 0, 6);
        }
    }
}
