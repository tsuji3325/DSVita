#!/usr/bin/env python3
"""Exercise production reverse-index methods without requiring VitaGL or a Vita.

Only texture decoding/hash comparison is replaced by a small content model.
Cache indexing methods and Bitset operations are extracted from production Rust.
This checks candidate selection, not GPU rendering or texture decoding.
"""
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
source = (root / 'src/core/graphics/gpu_3d/texture_cache.rs').read_text()
cache_impl = source[source.index('impl Texture3DCache {'):]

def method(name):
    start = cache_impl.index('fn ' + name + '(')
    brace = cache_impl.index('{', start)
    depth = 1
    end = brace + 1
    while depth:
        depth += (cache_impl[end] == '{') - (cache_impl[end] == '}')
        end += 1
    return cache_impl[start:end]

bitset = (root / 'src/bitset.rs').read_text()
bitset = bitset.replace('use crate::savestate::Savestate;', '').replace(', Savestate)', ')')
harness = r'''
use std::collections::HashMap;
use std::cell::Cell;
mod utils { pub type BuildNoHasher64 = std::collections::hash_map::RandomState; }
struct Maps { tex_rear_plane_img_banks: [u8; 4], tex_palette_banks: [u8; 6] }
struct Vram { maps: Maps }
struct Banks { dirty_sections: Bitset<6> }
struct GpuMemBuf { vram: Vram, vram_banks: Banks }
struct GpuMemRefs { values: [u8; 192] }
struct Texture3D {
    dirty: bool, source_sections: Bitset<6>, palette: bool, captured: u8, checks: Cell<u32>
}
impl Texture3D {
    fn section(&self, vram: &Vram) -> usize {
        if self.palette { vram.maps.tex_palette_banks[0] as usize }
        else { vram.maps.tex_rear_plane_img_banks[0] as usize }
    }
    fn calculate_source_sections(&self, vram: &Vram) -> Bitset<6> {
        Bitset::new() + self.section(vram)
    }
    fn is_dirty(&self, mem: &GpuMemBuf, refs: &GpuMemRefs) -> bool {
        self.checks.set(self.checks.get() + 1);
        self.captured != refs.values[self.section(&mem.vram)]
    }
}
const DIRTY_SECTION_SLOTS: usize = 192;
struct Texture3DCache {
    cache: HashMap<u64, Box<Texture3D>, utils::BuildNoHasher64>,
    section_keys: Vec<Vec<u64>>,
    last_tex_rear_plane_img_banks: [u8; 4],
    last_tex_palette_banks: [u8; 6],
    total_size: u32,
}
'''
harness = bitset + harness + '\nimpl Texture3DCache {\n' + '\n'.join(
    method(name) for name in ['new', 'register_sections', 'unregister_sections', 'mark_dirty']
) + '\n}\n' + r'''
fn fixture(palette: bool) -> (Texture3DCache, GpuMemBuf, GpuMemRefs) {
    let mut cache = Texture3DCache::new();
    let mem = GpuMemBuf {
        vram: Vram { maps: Maps { tex_rear_plane_img_banks: [1; 4], tex_palette_banks: [1; 6] } },
        vram_banks: Banks { dirty_sections: Bitset::new() },
    };
    let refs = GpuMemRefs { values: [7; 192] };
    let tex = Texture3D { dirty: false, source_sections: Bitset::new() + 1usize,
        palette, captured: 7, checks: Cell::new(0) };
    cache.register_sections(42, tex.source_sections);
    cache.cache.insert(42, Box::new(tex));
    cache.mark_dirty(&mem, &refs);
    (cache, mem, refs)
}
fn remap_then_write(palette: bool) {
    let (mut cache, mut mem, mut refs) = fixture(palette);
    if palette { mem.vram.maps.tex_palette_banks[0] = 2; }
    else { mem.vram.maps.tex_rear_plane_img_banks[0] = 2; }
    cache.mark_dirty(&mem, &refs); // same bytes in a different physical bank
    assert!(!cache.cache[&42].dirty);
    assert!(cache.section_keys[1].is_empty());
    assert_eq!(cache.section_keys[2], vec![42]);
    refs.values[2] = 9;
    mem.vram_banks.dirty_sections += 2usize;
    cache.mark_dirty(&mem, &refs);
    assert!(cache.cache[&42].dirty, "write in remapped bank must invalidate");
    assert!(cache.section_keys[2].is_empty());
}
#[test] fn image_remap_with_equal_bytes_then_write() { remap_then_write(false); }
#[test] fn palette_remap_with_equal_bytes_then_write() { remap_then_write(true); }
#[test] fn changed_bytes_on_remap_invalidate_immediately() {
    let (mut cache, mut mem, mut refs) = fixture(false);
    mem.vram.maps.tex_rear_plane_img_banks[0] = 2;
    refs.values[2] = 9;
    cache.mark_dirty(&mem, &refs);
    assert!(cache.cache[&42].dirty);
    assert!(cache.section_keys.iter().all(Vec::is_empty));
}
#[test] fn unrelated_dirty_section_does_not_scan_texture() {
    let (mut cache, mut mem, refs) = fixture(false);
    let checks = cache.cache[&42].checks.get();
    mem.vram_banks.dirty_sections += 99usize;
    cache.mark_dirty(&mem, &refs);
    assert_eq!(cache.cache[&42].checks.get(), checks);
    assert!(!cache.cache[&42].dirty);
}
#[test] fn matching_section_without_content_change_stays_indexed() {
    let (mut cache, mut mem, refs) = fixture(false);
    mem.vram_banks.dirty_sections += 1usize;
    cache.mark_dirty(&mem, &refs);
    assert!(!cache.cache[&42].dirty);
    assert_eq!(cache.section_keys[1], vec![42]);
}
'''
with tempfile.TemporaryDirectory() as tmp:
    src = Path(tmp) / 'dirty_index.rs'
    exe = Path(tmp) / 'dirty_index_tests'
    src.write_text(harness)
    subprocess.run(['rustc', '+stable', '--edition=2021', '--test', str(src), '-o', str(exe)], check=True)
    subprocess.run([str(exe)], check=True)
