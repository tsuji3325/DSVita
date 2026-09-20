use crate::bitset::Bitset;
use crate::core::graphics::gpu::DispCapCnt;
use crate::core::memory::vram::{Vram, VramBanks, VramCnt};
use crate::core::memory::{regions, vram};
use crate::utils::{self, HeapArrayU8, PtrWrapper};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

pub static VRAM_DIRTY_COPY_US: AtomicU32 = AtomicU32::new(0);
pub static VRAM_DIRTY_COPY_MAX_US: AtomicU32 = AtomicU32::new(0);
pub static VRAM_MAP_REBUILD_US: AtomicU32 = AtomicU32::new(0);
pub static VRAM_MAP_REBUILD_MAX_US: AtomicU32 = AtomicU32::new(0);
pub static VRAM_CAPTURE_INSERT_US: AtomicU32 = AtomicU32::new(0);
pub static VRAM_CAPTURE_INSERT_MAX_US: AtomicU32 = AtomicU32::new(0);
pub static VRAM_READ_ALL_US: AtomicU32 = AtomicU32::new(0);
pub static VRAM_READ_ALL_MAX_US: AtomicU32 = AtomicU32::new(0);
pub static VRAM_READ_LCDC_US: AtomicU32 = AtomicU32::new(0);
pub static VRAM_READ_2D_A_US: AtomicU32 = AtomicU32::new(0);
pub static VRAM_READ_2D_B_US: AtomicU32 = AtomicU32::new(0);
pub static VRAM_READ_3D_US: AtomicU32 = AtomicU32::new(0);
pub static VRAM_FULL_READ_COUNT: AtomicU32 = AtomicU32::new(0);
pub static VRAM_PARTIAL_READ_COUNT: AtomicU32 = AtomicU32::new(0);
pub static VRAM_PARTIAL_COPIED_BYTES: AtomicU32 = AtomicU32::new(0);
pub static VRAM_MAPPING_CHANGE_COUNT: AtomicU32 = AtomicU32::new(0);

#[inline]
fn perf_add_max(total: &AtomicU32, max: &AtomicU32, micros: u32) {
    total.fetch_add(micros, Ordering::Relaxed);
    let mut old = max.load(Ordering::Relaxed);
    while micros > old {
        match max.compare_exchange_weak(old, micros, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(current) => old = current,
        }
    }
}

#[derive(Default)]
pub struct GpuMemRefs {
    pub lcdc: PtrWrapper<[u8; vram::TOTAL_SIZE]>,

    pub bg_a: PtrWrapper<[u8; vram::BG_A_SIZE]>,
    pub obj_a: PtrWrapper<[u8; vram::OBJ_A_SIZE]>,
    pub bg_a_ext_pal: PtrWrapper<[u8; vram::BG_EXT_PAL_SIZE]>,
    pub obj_a_ext_pal: PtrWrapper<[u8; vram::OBJ_EXT_PAL_SIZE]>,
    pub pal_a: PtrWrapper<[u8; regions::STANDARD_PALETTES_SIZE as usize / 2]>,
    pub oam_a: PtrWrapper<[u8; regions::OAM_SIZE as usize / 2]>,

    pub bg_b: PtrWrapper<[u8; vram::BG_B_SIZE]>,
    pub obj_b: PtrWrapper<[u8; vram::OBJ_B_SIZE]>,
    pub bg_b_ext_pal: PtrWrapper<[u8; vram::BG_EXT_PAL_SIZE]>,
    pub obj_b_ext_pal: PtrWrapper<[u8; vram::OBJ_EXT_PAL_SIZE]>,
    pub pal_b: PtrWrapper<[u8; regions::STANDARD_PALETTES_SIZE as usize / 2]>,
    pub oam_b: PtrWrapper<[u8; regions::OAM_SIZE as usize / 2]>,

    pub tex_rear_plane_image: PtrWrapper<[u8; vram::TEX_REAR_PLANE_IMAGE_SIZE]>,
    pub tex_pal: PtrWrapper<[u8; vram::TEX_PAL_SIZE]>,
}

#[derive(Default)]
pub struct GpuMemBuf {
    pub vram: Vram,
    queued_vram_cnt: [u8; vram::BANK_SIZE],
    pub vram_banks: VramBanks,
    vram_banks_dirty_sections: Bitset<6>,
    last_read_vram_cnt: [u8; vram::BANK_SIZE],
    vram_read_initialized: bool,
    lcdc_read_valid: bool,
    tex_read_valid: bool,
    pub pal: HeapArrayU8<{ regions::STANDARD_PALETTES_SIZE as usize }>,
    pub oam: HeapArrayU8<{ regions::OAM_SIZE as usize }>,
}

impl GpuMemBuf {
    pub fn init(&mut self) {
        self.vram = Vram::default();
        self.vram_banks.dirty_sections.clear();
        self.last_read_vram_cnt = [0; vram::BANK_SIZE];
        self.vram_read_initialized = false;
        self.lcdc_read_valid = false;
        self.tex_read_valid = false;
    }

    pub fn queue_vram(&mut self, vram: &Vram) {
        self.queued_vram_cnt = vram.cnt;
    }

    pub fn read_vram(&mut self, vram_banks: &mut VramBanks) {
        let start = Instant::now();
        vram_banks.copy_dirty_sections(&mut self.vram_banks.mem);
        self.vram_banks_dirty_sections += vram_banks.dirty_sections;
        vram_banks.dirty_sections.clear();
        let micros = start.elapsed().as_micros().min(u32::MAX as u128) as u32;
        perf_add_max(&VRAM_DIRTY_COPY_US, &VRAM_DIRTY_COPY_MAX_US, micros);
    }

    pub fn read_palettes_oam(&mut self, palettes: &[u8; regions::STANDARD_PALETTES_SIZE as usize], oam: &[u8; regions::OAM_SIZE as usize]) {
        self.pal.copy_from_slice(palettes);
        self.oam.copy_from_slice(oam);
    }

    pub fn use_queued_vram(&mut self) {
        self.vram.cnt = self.queued_vram_cnt;
        self.vram_banks.dirty_sections += self.vram_banks_dirty_sections;
        self.vram_banks_dirty_sections.clear();
    }

    pub fn rebuild_vram_maps(&mut self) {
        let start = Instant::now();
        self.vram.rebuild_maps();
        let micros = start.elapsed().as_micros().min(u32::MAX as u128) as u32;
        perf_add_max(&VRAM_MAP_REBUILD_US, &VRAM_MAP_REBUILD_MAX_US, micros);
    }

    pub fn read_all(&mut self, refs: &mut GpuMemRefs, read_lcdc: bool, read_3d: bool) {
        let read_all_start = Instant::now();
        let mapping_changed = !self.vram_read_initialized || self.last_read_vram_cnt != self.vram.cnt;
        if mapping_changed {
            VRAM_MAPPING_CHANGE_COUNT.fetch_add(1, Ordering::Relaxed);
        }

        let dirty_sections = self.vram_banks.dirty_sections;
        let mut partial_copied_bytes = 0usize;

        if read_lcdc {
            let start = Instant::now();
            // Safety test: LCDC follows the original full-refresh path.
            self.vram.maps.read_all_lcdc(&mut refs.lcdc, &self.vram_banks.mem);
            self.lcdc_read_valid = true;
            VRAM_READ_LCDC_US.fetch_add(start.elapsed().as_micros().min(u32::MAX as u128) as u32, Ordering::Relaxed);
        } else {
            self.lcdc_read_valid = false;
        }

        let start = Instant::now();
        // Safety test: Engine A also stays on the original full-refresh path.
        self.vram.maps.read_all_bg_a(&mut refs.bg_a, &self.vram_banks.mem);
        self.vram.maps.read_all_obj_a(&mut refs.obj_a, &self.vram_banks.mem);
        self.vram.maps.read_all_bg_a_ext_palette(&mut refs.bg_a_ext_pal, &self.vram_banks.mem);
        self.vram.maps.read_all_obj_a_ext_palette(&mut refs.obj_a_ext_pal, &self.vram_banks.mem);
        refs.pal_a.copy_from_slice(&self.pal[..regions::STANDARD_PALETTES_SIZE as usize / 2]);
        refs.oam_a.copy_from_slice(&self.oam[..regions::OAM_SIZE as usize / 2]);
        VRAM_READ_2D_A_US.fetch_add(start.elapsed().as_micros().min(u32::MAX as u128) as u32, Ordering::Relaxed);

        let start = Instant::now();
        // Safety diagnostic: keep Engine B on the original full-refresh path.
        // HeartGold's lower/menu screen showed corruption when B used dirty-only refreshes.
        self.vram.maps.read_bg_b(&mut refs.bg_b, &self.vram_banks.mem);
        self.vram.maps.read_all_obj_b(&mut refs.obj_b, &self.vram_banks.mem);
        self.vram.maps.read_all_bg_b_ext_palette(&mut refs.bg_b_ext_pal, &self.vram_banks.mem);
        self.vram.maps.read_all_obj_b_ext_palette(&mut refs.obj_b_ext_pal, &self.vram_banks.mem);
        refs.pal_b.copy_from_slice(&self.pal[regions::STANDARD_PALETTES_SIZE as usize / 2..]);
        refs.oam_b.copy_from_slice(&self.oam[regions::OAM_SIZE as usize / 2..]);
        VRAM_READ_2D_B_US.fetch_add(start.elapsed().as_micros().min(u32::MAX as u128) as u32, Ordering::Relaxed);

        if read_3d {
            let start = Instant::now();
            if mapping_changed || !self.tex_read_valid {
                self.vram.maps.read_all_tex_rear_plane_img(&mut refs.tex_rear_plane_image, &self.vram_banks.mem);
                self.vram.maps.read_all_tex_palette(&mut refs.tex_pal, &self.vram_banks.mem);
            } else {
                partial_copied_bytes += self.vram.maps.read_dirty_tex_rear_plane_img(&mut refs.tex_rear_plane_image, &self.vram_banks.mem, &dirty_sections);
                partial_copied_bytes += self.vram.maps.read_dirty_tex_palette(&mut refs.tex_pal, &self.vram_banks.mem, &dirty_sections);
            }
            self.tex_read_valid = true;
            VRAM_READ_3D_US.fetch_add(start.elapsed().as_micros().min(u32::MAX as u128) as u32, Ordering::Relaxed);
        } else {
            self.tex_read_valid = false;
        }

        if mapping_changed {
            VRAM_FULL_READ_COUNT.fetch_add(1, Ordering::Relaxed);
        } else {
            VRAM_PARTIAL_READ_COUNT.fetch_add(1, Ordering::Relaxed);
            VRAM_PARTIAL_COPIED_BYTES.fetch_add(partial_copied_bytes.min(u32::MAX as usize) as u32, Ordering::Relaxed);
        }

        self.last_read_vram_cnt = self.vram.cnt;
        self.vram_read_initialized = true;

        let micros = read_all_start.elapsed().as_micros().min(u32::MAX as u128) as u32;
        perf_add_max(&VRAM_READ_ALL_US, &VRAM_READ_ALL_MAX_US, micros);
    }

    pub fn insert_capture_mem(&mut self, capture_mem: &[u8; vram::BANK_A_SIZE * 4]) {
        let start = Instant::now();
        for bank_num in 0..4 {
            if !VramCnt::from(self.vram.cnt[bank_num]).enable() {
                continue;
            }

            for offset_num in 0..4 {
                let offset = offset_num * 0x8000;
                let bank = &mut self.vram_banks.mem[bank_num * vram::BANK_A_SIZE + offset..];
                let disp_cap_cnt = utils::read_from_mem::<DispCapCnt>(bank, vram::CAPTURE_IDENTIFIER.len() as u32);
                if &bank[..vram::CAPTURE_IDENTIFIER.len()] == vram::CAPTURE_IDENTIFIER
                    && disp_cap_cnt.capture_enabled()
                    && u8::from(disp_cap_cnt.vram_write_block()) == bank_num as u8
                    && u8::from(disp_cap_cnt.vram_write_offset()) == offset_num as u8
                {
                    let bytes_len = disp_cap_cnt.pixel_size() * 2;
                    let capture_offset = bank_num * vram::BANK_A_SIZE + offset;
                    let capture_mem = &capture_mem[capture_offset..capture_offset + bytes_len];
                    bank[..bytes_len].copy_from_slice(capture_mem);

                    let start_section = capture_offset >> 12;
                    let end_section = (capture_offset + bytes_len - 1) >> 12;
                    for section in start_section..=end_section {
                        self.vram_banks.dirty_sections += section;
                    }
                }
            }
        }
        let micros = start.elapsed().as_micros().min(u32::MAX as u128) as u32;
        perf_add_max(&VRAM_CAPTURE_INSERT_US, &VRAM_CAPTURE_INSERT_MAX_US, micros);
    }
}
