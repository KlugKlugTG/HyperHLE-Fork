/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! The "Device frame" feature: draws the physical front of the emulated
//! iPhone around the app's screen, so the game appears to run inside a
//! real device (see the `--device-frame` option and the "Device frame"
//! switch in the Settings screen's DEVICE section).
//!
//! Each supported iPhone model has a portrait-orientation RGBA PNG in
//! `res/bezels/` whose screen area is a fully transparent rectangle (the
//! "hole"). At present time the image is letterboxed into the window and
//! the app's viewport is shrunk to the hole, so guest content, touch
//! input and host overlays all line up with the picture.
//!
//! The layout tables below must stay in sync with the generator script
//! `dev-scripts/gen_bezels.py`.

use crate::gles::gles11_raw as gles11; // constants and types only
use crate::gles::GLES;
use crate::image::Image;
use crate::window::{DeviceFamily, DeviceOrientation};
use std::sync::Mutex;

/// One device front image and the screen hole inside it, in image pixels.
struct BezelSpec {
    bytes: &'static [u8],
    width: u32,
    height: u32,
    /// (x, y, width, height) of the transparent screen area.
    hole: (u32, u32, u32, u32),
}

fn bezel_spec(family: DeviceFamily) -> Option<BezelSpec> {
    let spec = match family {
        DeviceFamily::iPhone => BezelSpec {
            bytes: include_bytes!("../res/bezels/iphone.png"),
            width: 1084,
            height: 920,
            hole: (62, 104, 960, 640),
        },
        DeviceFamily::iPhone3G => BezelSpec {
            bytes: include_bytes!("../res/bezels/iphone_3g.png"),
            width: 1084,
            height: 920,
            hole: (62, 104, 960, 640),
        },
        DeviceFamily::iPhone3GS => BezelSpec {
            bytes: include_bytes!("../res/bezels/iphone_3gs.png"),
            width: 1084,
            height: 920,
            hole: (62, 104, 960, 640),
        },
        DeviceFamily::iPhone4 => BezelSpec {
            bytes: include_bytes!("../res/bezels/iphone_4.png"),
            width: 1068,
            height: 884,
            hole: (54, 92, 960, 640),
        },
        DeviceFamily::iPhone4s => BezelSpec {
            bytes: include_bytes!("../res/bezels/iphone_4s.png"),
            width: 1068,
            height: 884,
            hole: (54, 92, 960, 640),
        },
        DeviceFamily::iPhone5 => BezelSpec {
            bytes: include_bytes!("../res/bezels/iphone_5.png"),
            width: 1236,
            height: 864,
            hole: (50, 82, 1136, 640),
        },
        DeviceFamily::iPhone5c => BezelSpec {
            bytes: include_bytes!("../res/bezels/iphone_5c.png"),
            width: 1244,
            height: 870,
            hole: (54, 84, 1136, 640),
        },
        // iPads and iPods intentionally have no frame: the feature is
        // iPhone-only.
        _ => return None,
    };
    Some(spec)
}

/// Whether a device front image exists for `family`.
pub fn supported(family: DeviceFamily) -> bool {
    bezel_spec(family).is_some()
}

/// How the portrait image is turned to match the device orientation:
/// 0 = upright, 1 = 90° clockwise, 2 = 180°, 3 = 90° counter-clockwise.
fn rotation_for(orientation: DeviceOrientation) -> u8 {
    match orientation {
        DeviceOrientation::Portrait => 0,
        DeviceOrientation::PortraitUpsideDown => 2,
        // Home button on the right, like a phone turned counter-clockwise.
        DeviceOrientation::LandscapeLeft => 3,
        // Home button on the left.
        DeviceOrientation::LandscapeRight => 1,
    }
}

/// Everything the present pass needs to draw the frame on top of the
/// window: the quad in drawable pixels and its texture coordinates.
#[derive(Copy, Clone)]
pub struct DrawInfo {
    family: DeviceFamily,
    /// Bezel quad in window drawable pixels: (x, y, width, height).
    bezel: (f32, f32, f32, f32),
    /// Size of the window drawable in pixels.
    drawable: (u32, u32),
    /// Texture coordinates (portrait image space) of the quad corners in
    /// the order top-left, top-right, bottom-right, bottom-left.
    uvs: [[f32; 2]; 4],
}

/// Computes the bezel quad for a drawable of `drawable` pixels and the
/// screen hole inside it (both in drawable pixels), or `None` when the
/// device family has no frame image.
pub fn layout(
    family: DeviceFamily,
    orientation: DeviceOrientation,
    drawable: (u32, u32),
) -> Option<(DrawInfo, (u32, u32, u32, u32))> {
    let spec = bezel_spec(family)?;
    let (dw, dh) = drawable;
    if dw == 0 || dh == 0 {
        return None;
    }
    let rot = rotation_for(orientation);
    let (iw, ih) = (spec.width as f32, spec.height as f32);
    let (hx, hy, hw, hh) = (
        spec.hole.0 as f32,
        spec.hole.1 as f32,
        spec.hole.2 as f32,
        spec.hole.3 as f32,
    );
    // Displayed (rotated) image size and hole position within it.
    let (disp_w, disp_h, hole_x, hole_y, hole_w, hole_h) = match rot {
        1 => (ih, iw, ih - hy - hh, hx, hh, hw),
        2 => (iw, ih, iw - hx - hw, ih - hy - hh, hw, hh),
        3 => (ih, iw, hy, iw - hx - hw, hh, hw),
        _ => (iw, ih, hx, hy, hw, hh),
    };
    let scale = (dw as f32 / disp_w).min(dh as f32 / disp_h);
    let ox = (dw as f32 - disp_w * scale) / 2.0;
    let oy = (dh as f32 - disp_h * scale) / 2.0;
    let hole = (
        (ox + hole_x * scale).round() as u32,
        (oy + hole_y * scale).round() as u32,
        (hole_w * scale).round() as u32,
        (hole_h * scale).round() as u32,
    );
    // Texture corners for the quad, per rotation (see rotation_for).
    let uvs = match rot {
        1 => [[0.0, 1.0], [0.0, 0.0], [1.0, 0.0], [1.0, 1.0]],
        2 => [[1.0, 1.0], [0.0, 1.0], [0.0, 0.0], [1.0, 0.0]],
        3 => [[1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.0, 0.0]],
        _ => [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
    };
    let info = DrawInfo {
        family,
        bezel: (ox, oy, disp_w * scale, disp_h * scale),
        drawable,
        uvs,
    };
    Some((info, hole))
}

/// Cached bezel texture, shared by all GL contexts of the process (they
/// are created in one share group). Recreated only when the model changes.
static BEZEL_TEXTURE: Mutex<Option<(DeviceFamily, u32)>> = Mutex::new(None);

unsafe fn ensure_texture(gles: &mut dyn GLES, family: DeviceFamily) -> Option<u32> {
    use gles11::types::*;
    {
        let cached = BEZEL_TEXTURE.lock().unwrap();
        if let Some((cached_family, tex)) = *cached {
            if cached_family == family {
                return Some(tex);
            }
        }
    }
    let spec = bezel_spec(family)?;
    let image = Image::from_bytes(spec.bytes).ok()?;
    let (width, height) = image.dimensions();
    let mut tex: GLuint = 0;
    gles.GenTextures(1, &mut tex);
    if tex == 0 {
        return None;
    }
    gles.BindTexture(gles11::TEXTURE_2D, tex);
    gles.TexImage2D(
        gles11::TEXTURE_2D,
        0,
        gles11::RGBA as _,
        width as _,
        height as _,
        0,
        gles11::RGBA,
        gles11::UNSIGNED_BYTE,
        image.pixels().as_ptr() as *const _,
    );
    gles.TexParameteri(
        gles11::TEXTURE_2D,
        gles11::TEXTURE_MIN_FILTER,
        gles11::LINEAR as _,
    );
    gles.TexParameteri(
        gles11::TEXTURE_2D,
        gles11::TEXTURE_MAG_FILTER,
        gles11::LINEAR as _,
    );
    gles.TexParameteri(
        gles11::TEXTURE_2D,
        gles11::TEXTURE_WRAP_S,
        gles11::CLAMP_TO_EDGE as _,
    );
    gles.TexParameteri(
        gles11::TEXTURE_2D,
        gles11::TEXTURE_WRAP_T,
        gles11::CLAMP_TO_EDGE as _,
    );
    *BEZEL_TEXTURE.lock().unwrap() = Some((family, tex));
    Some(tex)
}

unsafe fn get_int(gles: &mut dyn GLES, pname: u32) -> i32 {
    let mut value: i32 = 0;
    gles.GetIntegerv(pname, &mut value);
    value
}

unsafe fn get_ptr(gles: &mut dyn GLES, pname: u32) -> *const std::ffi::c_void {
    let mut value: *const std::ffi::c_void = std::ptr::null();
    gles.GetPointerv(pname, &mut value);
    value
}

/// The client-side array state clobbered by a client-array quad draw.
/// Guests such as cocos2d apps set their pointers once at init and never
/// again, so every field has to be put back exactly as it was.
struct SavedArrays {
    array_buffer: i32,
    vertex: (i32, i32, i32, i32, *const std::ffi::c_void),
    texcoord: (i32, i32, i32, i32, *const std::ffi::c_void),
    vertex_enabled: bool,
    texcoord_enabled: bool,
}

unsafe fn save_arrays(gles: &mut dyn GLES) -> SavedArrays {
    SavedArrays {
        array_buffer: get_int(gles, gles11::ARRAY_BUFFER_BINDING),
        vertex: (
            get_int(gles, gles11::VERTEX_ARRAY_BUFFER_BINDING),
            get_int(gles, gles11::VERTEX_ARRAY_SIZE),
            get_int(gles, gles11::VERTEX_ARRAY_TYPE),
            get_int(gles, gles11::VERTEX_ARRAY_STRIDE),
            get_ptr(gles, gles11::VERTEX_ARRAY_POINTER),
        ),
        texcoord: (
            get_int(gles, gles11::TEXTURE_COORD_ARRAY_BUFFER_BINDING),
            get_int(gles, gles11::TEXTURE_COORD_ARRAY_SIZE),
            get_int(gles, gles11::TEXTURE_COORD_ARRAY_TYPE),
            get_int(gles, gles11::TEXTURE_COORD_ARRAY_STRIDE),
            get_ptr(gles, gles11::TEXTURE_COORD_ARRAY_POINTER),
        ),
        vertex_enabled: gles.IsEnabled(gles11::VERTEX_ARRAY) != 0,
        texcoord_enabled: gles.IsEnabled(gles11::TEXTURE_COORD_ARRAY) != 0,
    }
}

unsafe fn restore_arrays(gles: &mut dyn GLES, saved: SavedArrays) {
    use gles11::types::*;
    let (buf, size, ty, stride, ptr) = saved.vertex;
    gles.BindBuffer(gles11::ARRAY_BUFFER, buf as GLuint);
    gles.VertexPointer(size, ty as GLenum, stride, ptr);
    let (buf, size, ty, stride, ptr) = saved.texcoord;
    gles.BindBuffer(gles11::ARRAY_BUFFER, buf as GLuint);
    gles.TexCoordPointer(size, ty as GLenum, stride, ptr);
    gles.BindBuffer(gles11::ARRAY_BUFFER, saved.array_buffer as GLuint);
    if saved.vertex_enabled {
        gles.EnableClientState(gles11::VERTEX_ARRAY);
    } else {
        gles.DisableClientState(gles11::VERTEX_ARRAY);
    }
    if saved.texcoord_enabled {
        gles.EnableClientState(gles11::TEXTURE_COORD_ARRAY);
    } else {
        gles.DisableClientState(gles11::TEXTURE_COORD_ARRAY);
    }
}

/// Builds the quad corners (top-left, top-right, bottom-right,
/// bottom-left) in drawable pixels.
fn quad_vertices(info: &DrawInfo) -> [f32; 8] {
    let (x, y, w, h) = info.bezel;
    [x, y, x + w, y, x + w, y + h, x, y + h]
}

/// Draws the device front over the window with the fixed-function
/// (OpenGL ES 1.x) API. The provided context must be current.
pub unsafe fn draw_es1(gles: &mut dyn GLES, info: &DrawInfo) {
    use gles11::types::*;
    let Some(tex) = ensure_texture(gles, info.family) else {
        return;
    };

    let mut old_viewport = [0i32; 4];
    gles.GetIntegerv(gles11::VIEWPORT, old_viewport.as_mut_ptr());
    let mut old_color = [0.0f32; 4];
    gles.GetFloatv(gles11::CURRENT_COLOR, old_color.as_mut_ptr());
    let blend_was_on = gles.IsEnabled(gles11::BLEND) != 0;
    let old_blend_src = get_int(gles, gles11::BLEND_SRC);
    let old_blend_dst = get_int(gles, gles11::BLEND_DST);
    let texture_was_on = gles.IsEnabled(gles11::TEXTURE_2D) != 0;
    let old_texture = get_int(gles, gles11::TEXTURE_BINDING_2D);
    let mut old_tex_env_mode: GLint = 0;
    gles.GetTexEnviv(
        gles11::TEXTURE_ENV,
        gles11::TEXTURE_ENV_MODE,
        &mut old_tex_env_mode,
    );
    let arrays = save_arrays(gles);

    gles.MatrixMode(gles11::PROJECTION);
    gles.PushMatrix();
    gles.LoadIdentity();
    gles.Orthof(
        0.0,
        info.drawable.0 as _,
        info.drawable.1 as _,
        0.0,
        -1.0,
        1.0,
    );
    gles.MatrixMode(gles11::MODELVIEW);
    gles.PushMatrix();
    gles.LoadIdentity();
    gles.MatrixMode(gles11::TEXTURE);
    gles.PushMatrix();
    gles.LoadIdentity();

    gles.Viewport(0, 0, info.drawable.0 as _, info.drawable.1 as _);
    gles.Color4f(1.0, 1.0, 1.0, 1.0);
    // The bezel PNG is decoded to premultiplied alpha, like all iPhone OS
    // image loading, so the source colour is added as-is.
    gles.Enable(gles11::BLEND);
    gles.BlendFunc(gles11::ONE, gles11::ONE_MINUS_SRC_ALPHA);
    gles.Enable(gles11::TEXTURE_2D);
    gles.BindTexture(gles11::TEXTURE_2D, tex);
    gles.TexEnvi(
        gles11::TEXTURE_ENV,
        gles11::TEXTURE_ENV_MODE,
        gles11::REPLACE as _,
    );

    let verts = quad_vertices(info);
    let uvs: [f32; 8] = [
        info.uvs[0][0],
        info.uvs[0][1],
        info.uvs[1][0],
        info.uvs[1][1],
        info.uvs[2][0],
        info.uvs[2][1],
        info.uvs[3][0],
        info.uvs[3][1],
    ];
    gles.EnableClientState(gles11::VERTEX_ARRAY);
    gles.EnableClientState(gles11::TEXTURE_COORD_ARRAY);
    gles.BindBuffer(gles11::ARRAY_BUFFER, 0);
    gles.VertexPointer(2, gles11::FLOAT, 0, verts.as_ptr() as *const GLvoid);
    gles.TexCoordPointer(2, gles11::FLOAT, 0, uvs.as_ptr() as *const GLvoid);
    gles.DrawArrays(gles11::TRIANGLE_STRIP, 0, 4);

    gles.MatrixMode(gles11::TEXTURE);
    gles.PopMatrix();
    gles.MatrixMode(gles11::MODELVIEW);
    gles.PopMatrix();
    gles.MatrixMode(gles11::PROJECTION);
    gles.PopMatrix();

    gles.Viewport(
        old_viewport[0],
        old_viewport[1],
        old_viewport[2] as _,
        old_viewport[3] as _,
    );
    gles.Color4f(old_color[0], old_color[1], old_color[2], old_color[3]);
    if blend_was_on {
        gles.Enable(gles11::BLEND);
    } else {
        gles.Disable(gles11::BLEND);
    }
    gles.BlendFunc(old_blend_src as GLenum, old_blend_dst as GLenum);
    if texture_was_on {
        gles.Enable(gles11::TEXTURE_2D);
    } else {
        gles.Disable(gles11::TEXTURE_2D);
    }
    gles.BindTexture(gles11::TEXTURE_2D, old_texture as GLuint);
    gles.TexEnvi(
        gles11::TEXTURE_ENV,
        gles11::TEXTURE_ENV_MODE,
        old_tex_env_mode,
    );
    restore_arrays(gles, arrays);
    let _ = gles.GetError();
}

#[derive(Copy, Clone)]
struct FrameProgram {
    program: u32,
    a_pos: i32,
    a_uv: i32,
    u_tex: i32,
}

#[derive(Copy, Clone)]
struct FrameObjects {
    vbo: u32,
}

static FRAME_PROGRAM: Mutex<Option<FrameProgram>> = Mutex::new(None);
static FRAME_OBJECTS: Mutex<Option<FrameObjects>> = Mutex::new(None);

unsafe fn ensure_program(gles: &mut dyn GLES) -> Option<FrameProgram> {
    use crate::gles::gles2_raw as gles2;
    use gles11::types::*;
    if let Some(program) = *FRAME_PROGRAM.lock().unwrap() {
        return Some(program);
    }
    let vs_src = b"attribute vec2 aPos;\nattribute vec2 aUV;\nvarying vec2 vUV;\nvoid main() {\n    gl_Position = vec4(aPos, 0.0, 1.0);\n    vUV = aUV;\n}\0";
    let fs_src = b"precision mediump float;\nvarying vec2 vUV;\nuniform sampler2D uTex;\nvoid main() {\n    gl_FragColor = texture2D(uTex, vUV);\n}\0";
    unsafe fn compile(
        gles: &mut dyn GLES,
        src: &[u8],
        kind: u32,
    ) -> Option<u32> {
        use crate::gles::gles2_raw as gles2;
        use gles11::types::*;
        let shader = gles.CreateShader(kind);
        let ptr = src.as_ptr() as *const _;
        let len = (src.len() - 1) as GLint;
        gles.ShaderSource(shader, 1, &ptr, &len);
        gles.CompileShader(shader);
        let mut ok: GLint = 0;
        gles.GetShaderiv(shader, gles2::COMPILE_STATUS, &mut ok);
        if ok == 0 {
            log!("Warning: device frame shader failed to compile");
            gles.DeleteShader(shader);
            return None;
        }
        Some(shader)
    }
    let vs = compile(gles, vs_src, gles2::VERTEX_SHADER)?;
    let fs = compile(gles, fs_src, gles2::FRAGMENT_SHADER)?;
    let program = gles.CreateProgram();
    gles.AttachShader(program, vs);
    gles.AttachShader(program, fs);
    gles.BindAttribLocation(program, 6, b"aPos\0".as_ptr() as *const _);
    gles.BindAttribLocation(program, 7, b"aUV\0".as_ptr() as *const _);
    gles.LinkProgram(program);
    let mut ok: GLint = 0;
    gles.GetProgramiv(program, gles2::LINK_STATUS, &mut ok);
    gles.DeleteShader(vs);
    gles.DeleteShader(fs);
    if ok == 0 {
        log!("Warning: device frame program failed to link");
        gles.DeleteProgram(program);
        return None;
    }
    let result = FrameProgram {
        program,
        a_pos: gles.GetAttribLocation(program, b"aPos\0".as_ptr() as *const _),
        a_uv: gles.GetAttribLocation(program, b"aUV\0".as_ptr() as *const _),
        u_tex: gles.GetUniformLocation(program, b"uTex\0".as_ptr() as *const _),
    };
    *FRAME_PROGRAM.lock().unwrap() = Some(result);
    Some(result)
}

unsafe fn ensure_objects(gles: &mut dyn GLES) -> Option<FrameObjects> {
    if let Some(objects) = *FRAME_OBJECTS.lock().unwrap() {
        return Some(objects);
    }
    let mut vbo: u32 = 0;
    gles.GenBuffers(1, &mut vbo);
    if vbo == 0 {
        return None;
    }
    let objects = FrameObjects { vbo };
    *FRAME_OBJECTS.lock().unwrap() = Some(objects);
    Some(objects)
}

/// Draws the device front over the window with a small OpenGL ES 2.0
/// shader, for native ES 2.0 drivers without a fixed-function pipeline.
/// The provided context must be current.
pub unsafe fn draw_es2(gles: &mut dyn GLES, info: &DrawInfo) {
    use crate::gles::gles2_raw as gles2;
    use gles11::types::*;

    let Some(tex) = ensure_texture(gles, info.family) else {
        return;
    };
    let Some(program) = ensure_program(gles) else {
        return;
    };
    let Some(objects) = ensure_objects(gles) else {
        return;
    };

    let old_program = get_int(gles, gles2::CURRENT_PROGRAM);
    let old_array_buffer = get_int(gles, gles2::ARRAY_BUFFER_BINDING);
    let old_active_texture = get_int(gles, gles2::ACTIVE_TEXTURE);
    gles.ActiveTexture(gles2::TEXTURE0);
    let old_texture0 = get_int(gles, gles2::TEXTURE_BINDING_2D);
    gles.ActiveTexture(old_active_texture as u32);
    let old_texture = get_int(gles, gles2::TEXTURE_BINDING_2D);
    let mut old_viewport = [0i32; 4];
    gles.GetIntegerv(gles2::VIEWPORT, old_viewport.as_mut_ptr());
    let blend_was_on = gles.IsEnabled(gles2::BLEND) != 0;
    let old_blend_src = get_int(gles, gles11::BLEND_SRC);
    let old_blend_dst = get_int(gles, gles11::BLEND_DST);
    let slots = [program.a_pos as u32, program.a_uv as u32];
    let mut slot_was_enabled = [0i32; 2];
    for (&slot, was) in slots.iter().zip(slot_was_enabled.iter_mut()) {
        gles.GetVertexAttribiv(slot, gles2::VERTEX_ATTRIB_ARRAY_ENABLED, was);
    }

    // Interleaved position (NDC) + texture coordinate per corner.
    let verts = quad_vertices(info);
    let (dw, dh) = (info.drawable.0 as f32, info.drawable.1 as f32);
    let mut data = [0.0f32; 16];
    for (i, corner) in verts.chunks_exact(2).enumerate() {
        data[i * 4] = corner[0] / dw * 2.0 - 1.0;
        data[i * 4 + 1] = 1.0 - corner[1] / dh * 2.0;
        data[i * 4 + 2] = info.uvs[i][0];
        data[i * 4 + 3] = info.uvs[i][1];
    }
    gles.BindBuffer(gles2::ARRAY_BUFFER, objects.vbo);
    gles.BufferData(
        gles2::ARRAY_BUFFER,
        (data.len() * 4) as GLsizeiptr,
        data.as_ptr() as *const GLvoid,
        gles2::DYNAMIC_DRAW,
    );
    gles.UseProgram(program.program);
    gles.Uniform1i(program.u_tex, 0);
    gles.ActiveTexture(gles2::TEXTURE0);
    gles.BindTexture(gles2::TEXTURE_2D, tex);
    // Premultiplied alpha, see draw_es1.
    gles.Enable(gles2::BLEND);
    gles.BlendFunc(gles2::ONE, gles2::ONE_MINUS_SRC_ALPHA);
    gles.Viewport(0, 0, info.drawable.0 as _, info.drawable.1 as _);
    gles.EnableVertexAttribArray(program.a_pos as u32);
    gles.EnableVertexAttribArray(program.a_uv as u32);
    gles.VertexAttribPointer(
        program.a_pos as u32,
        2,
        gles2::FLOAT,
        gles2::FALSE,
        16,
        std::ptr::null(),
    );
    gles.VertexAttribPointer(
        program.a_uv as u32,
        2,
        gles2::FLOAT,
        gles2::FALSE,
        16,
        8usize as *const _,
    );
    gles.DrawArrays(gles2::TRIANGLE_STRIP, 0, 4);

    for (&slot, &was) in slots.iter().zip(slot_was_enabled.iter()) {
        if was != 0 {
            gles.EnableVertexAttribArray(slot);
        } else {
            gles.DisableVertexAttribArray(slot);
        }
    }
    gles.UseProgram(old_program as u32);
    gles.BindBuffer(gles2::ARRAY_BUFFER, old_array_buffer as u32);
    gles.ActiveTexture(gles2::TEXTURE0);
    gles.BindTexture(gles2::TEXTURE_2D, old_texture0 as u32);
    gles.ActiveTexture(old_active_texture as u32);
    gles.BindTexture(gles2::TEXTURE_2D, old_texture as u32);
    gles.Viewport(
        old_viewport[0],
        old_viewport[1],
        old_viewport[2] as _,
        old_viewport[3] as _,
    );
    if blend_was_on {
        gles.Enable(gles2::BLEND);
    } else {
        gles.Disable(gles2::BLEND);
    }
    gles.BlendFunc(old_blend_src as u32, old_blend_dst as u32);
    let _ = gles.GetError();
}
