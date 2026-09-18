use crate::bitset::Bitset;
use crate::core::graphics::gpu::DispCapCnt;
use crate::core::memory::vram::{Vram, VramBanks, VramCnt};
use crate::core::memory::{regions, vram};
use crate::utils::{self, HeapArrayU8, PtrWrapper};

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

    // Serial for physical VRAM writes or VRAMCNT remaps. Each mapped consumer records
    // the last serial it copied, so static VRAM is not copied into VitaGL backing memory
    // again every frame.
    vram_serial: u64,
    last_2d_vram_serial: u64,
    last_lcdc_vram_serial: u64,
    last_3d_vram_serial: u64,

    pub pal: HeapArrayU8<{ regions::STANDARD_PALETTES_SIZE as usize }>,
    pub oam: HeapArrayU8<{ regions::OAM_SIZE as usize }>,
}

impl GpuMemBuf {
    pub fn init(&mut self) {
        self.vram = Vram::default();
        self.vram_banks.dirty_sections.clear();
        self.vram_serial = 1;
        self.last_2d_vram_serial = 0;
        self.last_lcdc_vram_serial = 0;
        self.last_3d_vram_serial = 0;
    }

    #[inline]
    fn bump_vram_serial(&mut self) {
        self.vram_serial = self.vram_serial.wrapping_add(1);
        if self.vram_serial == 0 {
            self.vram_serial = 1;
            self.last_2d_vram_serial = 0;
            self.last_lcdc_vram_serial = 0;
            self.last_3d_vram_serial = 0;
        }
    }

    pub fn queue_vram(&mut self, vram: &Vram) {
        self.queued_vram_cnt = vram.cnt;
    }

    pub fn read_vram(&mut self, vram_banks: &mut VramBanks) {
        let changed = !vram_banks.dirty_sections.is_empty();
        vram_banks.copy_dirty_sections(&mut self.vram_banks.mem);
        self.vram_banks_dirty_sections += vram_banks.dirty_sections;
        vram_banks.dirty_sections.clear();
        if changed {
            self.bump_vram_serial();
        }
    }

    pub fn read_palettes_oam(&mut self, palettes: &[u8; regions::STANDARD_PALETTES_SIZE as usize], oam: &[u8; regions::OAM_SIZE as usize]) {
        self.pal.copy_from_slice(palettes);
        self.oam.copy_from_slice(oam);
    }

    pub fn use_queued_vram(&mut self) {
        if self.vram.cnt != self.queued_vram_cnt {
            self.vram.cnt = self.queued_vram_cnt;
            self.bump_vram_serial();
        }
        self.vram_banks.dirty_sections += self.vram_banks_dirty_sections;
        self.vram_banks_dirty_sections.clear();
    }

    pub fn rebuild_vram_maps(&mut self) {
        self.vram.rebuild_maps();
    }

    pub fn read_all(&mut self, refs: &mut GpuMemRefs, read_lcdc: bool, read_3d: bool) {
        let serial = self.vram_serial;

        if read_lcdc && self.last_lcdc_vram_serial != serial {
            self.vram.maps.read_all_lcdc(&mut refs.lcdc, &self.vram_banks.mem);
            self.last_lcdc_vram_serial = serial;
        }

        if self.last_2d_vram_serial != serial {
            self.vram.maps.read_all_bg_a(&mut refs.bg_a, &self.vram_banks.mem);
            self.vram.maps.read_all_obj_a(&mut refs.obj_a, &self.vram_banks.mem);
            self.vram.maps.read_all_bg_a_ext_palette(&mut refs.bg_a_ext_pal, &self.vram_banks.mem);
            self.vram.maps.read_all_obj_a_ext_palette(&mut refs.obj_a_ext_pal, &self.vram_banks.mem);

            self.vram.maps.read_bg_b(&mut refs.bg_b, &self.vram_banks.mem);
            self.vram.maps.read_all_obj_b(&mut refs.obj_b, &self.vram_banks.mem);
            self.vram.maps.read_all_bg_b_ext_palette(&mut refs.bg_b_ext_pal, &self.vram_banks.mem);
            self.vram.maps.read_all_obj_b_ext_palette(&mut refs.obj_b_ext_pal, &self.vram_banks.mem);
            self.last_2d_vram_serial = serial;
        }

        refs.pal_a.copy_from_slice(&self.pal[..regions::STANDARD_PALETTES_SIZE as usize / 2]);
        refs.oam_a.copy_from_slice(&self.oam[..regions::OAM_SIZE as usize / 2]);
        refs.pal_b.copy_from_slice(&self.pal[regions::STANDARD_PALETTES_SIZE as usize / 2..]);
        refs.oam_b.copy_from_slice(&self.oam[regions::OAM_SIZE as usize / 2..]);

        if read_3d && self.last_3d_vram_serial != serial {
            self.vram.maps.read_all_tex_rear_plane_img(&mut refs.tex_rear_plane_image, &self.vram_banks.mem);
            self.vram.maps.read_all_tex_palette(&mut refs.tex_pal, &self.vram_banks.mem);
            self.last_3d_vram_serial = serial;
        }
    }

    pub fn insert_capture_mem(&mut self, capture_mem: &[u8; vram::BANK_A_SIZE * 4]) {
        let mut wrote_vram = false;
        for bank_num in 0..4 {
            if !VramCnt::from(self.vram.cnt[bank_num]).enable() {
                continue;
            }

            for offset_num in 0..4 {
                let offset = offset_num * 0x8000;
                let bank_offset = bank_num * vram::BANK_A_SIZE + offset;
                let bank = &mut self.vram_banks.mem[bank_offset..];
                let disp_cap_cnt = utils::read_from_mem::<DispCapCnt>(bank, vram::CAPTURE_IDENTIFIER.len() as u32);
                if &bank[..vram::CAPTURE_IDENTIFIER.len()] == vram::CAPTURE_IDENTIFIER
                    && disp_cap_cnt.capture_enabled()
                    && u8::from(disp_cap_cnt.vram_write_block()) == bank_num as u8
                    && u8::from(disp_cap_cnt.vram_write_offset()) == offset_num as u8
                {
                    let bytes_len = disp_cap_cnt.pixel_size() * 2;
                    let capture_offset = bank_offset;
                    let capture_src = &capture_mem[capture_offset..capture_offset + bytes_len];
                    bank[..bytes_len].copy_from_slice(capture_src);

                    if bytes_len != 0 {
                        const DIRTY_SHIFT: usize = 12;
                        let start_section = capture_offset >> DIRTY_SHIFT;
                        let end_section = (capture_offset + bytes_len - 1) >> DIRTY_SHIFT;
                        for section in start_section..=end_section {
                            self.vram_banks.dirty_sections += section;
                        }
                        wrote_vram = true;
                    }
                }
            }
        }
        if wrote_vram {
            self.bump_vram_serial();
        }
    }
}
