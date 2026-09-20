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
    last_read_vram_cnt: [u8; vram::BANK_SIZE],
    vram_read_initialized: bool,
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
        self.tex_read_valid = false;
    }

    pub fn queue_vram(&mut self, vram: &Vram) {
        self.queued_vram_cnt = vram.cnt;
    }

    pub fn read_vram(&mut self, vram_banks: &mut VramBanks) {
        vram_banks.copy_dirty_sections(&mut self.vram_banks.mem);
        self.vram_banks_dirty_sections += vram_banks.dirty_sections;
        vram_banks.dirty_sections.clear();
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
        self.vram.rebuild_maps();
    }

    pub fn read_all(&mut self, refs: &mut GpuMemRefs, read_lcdc: bool, read_3d: bool) {
        let mapping_changed = !self.vram_read_initialized || self.last_read_vram_cnt != self.vram.cnt;
        let dirty_sections = self.vram_banks.dirty_sections;

        if read_lcdc {
            self.vram.maps.read_all_lcdc(&mut refs.lcdc, &self.vram_banks.mem);
        }

        // Keep both 2D engines on the original full-refresh path. Partial updates
        // caused visible corruption in HeartGold.
        self.vram.maps.read_all_bg_a(&mut refs.bg_a, &self.vram_banks.mem);
        self.vram.maps.read_all_obj_a(&mut refs.obj_a, &self.vram_banks.mem);
        self.vram.maps.read_all_bg_a_ext_palette(&mut refs.bg_a_ext_pal, &self.vram_banks.mem);
        self.vram.maps.read_all_obj_a_ext_palette(&mut refs.obj_a_ext_pal, &self.vram_banks.mem);
        refs.pal_a.copy_from_slice(&self.pal[..regions::STANDARD_PALETTES_SIZE as usize / 2]);
        refs.oam_a.copy_from_slice(&self.oam[..regions::OAM_SIZE as usize / 2]);

        self.vram.maps.read_bg_b(&mut refs.bg_b, &self.vram_banks.mem);
        self.vram.maps.read_all_obj_b(&mut refs.obj_b, &self.vram_banks.mem);
        self.vram.maps.read_all_bg_b_ext_palette(&mut refs.bg_b_ext_pal, &self.vram_banks.mem);
        self.vram.maps.read_all_obj_b_ext_palette(&mut refs.obj_b_ext_pal, &self.vram_banks.mem);
        refs.pal_b.copy_from_slice(&self.pal[regions::STANDARD_PALETTES_SIZE as usize / 2..]);
        refs.oam_b.copy_from_slice(&self.oam[regions::OAM_SIZE as usize / 2..]);

        // 3D texture/palette VRAM is safe to update by dirty section while the
        // VRAM mapping is unchanged. Mapping changes still force a full refresh.
        if read_3d {
            if mapping_changed || !self.tex_read_valid {
                self.vram.maps.read_all_tex_rear_plane_img(&mut refs.tex_rear_plane_image, &self.vram_banks.mem);
                self.vram.maps.read_all_tex_palette(&mut refs.tex_pal, &self.vram_banks.mem);
            } else {
                self.vram
                    .maps
                    .read_dirty_tex_rear_plane_img(&mut refs.tex_rear_plane_image, &self.vram_banks.mem, &dirty_sections);
                self.vram.maps.read_dirty_tex_palette(&mut refs.tex_pal, &self.vram_banks.mem, &dirty_sections);
            }
            self.tex_read_valid = true;
        } else {
            self.tex_read_valid = false;
        }

        self.last_read_vram_cnt = self.vram.cnt;
        self.vram_read_initialized = true;
    }

    pub fn insert_capture_mem(&mut self, capture_mem: &[u8; vram::BANK_A_SIZE * 4]) {
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
    }
}
