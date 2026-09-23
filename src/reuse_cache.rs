//! Four-entry, same-allocation ARM9 cache. No locks or guest calls while borrowed.
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
pub(crate) const TARGETS: [u32; 4] = [0x0225E8CC, 0x0225E9A4, 0x0225E9B4, 0x0225E9D8];
static HITS: [AtomicU32; 4] = [const { AtomicU32::new(0) }; 4];
static STORES: AtomicU32 = AtomicU32::new(0);
static MISSES: AtomicU32 = AtomicU32::new(0);
static CHANGED: AtomicU32 = AtomicU32::new(0);
static PATCHED: AtomicU32 = AtomicU32::new(0);
static EXCLUDED: AtomicU32 = AtomicU32::new(0);
static CLEARS: AtomicU32 = AtomicU32::new(0);

#[derive(PartialEq, Eq)]
pub(crate) struct Key {
    pub pc: u32,
    pub end: u32,
    pub context: [u32; 2], // ARM7 mode and discovered overlay hook address
    pub instructions: Vec<(u32, u16)>, // freshly decoded opcode and cumulative cycles
}
struct Entry { key: Key, offset: usize, native: Vec<u8> }
#[derive(Default)]
pub(crate) struct Cache { entries: [Option<Entry>; 4] }
impl Cache {
    pub fn clear(&mut self) {
        self.entries = Default::default();
        CLEARS.fetch_add(1, Relaxed);
    }
    /// Observer only: used by the fault handler before touching patch metadata.
    pub fn native_owner(&self, offset: usize) -> Option<u32> {
        self.entries.iter().flatten().find(|e| offset >= e.offset && offset - e.offset < e.native.len()).map(|e| e.key.pc)
    }
    pub fn lookup(&self, key: &Key, memory: &[u8]) -> Option<usize> {
        let i = TARGETS.iter().position(|p| *p == key.pc)?;
        let Some(e) = &self.entries[i] else { MISSES.fetch_add(1, Relaxed); return None; };
        if e.key != *key { CHANGED.fetch_add(1, Relaxed); return None; }
        // Fault handlers can specialize native code. Never resurrect such a block.
        if memory.get(e.offset..e.offset + e.native.len()) != Some(e.native.as_slice()) {
            PATCHED.fetch_add(1, Relaxed); return None;
        }
        HITS[i].fetch_add(1, Relaxed);
        Some(e.offset)
    }
    pub fn remember(&mut self, key: Key, offset: usize, size: usize, memory: &[u8]) {
        let Some(i) = TARGETS.iter().position(|p| *p == key.pc) else { return; };
        if size == 0 || size > 16384 { return; }
        let Some(native) = memory.get(offset..offset + size) else { return; };
        self.entries[i] = Some(Entry { key, offset, native: native.to_vec() });
        STORES.fetch_add(1, Relaxed);
    }
}
pub(crate) fn excluded() { EXCLUDED.fetch_add(1, Relaxed); }
pub(crate) fn reset() {
    for v in HITS.iter().chain([&STORES, &MISSES, &CHANGED, &PATCHED, &EXCLUDED, &CLEARS]) { v.store(0, Relaxed); }
}
pub(crate) fn append_report(out: &mut String) {
    out.push_str("[jit_reuse]\npolicy=ARM9_ARM_four_targets_no_immediate_memory_operands fresh_decode_and_HLE_checks same_native_allocation unpatched_only clear_on_any_eviction write_snapshots_disabled\n");
    out.push_str(&format!("stores={} misses={} key_mismatch={} native_modified={} excluded={} cache_clears={}\n", STORES.load(Relaxed), MISSES.load(Relaxed), CHANGED.load(Relaxed), PATCHED.load(Relaxed), EXCLUDED.load(Relaxed), CLEARS.load(Relaxed)));
    for (i, pc) in TARGETS.iter().enumerate() { out.push_str(&format!("pc=0x{pc:08X} reuse_hits={}\n", HITS[i].load(Relaxed))); }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key() -> Key { Key { pc: TARGETS[0], end: TARGETS[0]+4, context: [0, 123], instructions: vec![(0xE1A00000, 1)] } }
    #[test]
    fn exact_identity_and_restoration() {
        let mut c=Cache::default(); let m=vec![7;128];
        c.remember(key(),16,32,&m);
        assert_eq!(c.lookup(&key(),&m),Some(16));
        assert_eq!(c.native_owner(16),Some(TARGETS[0]));
        assert_eq!(c.native_owner(47),Some(TARGETS[0]));
        assert_eq!(c.native_owner(15),None);assert_eq!(c.native_owner(48),None);
        let mut changed=key(); changed.instructions[0].0 ^= 1;
        assert_eq!(c.lookup(&changed,&m),None);
        assert_eq!(c.lookup(&key(),&m),Some(16));
        assert_eq!(c.native_owner(16),Some(TARGETS[0]));
        assert_eq!(c.native_owner(47),Some(TARGETS[0]));
        assert_eq!(c.native_owner(15),None);assert_eq!(c.native_owner(48),None);
        let mut changed=key(); changed.instructions[0].1=2;
        assert_eq!(c.lookup(&changed,&m),None);
        let mut changed=key(); changed.end+=4;
        assert_eq!(c.lookup(&changed,&m),None);
        let mut changed=key(); changed.context[0]=1;
        assert_eq!(c.lookup(&changed,&m),None);
        let mut changed=key(); changed.context[1]=456;
        assert_eq!(c.lookup(&changed,&m),None);
    }
    #[test]
    fn patch_eviction_reset_and_bounds_reject_stale_native_code() {
        let mut c=Cache::default(); let mut m=vec![7;128];
        c.remember(key(),16,32,&m); m[20]=8;
        assert_eq!(c.lookup(&key(),&m),None);
        m[20]=7; c.clear();
        assert_eq!(c.native_owner(16),None);
        assert_eq!(c.lookup(&key(),&m),None);
        c.remember(key(),120,32,&m);
        assert_eq!(c.lookup(&key(),&m),None);
        c.remember(key(),16,32,&m);
        assert_eq!(c.lookup(&key(),&m[..24]),None);
        let mut other=key(); other.pc+=1;
        assert_eq!(c.lookup(&other,&m),None);
    }
}
