use crate::core::graphics::gl_utils::GpuFbo;
use crate::core::graphics::gpu::{PowCnt1, DISPLAY_HEIGHT, DISPLAY_WIDTH};
use crate::core::graphics::gpu_3d::registers_3d::{Gpu3DBuffer, Gpu3DRegisters, PolygonAttr, PolygonMode, PrimitiveType, SwapBuffers, TexImageParam, TextureCoordTransMode, TextureFormat, Vertex, Viewport};
use crate::core::graphics::gpu_3d::registers_3d::{POLYGON_LIMIT, VERTEX_LIMIT};
use crate::core::graphics::gpu_3d::texture_cache::Texture3DCache;
use crate::core::graphics::gpu_mem_buf::{GpuMemBuf, GpuMemRefs};
use crate::core::graphics::gpu_renderer::GpuRendererCommon;
use crate::core::graphics::gpu_shaders::{Gpu3DShaderDepthPrograms, Gpu3DShaderPrograms, GpuShadersPrograms};
use crate::core::memory::vram;
use crate::math::{vmult_vec4_mat4_no_store, Vectori32};
use crate::savestate::{Savestate, SavestateContext};
use crate::settings::{ListInner, SettingValue};
use crate::utils::{rgb5_to_float8, HeapArray, HeapArrayU8, HeapMem, PtrWrapper, StrErr};
use bilge::prelude::*;
use gl::types::GLuint;
use static_assertions::const_assert_eq;
#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::{vcvt_n_f32_s32, vcvtq_n_f32_s32, vget_low_s32, vsetq_lane_s32, vshr_n_s32, vst1_f32, vst1q_f32};
#[cfg(target_arch = "arm")]
use std::arch::arm::{vcvt_n_f32_s32, vcvtq_n_f32_s32, vget_low_s32, vsetq_lane_s32, vshr_n_s32, vst1_f32, vst1q_f32};
use std::hint::{assert_unchecked, spin_loop, unreachable_unchecked};
use std::intrinsics::unlikely;
use std::mem::{self, MaybeUninit};
use std::ops::{Deref, DerefMut};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

const UPSCALE_FACTORS: [f32; 8] = [1.0, 1.25, 1.5, 1.75, 2.0, 2.25, 2.5, 2.75];

#[repr(u8)]
#[derive(Copy, Clone, EnumIter, Eq, PartialEq)]
pub enum WidescreenOption {
    Off = 0,
    Only3d,
    Both,
}

impl Into<String> for WidescreenOption {
    fn into(self) -> String {
        match self {
            WidescreenOption::Off => "Off".to_string(),
            WidescreenOption::Only3d => "3D only".to_string(),
            WidescreenOption::Both => "2D + 3D".to_string(),
        }
    }
}

impl From<u8> for WidescreenOption {
    fn from(value: u8) -> Self {
        debug_assert!(value <= WidescreenOption::Both as u8);
        unsafe { mem::transmute(value) }
    }
}

#[bitsize(32)]
#[derive(Copy, Clone, FromBits)]
struct ClearColor {
    color: u15,
    fog: bool,
    alpha: u5,
    not_used: u3,
    clear_polygon_id: u6,
    not_used1: u2,
}

impl Default for ClearColor {
    fn default() -> Self {
        ClearColor::from(0)
    }
}

#[bitsize(16)]
#[derive(Copy, Clone, FromBits)]
struct Disp3DCnt {
    texture_mapping: bool,
    polygon_attr_shading: u1,
    alpha_test: bool,
    alpha_blending: bool,
    anti_aliasing: bool,
    edge_marking: bool,
    alpha_mode: bool,
    fog_master_enable: bool,
    fog_depth_shift: u4,
    color_buf_rdlines_underflow: bool,
    polygon_vertex_ram_overflow: bool,
    rear_plane_mode: u1,
    not_used: u1,
}

impl Default for Disp3DCnt {
    fn default() -> Self {
        Disp3DCnt::from(0)
    }
}

crate::savestate::impl_savestate_bytes!(Disp3DCnt, ClearColor);

#[derive(Clone, Savestate)]
struct Gpu3DRendererInner {
    disp_cnt: Disp3DCnt,
    edge_colors: [u16; 8],
    clear_color: ClearColor,
    clear_colorf: [f32; 4],
    clear_depth: u16,
    clear_depthf: f32,
    fog_color: u32,
    fog_offset: u16,
    fog_table: [u8; 32],
    toon_table: [u16; 32],
}

impl Default for Gpu3DRendererInner {
    fn default() -> Self {
        Gpu3DRendererInner {
            disp_cnt: Default::default(),
            edge_colors: [0; 8],
            clear_color: Default::default(),
            clear_colorf: [0.0; 4],
            clear_depth: 0x7FFF,
            clear_depthf: 1.0,
            fog_color: 0,
            fog_offset: 0,
            fog_table: [0; 32],
            toon_table: [0; 32],
        }
    }
}

pub struct Gpu3DGl {
    vertices_buf: GLuint,
    indices_buf: GLuint,
    program: Gpu3DShaderDepthPrograms,
    fbos: [Gpu3DFbo; 2],
}

pub struct Gpu3DFbo {
    inner: GpuFbo,
    pub regular_width: u32,
    upscale_factor_index: u8,
    pub widescreen: WidescreenOption,
    widescreen_coefficient: f32,
    pub widescreen_invert_coefficient: f32,
    guest_width: f32,
}

impl Gpu3DFbo {
    fn new(upscale_factor_index: u8, widescreen: WidescreenOption, widescreen_coefficient: f32) -> Result<Self, StrErr> {
        let upscale_factor = UPSCALE_FACTORS[upscale_factor_index as usize];

        let regular_width = DISPLAY_WIDTH as f32 * upscale_factor;
        let height = DISPLAY_HEIGHT as f32 * upscale_factor;
        let width = regular_width * widescreen_coefficient;

        let guest_width = (DISPLAY_WIDTH - 1) as f32 * widescreen_coefficient;

        let regular_width = (regular_width as u32) & !1;
        let width = (width as u32) & !1;
        let height = (height as u32) & !1;

        Ok(Gpu3DFbo {
            inner: GpuFbo::new(width, height, true, true)?,
            regular_width,
            upscale_factor_index,
            widescreen,
            widescreen_coefficient,
            widescreen_invert_coefficient: if widescreen == WidescreenOption::Only3d { 1.0 / widescreen_coefficient } else { 1.0 },
            guest_width,
        })
    }

    pub fn width(&self) -> u32 {
        self.inner.width
    }

    pub fn height(&self) -> u32 {
        self.inner.height
    }

    pub fn color(&self) -> GLuint {
        self.inner.color
    }

    pub fn fbo(&self) -> GLuint {
        self.inner.fbo
    }
}

impl Gpu3DGl {
    fn new(gpu_programs: &GpuShadersPrograms) -> Self {
        unsafe {
            let mut vertices_buf = 0;
            let mut indices_buf = 0;
            gl::GenBuffers(1, &mut vertices_buf);
            gl::GenBuffers(1, &mut indices_buf);
            gl::BindBuffer(gl::ARRAY_BUFFER, vertices_buf);
            gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, indices_buf);

            gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, 0);
            gl::BindBuffer(gl::ARRAY_BUFFER, 0);
            gl::UseProgram(0);

            Gpu3DGl {
                vertices_buf,
                indices_buf,
                program: gpu_programs.render_3d,
                fbos: [Gpu3DFbo::new(4, WidescreenOption::Off, 1.0).unwrap(), Gpu3DFbo::new(4, WidescreenOption::Off, 1.0).unwrap()],
            }
        }
    }
}

#[derive(Default)]
#[repr(C)]
struct Gpu3dPolygonAttr {
    tex_image_param: u32,
    pal_addr_poly_attr: u32,
}

const_assert_eq!(size_of::<Gpu3dPolygonAttr>(), 8);

#[derive(Default)]
struct Gpu3DTexMem {
    tex: HeapArrayU8<{ vram::TEX_REAR_PLANE_IMAGE_SIZE }>,
    pal: HeapArrayU8<{ vram::TEX_PAL_SIZE }>,
    vertices_buf: HeapArray<Gpu3DVertex, VERTEX_LIMIT>,
}

#[derive(Default, Copy, Clone)]
#[repr(C)]
struct Gpu3DVertex {
    coords: [f32; 4],
    tex_coords: [f32; 2],
    viewport: [u8; 4],
    color: [u8; 4],
    tex_size: [u8; 2],
}

const INDEX_LIMIT: usize = VERTEX_LIMIT * 3;

/// Frame-local geometry storage. On Vita this is allocated from vitaGL's GPU-mapped
/// RAM so Core1 writes the exact memory that GXM later consumes. Desktop builds keep
/// the same fixed-array interface backed by the normal heap.
struct GpuFrameArray<T, const SIZE: usize> {
    #[cfg(target_os = "vita")]
    ptr: *mut T,
    #[cfg(not(target_os = "vita"))]
    heap: HeapArray<T, SIZE>,
}

impl<T: Default, const SIZE: usize> Default for GpuFrameArray<T, SIZE> {
    fn default() -> Self {
        #[cfg(target_os = "vita")]
        unsafe {
            let ptr = crate::presenter::Presenter::gl_mem_align_ram(16, size_of::<T>() * SIZE) as *mut T;
            assert!(!ptr.is_null());
            ptr.write_bytes(0, SIZE);
            GpuFrameArray { ptr }
        }
        #[cfg(not(target_os = "vita"))]
        {
            GpuFrameArray {
                heap: HeapArray::default(),
            }
        }
    }
}

impl<T, const SIZE: usize> GpuFrameArray<T, SIZE> {
    #[inline]
    fn as_ptr(&self) -> *const T {
        #[cfg(target_os = "vita")]
        {
            self.ptr as *const T
        }
        #[cfg(not(target_os = "vita"))]
        {
            self.heap.as_ptr()
        }
    }

    #[inline]
    fn as_mut_ptr(&mut self) -> *mut T {
        #[cfg(target_os = "vita")]
        {
            self.ptr
        }
        #[cfg(not(target_os = "vita"))]
        {
            self.heap.as_mut_ptr()
        }
    }
}

impl<T, const SIZE: usize> Deref for GpuFrameArray<T, SIZE> {
    type Target = [T; SIZE];

    fn deref(&self) -> &Self::Target {
        unsafe { &*(self.as_ptr() as *const [T; SIZE]) }
    }
}

impl<T, const SIZE: usize> DerefMut for GpuFrameArray<T, SIZE> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *(self.as_mut_ptr() as *mut [T; SIZE]) }
    }
}

unsafe impl<T: Send, const SIZE: usize> Send for GpuFrameArray<T, SIZE> {}
unsafe impl<T: Sync, const SIZE: usize> Sync for GpuFrameArray<T, SIZE> {}

#[bitsize(32)]
#[derive(Copy, Clone, DebugBits, Default, FromBits)]
struct Gpu3DDrawAttr {
    mode: PolygonMode,
    render_back: bool,
    render_front: bool,
    trans_new_depth: bool,
    render_far_plane: bool,
    render_1_dot_polygons: bool,
    depth_test_equal: bool,
    fog: bool,
    id: u6,
    not_used: u1,
    pal_addr: u16,
}

impl From<PolygonAttr> for Gpu3DDrawAttr {
    fn from(value: PolygonAttr) -> Self {
        Gpu3DDrawAttr::new(
            value.mode(),
            value.render_back(),
            value.render_front(),
            value.trans_new_depth(),
            value.render_far_plane(),
            value.render_1_dot_polygons(),
            value.depth_test_equal(),
            value.fog(),
            value.id(),
            u1::new(0),
            0,
        )
    }
}

#[derive(Copy, Clone, Default)]
pub struct Gpu3DDraw {
    vertex_start_index: u16,
    vertex_count: u16,
    attr: PolygonAttr,
    pub tex_image_param: TexImageParam,
    pub pal_addr: u16,
    viewport: Viewport,
}

impl Gpu3DDraw {
    pub fn key(&self) -> u64 {
        self.tex_image_param.key() as u64 | ((self.pal_addr as u64) << 32)
    }
}

/// CPU-prepared 3D state for one DS frame.
///
/// Two instances are kept so the worker can eventually prepare frame N+1 while the
/// render thread consumes frame N. Stage A keeps the existing synchronization, but
/// all frame-owned geometry/state is isolated here instead of the shared scratch state.
struct Prepared3DFrame {
    assembled_draws: HeapArray<Gpu3DDraw, POLYGON_LIMIT>,
    assembled_draw_count: u16,
    // On Vita these buffers are GPU-mapped. Core1 prepares directly into the memory consumed
    // by GXM, so the render thread does not memcpy vertices or per-batch indices anymore.
    gpu_vertices: GpuFrameArray<Gpu3DVertex, VERTEX_LIMIT>,
    gpu_vertices_count: u16,
    gpu_indices: GpuFrameArray<u16, INDEX_LIMIT>,
    indices_opaque_count: usize,
    indices_translucent_start: usize,
    indices_translucent_count: usize,
    translucent_polygons: Vec<u16>,
    indices_opaque_batches: Vec<IndicesBatch>,
    indices_translucent_batches: Vec<IndicesBatch>,

    inner: Gpu3DRendererInner,
    pow_cnt1: PowCnt1,
    swap_buffers: SwapBuffers,
}

impl Default for Prepared3DFrame {
    fn default() -> Self {
        Self {
            assembled_draws: HeapArray::default(),
            assembled_draw_count: 0,
            gpu_vertices: GpuFrameArray::default(),
            gpu_vertices_count: 0,
            gpu_indices: GpuFrameArray::default(),
            indices_opaque_count: 0,
            indices_translucent_start: 0,
            indices_translucent_count: 0,
            translucent_polygons: Vec::new(),
            indices_opaque_batches: Vec::new(),
            indices_translucent_batches: Vec::new(),
            inner: Gpu3DRendererInner::default(),
            pow_cnt1: PowCnt1::from(0),
            swap_buffers: SwapBuffers::default(),
        }
    }
}

impl Prepared3DFrame {
    #[inline]
    fn reset_for_prepare(&mut self) {
        self.assembled_draw_count = 0;
        self.gpu_vertices_count = 0;
        self.indices_opaque_count = 0;
        self.indices_translucent_start = 0;
        self.indices_translucent_count = 0;
        self.translucent_polygons.clear();
        self.indices_opaque_batches.clear();
        self.indices_translucent_batches.clear();
    }
}

struct IndicesBatch {
    indices_offset: usize,
    tex_key: u64,
    tex: GLuint,
    tex_image_param: TexImageParam,
    attr: Gpu3DDrawAttr,
}

pub struct Gpu3DRenderer {
    pub dirty: bool,

    inners: [Gpu3DRendererInner; 2],
    buffer: HeapMem<Gpu3DBuffer>,
    gl: Gpu3DGl,

    prepared_frames: [Prepared3DFrame; 2],
    prepare_frame_index: usize,
    render_frame_index: usize,

    texture_cache: Texture3DCache,
    texture_ids_to_delete: Vec<GLuint>,
    vram_ready: AtomicBool,
    // Core1 previously busy-spun at 100% while waiting for the render thread to finish
    // reading VRAM. Keep a very short spin for sub-microsecond handoffs, then sleep.
    vram_ready_mutex: Mutex<()>,
    vram_ready_condvar: Condvar,

    mem: Gpu3DTexMem,
}

impl Gpu3DRenderer {
    pub fn upscale_factor_settings_value() -> SettingValue {
        SettingValue::List(ListInner::new(4, UPSCALE_FACTORS.map(|factor| format!("{factor}x")).to_vec()))
    }

    pub fn widescreen_settings_value() -> SettingValue {
        SettingValue::List(ListInner::new(0, WidescreenOption::iter().map(|option| option.into()).collect()))
    }

    pub fn new(gpu_programs: &GpuShadersPrograms) -> Self {
        Gpu3DRenderer {
            dirty: false,
            inners: [Gpu3DRendererInner::default(), Gpu3DRendererInner::default()],
            buffer: Default::default(),
            gl: Gpu3DGl::new(gpu_programs),

            prepared_frames: [Prepared3DFrame::default(), Prepared3DFrame::default()],
            prepare_frame_index: 0,
            render_frame_index: 1,

            texture_ids_to_delete: Vec::new(),
            texture_cache: Texture3DCache::new(),
            vram_ready: AtomicBool::new(false),
            vram_ready_mutex: Mutex::new(()),
            vram_ready_condvar: Condvar::new(),

            mem: Default::default(),
        }
    }

    pub fn init(&mut self) {
        self.dirty = false;
        self.inners[0] = Gpu3DRendererInner::default();
        self.inners[1] = Gpu3DRendererInner::default();
        self.buffer.reset_all();
        self.buffer.pow_cnt1 = PowCnt1::from(0);
        self.prepare_frame_index = 0;
        self.render_frame_index = 1;
        for frame in &mut self.prepared_frames {
            frame.reset_for_prepare();
            frame.inner = Gpu3DRendererInner::default();
            frame.pow_cnt1 = PowCnt1::from(0);
            frame.swap_buffers = SwapBuffers::default();
        }
        self.texture_cache.clear();

        unsafe {
            for fbo in &self.gl.fbos {
                gl::BindFramebuffer(gl::FRAMEBUFFER, fbo.fbo());
                gl::Viewport(0, 0, fbo.inner.width as _, fbo.inner.height as _);
                gl::ClearColor(0.0, 0.0, 0.0, 0.0);

                gl::Clear(gl::COLOR_BUFFER_BIT);
            }

            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
        }
    }

    pub fn invalidate(&mut self) {
        self.dirty = true;
    }

    // The 3d display registers (disp cnt, clear, fog, toon, edge) are written straight into the
    // renderer and live nowhere else, so savestates walk them here; runs on the cpu thread like
    // the io writes themselves
    pub fn savestate_registers(&mut self, state: &mut SavestateContext) {
        self.inners[1].savestate(state);
        if !state.is_save() {
            self.invalidate();
        }
    }

    pub fn get_disp_3d_cnt(&self) -> u16 {
        self.inners[1].disp_cnt.into()
    }

    pub fn set_disp_3d_cnt(&mut self, mut mask: u16, value: u16) {
        let new_cnt = Disp3DCnt::from(value);
        if new_cnt.color_buf_rdlines_underflow() {
            self.inners[1].disp_cnt.set_color_buf_rdlines_underflow(false);
        }
        if new_cnt.polygon_vertex_ram_overflow() {
            self.inners[1].disp_cnt.set_polygon_vertex_ram_overflow(false);
        }

        mask &= 0x4FFF;
        let new_value = (u16::from(self.inners[1].disp_cnt) & !mask) | (value & mask);
        if u16::from(self.inners[1].disp_cnt) != new_value {
            self.inners[1].disp_cnt = new_value.into();
            self.invalidate();
        }
    }

    pub fn set_edge_color(&mut self, index: usize, mut mask: u16, value: u16) {
        mask &= 0x7FFF;
        if value & mask == self.inners[1].edge_colors[index] & mask {
            return;
        }
        self.inners[1].edge_colors[index] = (self.inners[1].edge_colors[index] & !mask) | (value & mask);
        self.invalidate();
    }

    pub fn set_clear_color(&mut self, mut mask: u32, value: u32) {
        mask &= 0x3F1FFFFF;
        if value & mask == self.inners[1].clear_color.value & mask {
            return;
        }
        self.inners[1].clear_color.value = (self.inners[1].clear_color.value & !mask) | (value & mask);
        let [r, g, b] = rgb5_to_float8(u16::from(self.inners[1].clear_color.color()));
        self.inners[1].clear_colorf = [r, g, b, u8::from(self.inners[1].clear_color.alpha()) as f32 / 31f32];
        self.invalidate();
    }

    pub fn set_clear_depth(&mut self, mut mask: u16, value: u16) {
        mask &= 0x7FFF;
        if value & mask == self.inners[1].clear_depth & mask {
            return;
        }
        self.inners[1].clear_depth = (self.inners[1].clear_depth & !mask) | (value & mask);
        let depth = self.inners[1].clear_depth as u32;
        let expanded_depth = depth * 0x200 + ((depth + 1) / 0x8000) * 0x1FF;
        self.inners[1].clear_depthf = expanded_depth as f32 / 0xFFFFFF as f32;
        const TOLERANCE: f32 = 0x200 as f32 / 0xFFFFFF as f32;
        self.inners[1].clear_depthf += TOLERANCE;
        if self.inners[1].clear_depthf > 1.0 {
            self.inners[1].clear_depthf = 1.0;
        }
        self.invalidate();
    }

    pub fn set_toon_table(&mut self, index: usize, mut mask: u16, value: u16) {
        mask &= 0x7FFF;
        if value & mask == self.inners[1].toon_table[index] & mask {
            return;
        }
        self.inners[1].toon_table[index] = (self.inners[1].toon_table[index] & !mask) | (value & mask);
        self.invalidate();
    }

    pub fn set_fog_color(&mut self, mut mask: u32, value: u32) {
        mask &= 0x001F7FFF;
        if value & mask == self.inners[1].fog_color & mask {
            return;
        }
        self.inners[1].fog_color = (self.inners[1].fog_color & !mask) | (value & mask);
        self.invalidate();
    }

    pub fn set_fog_offset(&mut self, mut mask: u16, value: u16) {
        mask &= 0x7FFF;
        if value & mask == self.inners[1].fog_offset & mask {
            return;
        }
        self.inners[1].fog_offset = (self.inners[1].fog_offset & !mask) | (value & mask);
        self.invalidate();
    }

    pub fn set_fog_table(&mut self, index: usize, value: u8) {
        if value & 0x7F == self.inners[1].fog_table[index] & 0x7F {
            return;
        }
        self.inners[1].fog_table[index] = value & 0x7F;
        self.invalidate();
    }

    pub fn finish_scanline(&mut self, registers: &mut Gpu3DRegisters) {
        self.inners[0] = self.inners[1].clone();

        if registers.can_consume() {
            registers.swap_to_renderer(&mut self.buffer);
        }
    }

    unsafe fn process_vertices(&mut self) {
        let mut clip_matrix_index = usize::MAX;
        let mut clip_matrix = MaybeUninit::uninit().assume_init();

        for i in 0..self.buffer.vertices_count {
            let vertex: &mut Vertex = mem::transmute(self.buffer.vertices.get_unchecked_mut(i as usize));
            let coords = vertex.coords.fixed.vld();

            assert_unchecked(vertex.s.indices.clip_matrix as usize != usize::MAX);
            if clip_matrix_index != vertex.s.indices.clip_matrix as usize {
                clip_matrix_index = vertex.s.indices.clip_matrix as usize;
                clip_matrix = self.buffer.clip_matrices[clip_matrix_index].vld();
            }
            let trans_coords = vmult_vec4_mat4_no_store(coords, clip_matrix);
            let trans_coords_float = vcvtq_n_f32_s32::<12>(trans_coords);
            vst1q_f32(vertex.coords.float.0.as_mut_ptr(), trans_coords_float);

            let tex_coord_trans_mode = vertex.data.coord_trans_mode();
            if tex_coord_trans_mode != TextureCoordTransMode::None && (vertex.s.indices.tex_matrix as usize) < self.buffer.tex_matrices.len() {
                let mut tex_matrix = self.buffer.tex_matrices[vertex.s.indices.tex_matrix as usize].vld();

                let ret = match tex_coord_trans_mode {
                    TextureCoordTransMode::TexCoord => {
                        let vector = Vectori32::<4>::new([(vertex.s.indices.tex_coords[0] as i32) << 8, (vertex.s.indices.tex_coords[1] as i32) << 8, 1 << 8, 1 << 8]);
                        let ret = vmult_vec4_mat4_no_store(vector.vld(), tex_matrix);
                        vshr_n_s32::<8>(vget_low_s32(ret))
                    }
                    TextureCoordTransMode::Normal => {
                        tex_matrix[3] = vsetq_lane_s32::<0>((vertex.s.indices.tex_coords[0] as i32) << 12, tex_matrix[3]);
                        tex_matrix[3] = vsetq_lane_s32::<1>((vertex.s.indices.tex_coords[1] as i32) << 12, tex_matrix[3]);
                        let normal = Vectori32::<4>::new([vertex.normal[0] as i32, vertex.normal[1] as i32, vertex.normal[2] as i32, 1 << 12]);
                        let ret = vmult_vec4_mat4_no_store(normal.vld(), tex_matrix);
                        vshr_n_s32::<12>(vget_low_s32(ret))
                    }
                    TextureCoordTransMode::Vertex => {
                        tex_matrix[3] = vsetq_lane_s32::<0>((vertex.s.indices.tex_coords[0] as i32) << 12, tex_matrix[3]);
                        tex_matrix[3] = vsetq_lane_s32::<1>((vertex.s.indices.tex_coords[1] as i32) << 12, tex_matrix[3]);
                        let ret = vmult_vec4_mat4_no_store(trans_coords, tex_matrix);
                        vshr_n_s32::<12>(vget_low_s32(ret))
                    }
                    _ => unreachable_unchecked(),
                };

                let ret = vcvt_n_f32_s32::<4>(ret);
                vst1_f32(vertex.s.trans_tex_coords.0.as_mut_ptr(), ret);
            } else {
                let tex_coords = vertex.s.indices.tex_coords;
                vertex.s.trans_tex_coords[0] = tex_coords[0] as f32 / 16.0;
                vertex.s.trans_tex_coords[1] = tex_coords[1] as f32 / 16.0;
            }
        }
    }

    unsafe fn assemble_draws(&mut self, frame_index: usize) {
        let buffer = &self.buffer;
        let frame = &mut self.prepared_frames[frame_index];
        frame.assembled_draw_count = 0;

        let add_draw = |frame: &mut Prepared3DFrame, vertex_start_index, vertex_count, polygon_attr, tex_image_param, pal_addr, viewport| {
            *frame.assembled_draws.get_unchecked_mut(frame.assembled_draw_count as usize) = Gpu3DDraw {
                vertex_start_index,
                vertex_count,
                attr: polygon_attr,
                tex_image_param,
                pal_addr,
                viewport,
            };

            frame.assembled_draw_count += 1;
            frame.assembled_draw_count != POLYGON_LIMIT as u16
        };

        let mut viewport = MaybeUninit::uninit().assume_init();
        let mut polygon_attr: PolygonAttr = MaybeUninit::uninit().assume_init();
        let mut draw_vertex_count: u16 = 0;
        let mut tex_image_param = MaybeUninit::uninit().assume_init();
        let mut pal_addr = MaybeUninit::uninit().assume_init();

        for i in 0..buffer.vertices_count {
            let vertex = buffer.vertices.get_unchecked(i as usize);
            assert_unchecked(i != 0 || vertex.data.begin_vtxs());

            let begin_vtxs = vertex.data.begin_vtxs();
            let polygon_index = u16::from(vertex.data.polygon_index());

            if begin_vtxs {
                let draw_complete = match polygon_attr.primitive_type() {
                    PrimitiveType::TriangleStrips => draw_vertex_count >= 3,
                    PrimitiveType::QuadliteralStrips => draw_vertex_count >= 4 && draw_vertex_count % 2 == 0,
                    _ => false,
                };
                if draw_complete && !add_draw(frame, i - draw_vertex_count, draw_vertex_count, polygon_attr, tex_image_param, pal_addr, viewport) {
                    return;
                }

                let polygon = buffer.polygons.get_unchecked(polygon_index as usize);
                viewport = polygon.viewport;
                polygon_attr = polygon.attr;
                draw_vertex_count = 0;
                tex_image_param = polygon.tex_image_param;
                pal_addr = polygon.palette_addr;
            }

            draw_vertex_count += 1;
            let draw_complete = match polygon_attr.primitive_type() {
                PrimitiveType::SeparateTriangles => {
                    let ret = draw_vertex_count == 3;
                    if ret {
                        draw_vertex_count = 0;
                    }
                    ret
                }
                PrimitiveType::SeparateQuadliterals => draw_vertex_count % 4 == 0,
                _ => false,
            };
            if draw_complete {
                let polygon = buffer.polygons.get_unchecked(polygon_index as usize);
                if !add_draw(
                    frame,
                    i + 1 - polygon_attr.primitive_type().vertex_count() as u16,
                    polygon_attr.primitive_type().vertex_count() as u16,
                    polygon_attr,
                    polygon.tex_image_param,
                    polygon.palette_addr,
                    viewport,
                ) {
                    return;
                }
            }
        }

        let draw_complete = match polygon_attr.primitive_type() {
            PrimitiveType::TriangleStrips => draw_vertex_count >= 3,
            PrimitiveType::QuadliteralStrips => draw_vertex_count >= 4 && draw_vertex_count % 2 == 0,
            _ => false,
        };
        if draw_complete {
            add_draw(
                frame,
                buffer.vertices_count - draw_vertex_count,
                draw_vertex_count,
                polygon_attr,
                tex_image_param,
                pal_addr,
                viewport,
            );
        }
    }

    #[inline]
    unsafe fn add_prepared_indices_batch<const TRANSLUCENT_ONLY: bool>(
        frame: &mut Prepared3DFrame,
        active_texture_key: u64,
        active_tex_image_param: TexImageParam,
        active_polygon_attr: Gpu3DDrawAttr,
    ) {
        let (indices_len, indices_batch) = if TRANSLUCENT_ONLY {
            (frame.indices_translucent_count, &mut frame.indices_translucent_batches)
        } else {
            (frame.indices_opaque_count, &mut frame.indices_opaque_batches)
        };
        if indices_len != 0 {
            indices_batch.push(IndicesBatch {
                indices_offset: indices_len,
                tex_key: active_texture_key,
                tex: u32::MAX,
                tex_image_param: active_tex_image_param,
                attr: active_polygon_attr,
            });
        }
    }

    unsafe fn add_prepared_vertices<const TRANSLUCENT_ONLY: bool>(
        frame: &mut Prepared3DFrame,
        source_vertices: *const Vertex,
        draw_index: u16,
        active_texture_key: &mut u64,
        active_tex_image_param: &mut TexImageParam,
        active_polygon_attr: &mut Gpu3DDrawAttr,
    ) {
        assert_unchecked((draw_index as usize) < POLYGON_LIMIT);
        let draw = frame.assembled_draws[draw_index as usize];
        let primitive_type = draw.attr.primitive_type();

        let texture_key = if draw.tex_image_param.format() != TextureFormat::None {
            draw.key()
        } else {
            u64::MAX
        };
        let draw_attr = Gpu3DDrawAttr::from(draw.attr);
        let tex_image_param = u32::from(draw.tex_image_param) & 0x1C0F0000;

        if *active_texture_key != texture_key
            || u32::from(*active_tex_image_param) != tex_image_param
            || active_polygon_attr.value != draw_attr.value
        {
            Self::add_prepared_indices_batch::<TRANSLUCENT_ONLY>(
                frame,
                *active_texture_key,
                *active_tex_image_param,
                *active_polygon_attr,
            );
            *active_texture_key = texture_key;
            *active_tex_image_param = TexImageParam::from(tex_image_param);
            *active_polygon_attr = draw_attr;
        }

        let indices_base = if TRANSLUCENT_ONLY { frame.indices_translucent_start } else { 0 };
        let mut indices_count = if TRANSLUCENT_ONLY {
            frame.indices_translucent_count
        } else {
            frame.indices_opaque_count
        };
        let vertex_index = frame.gpu_vertices_count;
        let mut emit = |values: &[u16]| {
            let start = indices_base + indices_count;
            let end = start + values.len();
            debug_assert!(end <= INDEX_LIMIT);
            frame.gpu_indices[start..end].copy_from_slice(values);
            indices_count += values.len();
        };

        match primitive_type {
            PrimitiveType::SeparateTriangles => emit(&[vertex_index, vertex_index + 1, vertex_index + 2]),
            PrimitiveType::SeparateQuadliterals => emit(&[
                vertex_index,
                vertex_index + 1,
                vertex_index + 2,
                vertex_index,
                vertex_index + 2,
                vertex_index + 3,
            ]),
            PrimitiveType::TriangleStrips => {
                emit(&[vertex_index, vertex_index + 1, vertex_index + 2]);
                for i in 3..draw.vertex_count {
                    let index = i + vertex_index;
                    emit(&[index - 2, index - (!i & 1), index - (i & 1)]);
                }
            }
            PrimitiveType::QuadliteralStrips => {
                emit(&[vertex_index, vertex_index + 1, vertex_index + 3, vertex_index, vertex_index + 3, vertex_index + 2]);
                for i in (vertex_index + 4..vertex_index + draw.vertex_count).step_by(2) {
                    emit(&[i - 2, i - 1, i + 1, i - 2, i + 1, i]);
                }
            }
        }

        if TRANSLUCENT_ONLY {
            frame.indices_translucent_count = indices_count;
        } else {
            frame.indices_opaque_count = indices_count;
        }

        const Z_EQUAL_MARGIN: f32 = 2.0 * 0x200 as f32 / 0xFFFFFF as f32;
        let z_bias = if draw.attr.depth_test_equal() && !frame.swap_buffers.depth_buffering_w() {
            Z_EQUAL_MARGIN
        } else {
            0.0
        };

        for i in draw.vertex_start_index..draw.vertex_start_index + draw.vertex_count {
            let vertex = &*source_vertices.add(i as usize);
            let color = u16::from(vertex.data.color());

            let mut gpu_vertex = Gpu3DVertex {
                coords: vertex.coords.float.0,
                tex_coords: [vertex.s.trans_tex_coords[0], vertex.s.trans_tex_coords[1]],
                tex_size: [1 << u8::from(draw.tex_image_param.size_s_shift()), 1 << u8::from(draw.tex_image_param.size_t_shift())],
                viewport: [draw.viewport.x1(), draw.viewport.y1(), draw.viewport.x2(), draw.viewport.y2()],
                color: [(color & 0x1F) as u8, ((color >> 5) & 0x1F) as u8, ((color >> 10) & 0x1F) as u8, u8::from(draw.attr.alpha())],
            };
            gpu_vertex.coords[2] -= z_bias * gpu_vertex.coords[3];

            *frame.gpu_vertices.get_unchecked_mut(frame.gpu_vertices_count as usize) = gpu_vertex;
            frame.gpu_vertices_count += 1;
        }
    }

    unsafe fn prepare_render_geometry(&mut self, frame_index: usize, source_vertices: *const Vertex) {
        let frame = &mut self.prepared_frames[frame_index];
        frame.gpu_vertices_count = 0;
        frame.indices_opaque_count = 0;
        frame.indices_translucent_start = 0;
        frame.indices_translucent_count = 0;
        frame.translucent_polygons.clear();
        frame.indices_opaque_batches.clear();
        frame.indices_translucent_batches.clear();

        let mut active_texture_key = u64::MAX;
        let mut active_tex_image_param = TexImageParam::default();
        let mut active_polygon_attr = Gpu3DDrawAttr::default();

        for i in 0..frame.assembled_draw_count {
            let draw = frame.assembled_draws[i as usize];
            if draw.attr.is_translucent() || draw.tex_image_param.is_translucent() {
                frame.translucent_polygons.push(i);
            } else {
                Self::add_prepared_vertices::<false>(
                    frame,
                    source_vertices,
                    i,
                    &mut active_texture_key,
                    &mut active_tex_image_param,
                    &mut active_polygon_attr,
                );
            }
        }
        Self::add_prepared_indices_batch::<false>(
            frame,
            active_texture_key,
            active_tex_image_param,
            active_polygon_attr,
        );
        frame.indices_translucent_start = frame.indices_opaque_count;

        active_texture_key = u64::MAX;
        active_tex_image_param = TexImageParam::default();
        active_polygon_attr = Gpu3DDrawAttr::default();

        for i in 0..frame.translucent_polygons.len() {
            let draw_index = *frame.translucent_polygons.get_unchecked(i);
            Self::add_prepared_vertices::<true>(
                frame,
                source_vertices,
                draw_index,
                &mut active_texture_key,
                &mut active_tex_image_param,
                &mut active_polygon_attr,
            );
        }
        Self::add_prepared_indices_batch::<true>(
            frame,
            active_texture_key,
            active_tex_image_param,
            active_polygon_attr,
        );
    }

    pub unsafe fn populate_tex_cache(&mut self, frame_index: usize, mem_buf: &mut GpuMemBuf, mem_refs: &GpuMemRefs) {
        self.texture_cache.mark_dirty(mem_buf, mem_refs);

        // Geometry preparation already collapses polygons into draw batches. Use those batches as
        // the texture working set instead of walking every assembled polygon again.
        let frame = &self.prepared_frames[frame_index];
        let mut last_key = u64::MAX;
        for batch in frame
            .indices_opaque_batches
            .iter()
            .chain(frame.indices_translucent_batches.iter())
        {
            let key = batch.tex_key;
            if key == u64::MAX || key == last_key {
                continue;
            }
            let _ = self.texture_cache.get(key, mem_buf, mem_refs, &mut self.texture_ids_to_delete);
            last_key = key;
        }

        self.texture_cache.reset_usage();
        mem_buf.vram_banks.dirty_sections.clear();
    }

    pub unsafe fn process_polygons(&mut self, common: &mut GpuRendererCommon, mem_refs: &GpuMemRefs) {
        let frame_index = self.prepare_frame_index;
        self.prepared_frames[frame_index].reset_for_prepare();
        self.prepared_frames[frame_index].pow_cnt1 = self.buffer.pow_cnt1;
        self.prepared_frames[frame_index].swap_buffers = self.buffer.swap_buffers;
        self.prepared_frames[frame_index].inner = self.inners[0].clone();

        if self.buffer.pow_cnt1 != common.pow_cnt1[0] {
            self.render_frame_index = frame_index;
            self.prepare_frame_index ^= 1;
            return;
        }

        self.process_vertices();
        self.assemble_draws(frame_index);

        // Consume the transformed DS vertices directly from Gpu3DBuffer. The previous pipeline
        // copied the entire vertex array into Prepared3DFrame only to convert it immediately,
        // wasting memory bandwidth on Core1.
        let source_vertices = self.buffer.vertices.as_ptr();
        self.prepare_render_geometry(frame_index, source_vertices);
        self.buffer.vertices_count = 0;

        self.wait_for_vram_ready();
        self.populate_tex_cache(frame_index, &mut common.mem_buf, mem_refs);

        self.render_frame_index = frame_index;
        self.prepare_frame_index ^= 1;
    }

    #[inline]
    fn wait_for_vram_ready(&self) {
        // Most frames reach the handoff quickly. A tiny spin avoids a kernel sleep/wake when the
        // render thread is only a few instructions behind, but unlike the old unbounded spin it
        // cannot pin Core1 at ~100% while waiting on VRAM copies or 2D work.
        const SPIN_ITERS: usize = 128;
        for _ in 0..SPIN_ITERS {
            if self.vram_ready.load(Ordering::Acquire) {
                return;
            }
            spin_loop();
        }

        if self.vram_ready.load(Ordering::Acquire) {
            return;
        }

        let guard = self.vram_ready_mutex.lock().unwrap();
        let _guard = self
            .vram_ready_condvar
            .wait_while(guard, |_| !self.vram_ready.load(Ordering::Acquire))
            .unwrap();
    }

    pub fn on_render_start(&self) {
        self.vram_ready.store(false, Ordering::Release);
    }

    pub fn set_tex_ptrs(&mut self, refs: &mut GpuMemRefs) {
        unsafe {
            refs.tex_rear_plane_image = PtrWrapper::new(mem::transmute(self.mem.tex.as_mut_ptr()));
            refs.tex_pal = PtrWrapper::new(mem::transmute(self.mem.pal.as_mut_ptr()));
        }
    }

    pub fn on_vram_ready(&self) {
        self.vram_ready.store(true, Ordering::Release);
        self.vram_ready_condvar.notify_one();
    }

    pub fn get_fbo(&mut self, swap: bool, upscale_factor_index: u8, widescreen: WidescreenOption, widescreen_coefficient: f32) -> &Gpu3DFbo {
        let fbo = &mut self.gl.fbos[swap as usize];
        if fbo.upscale_factor_index != upscale_factor_index || fbo.widescreen != widescreen || fbo.widescreen_coefficient != widescreen_coefficient {
            *fbo = Gpu3DFbo::new(upscale_factor_index, widescreen, widescreen_coefficient).unwrap();
        }
        fbo
    }

    unsafe fn draw_elements(translucent_only: bool, program: &Gpu3DShaderPrograms, indices_base: usize, indices_batch: &[IndicesBatch]) {
        let mut previous_offset = 0;

        // VitaGL state changes are not free. Batches are already ordered for DS correctness, so
        // keep that order but avoid re-emitting states that are identical to the previous batch.
        let mut bound_texture = u32::MAX;
        let mut bound_wrap_s = i32::MIN;
        let mut bound_wrap_t = i32::MIN;
        let mut uniform_tex_image_param = u32::MAX;
        let mut uniform_polygon_attr = u32::MAX;
        let mut depth_mask: Option<bool> = None;
        let mut color_mask: Option<[bool; 4]> = None;
        let mut stencil_mask = u32::MAX;
        let mut stencil_func: Option<(u32, i32, u32)> = None;
        let mut stencil_op: Option<(u32, u32, u32)> = None;
        let mut cull_enabled: Option<bool> = None;
        let mut cull_face = u32::MAX;

        gl::ActiveTexture(gl::TEXTURE0);

        for batch in indices_batch {
            if batch.tex_image_param.format() != TextureFormat::None {
                debug_assert_ne!(batch.tex, u32::MAX);

                let wrap_s = if batch.tex_image_param.repeat_s() {
                    if batch.tex_image_param.flip_s() {
                        gl::MIRRORED_REPEAT
                    } else {
                        gl::REPEAT
                    }
                } else {
                    gl::CLAMP_TO_EDGE
                } as i32;
                let wrap_t = if batch.tex_image_param.repeat_t() {
                    if batch.tex_image_param.flip_t() {
                        gl::MIRRORED_REPEAT
                    } else {
                        gl::REPEAT
                    }
                } else {
                    gl::CLAMP_TO_EDGE
                } as i32;

                if bound_texture != batch.tex {
                    gl::BindTexture(gl::TEXTURE_2D, batch.tex);
                    bound_texture = batch.tex;
                    // Texture parameters belong to the texture object. Conservatively refresh the
                    // cached values on rebind; consecutive polygons using the same texture skip them.
                    bound_wrap_s = i32::MIN;
                    bound_wrap_t = i32::MIN;
                }
                if bound_wrap_s != wrap_s {
                    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, wrap_s);
                    bound_wrap_s = wrap_s;
                }
                if bound_wrap_t != wrap_t {
                    gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, wrap_t);
                    bound_wrap_t = wrap_t;
                }
            }

            let tex_param = u32::from(batch.tex_image_param);
            if uniform_tex_image_param != tex_param {
                gl::Uniform1fv(program.tex_image_param, 1, (&tex_param as *const u32).cast());
                uniform_tex_image_param = tex_param;
            }

            let set_depth_mask = |value: bool, cached: &mut Option<bool>| {
                if *cached != Some(value) {
                    gl::DepthMask(if value { gl::TRUE } else { gl::FALSE });
                    *cached = Some(value);
                }
            };
            let set_color_mask = |value: [bool; 4], cached: &mut Option<[bool; 4]>| {
                if *cached != Some(value) {
                    gl::ColorMask(
                        if value[0] { gl::TRUE } else { gl::FALSE },
                        if value[1] { gl::TRUE } else { gl::FALSE },
                        if value[2] { gl::TRUE } else { gl::FALSE },
                        if value[3] { gl::TRUE } else { gl::FALSE },
                    );
                    *cached = Some(value);
                }
            };
            let set_stencil_mask = |value: u32, cached: &mut u32| {
                if *cached != value {
                    gl::StencilMask(value);
                    *cached = value;
                }
            };
            let set_stencil_func = |value: (u32, i32, u32), cached: &mut Option<(u32, i32, u32)>| {
                if *cached != Some(value) {
                    gl::StencilFunc(value.0, value.1, value.2);
                    *cached = Some(value);
                }
            };
            let set_stencil_op = |value: (u32, u32, u32), cached: &mut Option<(u32, u32, u32)>| {
                if *cached != Some(value) {
                    gl::StencilOp(value.0, value.1, value.2);
                    *cached = Some(value);
                }
            };

            if translucent_only {
                set_depth_mask(batch.attr.trans_new_depth(), &mut depth_mask);

                if batch.attr.mode() == PolygonMode::Shadow {
                    set_color_mask([false; 4], &mut color_mask);
                    set_stencil_mask(0x80, &mut stencil_mask);
                    if u8::from(batch.attr.id()) == 0 {
                        set_stencil_func((gl::ALWAYS, 0x80, 0x80), &mut stencil_func);
                        set_stencil_op((gl::KEEP, gl::REPLACE, gl::KEEP), &mut stencil_op);
                    } else {
                        set_stencil_func((gl::NOTEQUAL, u8::from(batch.attr.id()) as i32, 0x3F), &mut stencil_func);
                        set_stencil_op((gl::ZERO, gl::KEEP, gl::KEEP), &mut stencil_op);
                    }
                } else {
                    set_color_mask([true; 4], &mut color_mask);
                    set_stencil_mask(0x7F, &mut stencil_mask);
                    set_stencil_func((gl::NOTEQUAL, (u8::from(batch.attr.id()) | 0x40) as i32, 0x7F), &mut stencil_func);
                    set_stencil_op((gl::KEEP, gl::KEEP, gl::REPLACE), &mut stencil_op);
                }
            } else {
                set_color_mask([true; 4], &mut color_mask);
                set_stencil_mask(0x7F, &mut stencil_mask);
                set_stencil_func((gl::ALWAYS, u8::from(batch.attr.id()) as i32, 0x7F), &mut stencil_func);
                set_stencil_op((gl::KEEP, gl::KEEP, gl::REPLACE), &mut stencil_op);
            }

            let needs_cull = !batch.attr.render_back() || !batch.attr.render_front();
            if cull_enabled != Some(needs_cull) {
                if needs_cull {
                    gl::Enable(gl::CULL_FACE);
                } else {
                    gl::Disable(gl::CULL_FACE);
                }
                cull_enabled = Some(needs_cull);
            }
            if needs_cull {
                let face = match (batch.attr.render_back(), batch.attr.render_front()) {
                    (false, false) => gl::FRONT_AND_BACK,
                    (true, false) => gl::FRONT,
                    (false, true) => gl::BACK,
                    _ => unreachable_unchecked(),
                };
                if cull_face != face {
                    gl::CullFace(face);
                    cull_face = face;
                }
            }

            let attr = u32::from(batch.attr);
            if uniform_polygon_attr != attr {
                gl::Uniform1fv(program.polygon_attrs, 1, (&attr as *const u32).cast());
                uniform_polygon_attr = attr;
            }

            let count = batch.indices_offset - previous_offset;
            let byte_offset = (indices_base + previous_offset) * size_of::<u16>();
            gl::DrawElements(gl::TRIANGLES, count as _, gl::UNSIGNED_SHORT, byte_offset as *const _);

            if translucent_only && batch.attr.mode() == PolygonMode::Shadow && u8::from(batch.attr.id()) != 0 {
                set_color_mask([true; 4], &mut color_mask);
                set_stencil_func((gl::EQUAL, 0x80, 0x80), &mut stencil_func);
                set_stencil_op((gl::KEEP, gl::KEEP, gl::KEEP), &mut stencil_op);

                gl::DrawElements(gl::TRIANGLES, count as _, gl::UNSIGNED_SHORT, ptr as _);

                set_stencil_mask(0x80, &mut stencil_mask);
                gl::Clear(gl::STENCIL_BUFFER_BIT);
            }

            previous_offset = batch.indices_offset;
        }
    }

    /// Convert frame-local texture keys into stable GL object ids on the render thread.
    ///
    /// The 3D worker is blocked while this runs under the current handoff protocol. Once ids are
    /// copied into Prepared3DFrame, later cache replacement can no longer invalidate the frame by
    /// moving/freeing a cache allocation. GL deletions stay deferred until a later render pass.
    unsafe fn resolve_prepared_texture_ids(&mut self, frame_index: usize) {
        let texture_cache = &mut self.texture_cache;
        let frame = &mut self.prepared_frames[frame_index];

        let mut last_key = u64::MAX;
        let mut last_texture_id = u32::MAX;
        for batch in &mut frame.indices_opaque_batches {
            batch.tex = if batch.tex_image_param.format() == TextureFormat::None {
                u32::MAX
            } else {
                if batch.tex_key != last_key {
                    last_key = batch.tex_key;
                    last_texture_id = texture_cache.resolve_texture_id(batch.tex_key).unwrap_or(u32::MAX);
                }
                last_texture_id
            };
            if batch.tex_image_param.format() != TextureFormat::None {
                debug_assert_ne!(batch.tex, u32::MAX);
            }
        }

        // Keep the memo across opaque -> translucent; many games reuse the same atlas.
        for batch in &mut frame.indices_translucent_batches {
            batch.tex = if batch.tex_image_param.format() == TextureFormat::None {
                u32::MAX
            } else {
                if batch.tex_key != last_key {
                    last_key = batch.tex_key;
                    last_texture_id = texture_cache.resolve_texture_id(batch.tex_key).unwrap_or(u32::MAX);
                }
                last_texture_id
            };
            if batch.tex_image_param.format() != TextureFormat::None {
                debug_assert_ne!(batch.tex, u32::MAX);
            }
        }
    }

    /// Finish the published frame's texture ownership while the old handoff is still exclusive.
    /// After this returns, render() only consumes frame-local GLuints and GL scratch state.
    pub unsafe fn finalize_prepared_frame_for_render(&mut self) {
        let frame_index = self.render_frame_index;
        if !self.texture_ids_to_delete.is_empty() {
            gl::DeleteTextures(self.texture_ids_to_delete.len() as _, self.texture_ids_to_delete.as_ptr());
            self.texture_ids_to_delete.clear();
        }
        self.resolve_prepared_texture_ids(frame_index);
    }

    pub unsafe fn render(&mut self, upscale_factor_index: u8, widescreen: WidescreenOption, widescreen_coefficient: f32) {
        let frame_index = self.render_frame_index;
        let frame_pow_cnt1 = self.prepared_frames[frame_index].pow_cnt1;
        let frame_swap_buffers = self.prepared_frames[frame_index].swap_buffers;

        let fbo = self.get_fbo(frame_pow_cnt1.display_swap(), upscale_factor_index, widescreen, widescreen_coefficient);
        gl::BindFramebuffer(gl::FRAMEBUFFER, fbo.fbo());
        gl::Viewport(0, 0, fbo.inner.width as _, fbo.inner.height as _);

        let guest_width = fbo.guest_width;

        let [r, g, b, a] = self.prepared_frames[frame_index].inner.clear_colorf;
        gl::ClearColor(r, g, b, a);

        // gl::ClearDepth(self.inners[0].clear_depthf as _);
        gl::StencilMask(0xFF);
        gl::Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT | gl::STENCIL_BUFFER_BIT);

        let gpu_vertices_count = self.prepared_frames[frame_index].gpu_vertices_count;
        if gpu_vertices_count == 0 {
            return;
        }

        // Frame geometry is already in GPU-mapped RAM on Vita. No render-thread memcpy.

        // println!("render");

        let program = self.gl.program.get_program(frame_swap_buffers.depth_buffering_w());
        gl::UseProgram(program.program);

        gl::Enable(gl::DEPTH_TEST);
        gl::DepthFunc(gl::LEQUAL);

        gl::Enable(gl::STENCIL_TEST);

        gl::Uniform1f(program.screen_width, guest_width);

        let mut toon_table = [0f32; 32 * 3];
        for i in 0..32 {
            let [r, g, b] = rgb5_to_float8(self.prepared_frames[frame_index].inner.toon_table[i]);
            toon_table[i * 3] = r;
            toon_table[i * 3 + 1] = g;
            toon_table[i * 3 + 2] = b;
        }
        gl::Uniform3fv(program.toon_table, 32, toon_table.as_ptr());
        gl::Uniform1f(program.toon_highlight, u8::from(self.prepared_frames[frame_index].inner.disp_cnt.polygon_attr_shading()) as f32);

        gl::BindBuffer(gl::ARRAY_BUFFER, self.gl.vertices_buf);
        gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, self.gl.indices_buf);
        let frame_indices_count =
            self.prepared_frames[frame_index].indices_translucent_start + self.prepared_frames[frame_index].indices_translucent_count;
        #[cfg(not(target_os = "vita"))]
        {
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (size_of::<Gpu3DVertex>() * gpu_vertices_count as usize) as _,
                self.prepared_frames[frame_index].gpu_vertices.as_ptr() as _,
                gl::DYNAMIC_DRAW,
            );
            gl::BufferData(
                gl::ELEMENT_ARRAY_BUFFER,
                (size_of::<u16>() * frame_indices_count) as _,
                self.prepared_frames[frame_index].gpu_indices.as_ptr() as _,
                gl::DYNAMIC_DRAW,
            );
        }
        #[cfg(target_os = "vita")]
        {
            crate::presenter::Presenter::gl_buffer_data(gl::ARRAY_BUFFER, self.prepared_frames[frame_index].gpu_vertices.as_ptr() as _);
            crate::presenter::Presenter::gl_buffer_data(gl::ELEMENT_ARRAY_BUFFER, self.prepared_frames[frame_index].gpu_indices.as_ptr() as _);
        }

        gl::EnableVertexAttribArray(0);
        gl::VertexAttribPointer(0, 4, gl::FLOAT, gl::FALSE, size_of::<Gpu3DVertex>() as _, mem::offset_of!(Gpu3DVertex, coords) as _);

        gl::EnableVertexAttribArray(1);
        gl::VertexAttribPointer(1, 2, gl::FLOAT, gl::FALSE, size_of::<Gpu3DVertex>() as _, mem::offset_of!(Gpu3DVertex, tex_coords) as _);

        gl::EnableVertexAttribArray(2);
        gl::VertexAttribPointer(2, 4, gl::UNSIGNED_BYTE, gl::FALSE, size_of::<Gpu3DVertex>() as _, mem::offset_of!(Gpu3DVertex, viewport) as _);

        gl::EnableVertexAttribArray(3);
        gl::VertexAttribPointer(3, 4, gl::UNSIGNED_BYTE, gl::FALSE, size_of::<Gpu3DVertex>() as _, mem::offset_of!(Gpu3DVertex, color) as _);

        gl::EnableVertexAttribArray(4);
        gl::VertexAttribPointer(4, 2, gl::UNSIGNED_BYTE, gl::FALSE, size_of::<Gpu3DVertex>() as _, mem::offset_of!(Gpu3DVertex, tex_size) as _);

        let frame = &self.prepared_frames[frame_index];
        if frame.indices_opaque_count != 0 {
            gl::DepthMask(gl::TRUE);
            gl::Disable(gl::BLEND);
            Self::draw_elements(false, program, 0, &frame.indices_opaque_batches);
        }

        if frame.indices_translucent_count != 0 {
            gl::Enable(gl::BLEND);

            gl::BlendFuncSeparate(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA, gl::ONE, gl::ONE);
            gl::BlendEquationSeparate(gl::FUNC_ADD, gl::MAX);

            Self::draw_elements(true, program, frame.indices_translucent_start, &frame.indices_translucent_batches);
        }

        gl::DepthMask(gl::TRUE);
        gl::Disable(gl::DEPTH_TEST);
        gl::Disable(gl::BLEND);
        gl::BindBuffer(gl::ARRAY_BUFFER, 0);
        gl::BindBuffer(gl::ELEMENT_ARRAY_BUFFER, 0);
        gl::BindTexture(gl::TEXTURE_2D, 0);
        gl::UseProgram(0);
        gl::BindFramebuffer(gl::FRAMEBUFFER, 0);
        gl::CullFace(gl::BACK);
        gl::Disable(gl::CULL_FACE);
        gl::Disable(gl::STENCIL_TEST);
        gl::ColorMask(gl::TRUE, gl::TRUE, gl::TRUE, gl::TRUE);
    }
}
