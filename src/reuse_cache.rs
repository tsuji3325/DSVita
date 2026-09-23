//! Eight-entry same-allocation ARM9 cache. No locks or guest calls while borrowed.
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};

pub(crate) const ARM_TARGETS: [u32; 4] = [0x0225E8CC, 0x0225E9A4, 0x0225E9B4, 0x0225E9D8];
pub(crate) const THUMB_TARGETS: [u32; 4] = [0x021FAED6, 0x021FA25A, 0x02203714, 0x021F2E34];
const TARGETS: [(u32, bool); 8] = [
    (ARM_TARGETS[0], false),
    (ARM_TARGETS[1], false),
    (ARM_TARGETS[2], false),
    (ARM_TARGETS[3], false),
    (THUMB_TARGETS[0], true),
    (THUMB_TARGETS[1], true),
    (THUMB_TARGETS[2], true),
    (THUMB_TARGETS[3], true),
];

static HITS: [AtomicU32; 8] = [const { AtomicU32::new(0) }; 8];
static STORES: AtomicU32 = AtomicU32::new(0);
static MISSES: AtomicU32 = AtomicU32::new(0);
static CHANGED: AtomicU32 = AtomicU32::new(0);
static PATCHED: AtomicU32 = AtomicU32::new(0);
static ACCEPTED_PATCHES: AtomicU32 = AtomicU32::new(0);
static EXCLUDED: AtomicU32 = AtomicU32::new(0);
static CLEARS: AtomicU32 = AtomicU32::new(0);

#[derive(PartialEq, Eq)]
pub(crate) struct Key {
    pub pc: u32,
    pub end: u32,
    pub thumb: bool,
    pub context: [u32; 2],
    pub instructions: Vec<(u32, u16)>,
    pub dependencies: Vec<(u32, u32, u32)>,
}

struct Entry {
    key: Key,
    offset: usize,
    native: Vec<u8>,
}

#[derive(Default)]
pub(crate) struct Cache {
    entries: [Option<Entry>; 8],
}

fn target_index(pc: u32, thumb: bool) -> Option<usize> {
    TARGETS.iter().position(|&(target_pc, target_thumb)| target_pc == pc && target_thumb == thumb)
}

impl Cache {
    pub fn clear(&mut self) {
        self.entries = Default::default();
        CLEARS.fetch_add(1, Relaxed);
    }

    pub fn native_owner(&self, offset: usize) -> Option<u32> {
        self.entries
            .iter()
            .flatten()
            .find(|e| offset >= e.offset && offset - e.offset < e.native.len())
            .map(|e| e.key.pc)
    }

    pub fn lookup(&self, key: &Key, memory: &[u8]) -> Option<usize> {
        let i = target_index(key.pc, key.thumb)?;
        let Some(e) = &self.entries[i] else {
            MISSES.fetch_add(1, Relaxed);
            return None;
        };
        if e.key != *key {
            CHANGED.fetch_add(1, Relaxed);
            return None;
        }
        if memory.get(e.offset..e.offset + e.native.len()) != Some(e.native.as_slice()) {
            PATCHED.fetch_add(1, Relaxed);
            return None;
        }
        HITS[i].fetch_add(1, Relaxed);
        Some(e.offset)
    }

    pub fn remember(&mut self, key: Key, offset: usize, size: usize, memory: &[u8]) {
        let Some(i) = target_index(key.pc, key.thumb) else { return; };
        if size == 0 || size > 16384 { return; }
        let Some(native) = memory.get(offset..offset + size) else { return; };
        self.entries[i] = Some(Entry { key, offset, native: native.to_vec() });
        STORES.fetch_add(1, Relaxed);
    }

    /// Accept only hardware-confirmed MMIO slow-path windows for FAED6 and E8CC.
    /// The cache remains tied to the same allocation/metadata. Updating only the
    /// exact rewritten window means any unrelated native modification still fails
    /// the full-byte identity check on the next lookup.
    pub fn accept_known_patch(
        &mut self,
        owner: u32,
        guest_pc: u32,
        addr: u32,
        write: bool,
        patch_offset: usize,
        patched: &[u8],
    ) -> bool {
        let thumb = THUMB_TARGETS.contains(&owner);
        let guest_pc_norm = if thumb { guest_pc & !1 } else { guest_pc };
        let known = if thumb {
            addr == 0x04000060
                && ((guest_pc_norm == 0x021FAFFE && !write)
                    || (guest_pc_norm == 0x021FB006 && write))
        } else if owner == ARM_TARGETS[0] {
            !write
                && ((guest_pc_norm == 0x0225E8CC && addr == 0x040001A4)
                    || (guest_pc_norm == 0x0225E8DC && addr == 0x04100010)
                    || (guest_pc_norm == 0x0225E8F0 && addr == 0x040001A4))
        } else {
            false
        };
        if !known {
            return false;
        }
        let Some(i) = target_index(owner, thumb) else { return false; };
        let Some(entry) = &mut self.entries[i] else { return false; };
        if entry.key.pc != owner || entry.key.thumb != thumb || patch_offset < entry.offset {
            return false;
        }
        let rel = patch_offset - entry.offset;
        let Some(dst) = entry.native.get_mut(rel..rel + patched.len()) else { return false; };
        dst.copy_from_slice(patched);
        ACCEPTED_PATCHES.fetch_add(1, Relaxed);
        true
    }
}

pub(crate) fn excluded() { EXCLUDED.fetch_add(1, Relaxed); }

pub(crate) fn reset() {
    for v in HITS.iter().chain([&STORES, &MISSES, &CHANGED, &PATCHED, &ACCEPTED_PATCHES, &EXCLUDED, &CLEARS]) {
        v.store(0, Relaxed);
    }
}

pub(crate) fn append_report(out: &mut String) {
    out.push_str("[jit_reuse]\npolicy=ARM9_four_ARM_targets_without_immediate_memory_operands_plus_four_Thumb_targets_with_main_RAM_folded_literals_revalidated same_native_allocation known_FAED6_and_E8CC_MMIO_patch_windows_accepted_in_place all_other_native_changes_rejected clear_on_any_eviction write_snapshots_disabled\n");
    out.push_str(&format!(
        "stores={} misses={} key_mismatch={} native_modified={} accepted_native_patches={} excluded={} cache_clears={}\n",
        STORES.load(Relaxed), MISSES.load(Relaxed), CHANGED.load(Relaxed),
        PATCHED.load(Relaxed), ACCEPTED_PATCHES.load(Relaxed), EXCLUDED.load(Relaxed), CLEARS.load(Relaxed)
    ));
    for (i, (pc, thumb)) in TARGETS.iter().enumerate() {
        out.push_str(&format!("pc=0x{pc:08X} thumb={} reuse_hits={}\n", *thumb as u8, HITS[i].load(Relaxed)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arm_key() -> Key {
        Key {
            pc: ARM_TARGETS[0], end: ARM_TARGETS[0] + 4, thumb: false,
            context: [0, 123], instructions: vec![(0xE1A00000, 1)], dependencies: Vec::new(),
        }
    }

    fn thumb_key() -> Key {
        Key {
            pc: THUMB_TARGETS[0], end: THUMB_TARGETS[0] + 4, thumb: true,
            context: [0, 123], instructions: vec![(0x4800, 1), (0x4770, 2)],
            dependencies: vec![(THUMB_TARGETS[0], 0x021FB080, 0x000008AC)],
        }
    }

    #[test]
    fn exact_identity_and_restoration() {
        let mut c=Cache::default(); let m=vec![7;128];
        c.remember(arm_key(),16,32,&m);
        assert_eq!(c.lookup(&arm_key(),&m),Some(16));
        assert_eq!(c.native_owner(16),Some(ARM_TARGETS[0]));
        assert_eq!(c.native_owner(47),Some(ARM_TARGETS[0]));
        assert_eq!(c.native_owner(15),None); assert_eq!(c.native_owner(48),None);

        let mut changed=arm_key(); changed.instructions[0].0^=1;
        assert_eq!(c.lookup(&changed,&m),None);
        assert_eq!(c.lookup(&arm_key(),&m),Some(16));
        let mut changed=arm_key(); changed.instructions[0].1=2;
        assert_eq!(c.lookup(&changed,&m),None);
        let mut changed=arm_key(); changed.end+=4;
        assert_eq!(c.lookup(&changed,&m),None);
        let mut changed=arm_key(); changed.context[0]=1;
        assert_eq!(c.lookup(&changed,&m),None);
        let mut changed=arm_key(); changed.context[1]=456;
        assert_eq!(c.lookup(&changed,&m),None);
    }

    #[test]
    fn thumb_literal_values_are_part_of_identity() {
        let mut c=Cache::default(); let m=vec![9;128];
        c.remember(thumb_key(),32,24,&m);
        assert_eq!(c.lookup(&thumb_key(),&m),Some(32));
        let mut changed=thumb_key(); changed.dependencies[0].2^=1;
        assert_eq!(c.lookup(&changed,&m),None);
        let mut wrong_mode=thumb_key(); wrong_mode.thumb=false;
        assert_eq!(c.lookup(&wrong_mode,&m),None);
    }

    #[test]
    fn all_thumb_targets_have_distinct_slots_and_dependency_identity() {
        let mut c=Cache::default(); let m=vec![5;256];
        for (slot, pc) in THUMB_TARGETS.iter().copied().enumerate() {
            let mut key=thumb_key();
            key.pc=pc; key.end=pc+4;
            key.dependencies=if pc==0x021F2E34 { vec![(pc+4,0x021F2EF4,0x02205E54),(pc+0x76,0x021F2EF8,0x00000F33)] } else { Vec::new() };
            c.remember(key,slot*24,16,&m);
        }
        for (slot, pc) in THUMB_TARGETS.iter().copied().enumerate() {
            let mut key=thumb_key();
            key.pc=pc; key.end=pc+4;
            key.dependencies=if pc==0x021F2E34 { vec![(pc+4,0x021F2EF4,0x02205E54),(pc+0x76,0x021F2EF8,0x00000F33)] } else { Vec::new() };
            assert_eq!(c.lookup(&key,&m),Some(slot*24));
        }
        let mut changed=thumb_key();
        changed.pc=0x021F2E34; changed.end=0x021F2E38;
        changed.dependencies=vec![(0x021F2E38,0x021F2EF4,0x02205E55),(0x021F2EAA,0x021F2EF8,0x00000F33)];
        assert_eq!(c.lookup(&changed,&m),None);
    }

    #[test]
    fn known_faed6_patch_window_can_advance_snapshot_but_other_changes_still_fail() {
        let mut c=Cache::default(); let mut m=vec![9;128];
        c.remember(thumb_key(),32,64,&m);
        m[40..44].copy_from_slice(&[1,2,3,4]);
        assert!(c.accept_known_patch(THUMB_TARGETS[0],0x021FAFFE|1,0x04000060,false,40,&m[40..44]));
        assert_eq!(c.lookup(&thumb_key(),&m),Some(32));

        m[52]=8;
        assert_eq!(c.lookup(&thumb_key(),&m),None);
        m[52]=9;
        assert!(!c.accept_known_patch(THUMB_TARGETS[0],0x021FAFFE|1,0x04000064,false,40,&m[40..44]));
        assert_eq!(c.lookup(&thumb_key(),&m),Some(32));
        assert!(!c.accept_known_patch(THUMB_TARGETS[0],0x021FB006|1,0x04000060,false,40,&m[40..44]));
    }

    #[test]
    fn known_e8cc_patch_windows_can_advance_snapshot_but_unknown_changes_still_fail() {
        let mut c=Cache::default(); let mut m=vec![7;160];
        c.remember(arm_key(),32,96,&m);

        m[40..44].copy_from_slice(&[1,2,3,4]);
        assert!(c.accept_known_patch(ARM_TARGETS[0],0x0225E8CC,0x040001A4,false,40,&m[40..44]));
        m[56..60].copy_from_slice(&[5,6,7,8]);
        assert!(c.accept_known_patch(ARM_TARGETS[0],0x0225E8DC,0x04100010,false,56,&m[56..60]));
        m[72..76].copy_from_slice(&[9,10,11,12]);
        assert!(c.accept_known_patch(ARM_TARGETS[0],0x0225E8F0,0x040001A4,false,72,&m[72..76]));
        assert_eq!(c.lookup(&arm_key(),&m),Some(32));

        m[80]=9;
        assert_eq!(c.lookup(&arm_key(),&m),None);
        m[80]=7;
        assert!(!c.accept_known_patch(ARM_TARGETS[0],0x0225E8CC,0x040001A4,true,40,&m[40..44]));
        assert!(!c.accept_known_patch(ARM_TARGETS[0],0x0225E8D0,0x040001A4,false,40,&m[40..44]));
        assert_eq!(c.lookup(&arm_key(),&m),Some(32));
    }

    #[test]
    fn patch_eviction_reset_and_bounds_reject_stale_native_code() {
        let mut c=Cache::default(); let mut m=vec![7;128];
        c.remember(arm_key(),16,32,&m); m[20]=8;
        assert_eq!(c.lookup(&arm_key(),&m),None);
        m[20]=7; c.clear();
        assert_eq!(c.native_owner(16),None);
        assert_eq!(c.lookup(&arm_key(),&m),None);
        c.remember(arm_key(),120,32,&m);
        assert_eq!(c.lookup(&arm_key(),&m),None);
        c.remember(arm_key(),16,32,&m);
        assert_eq!(c.lookup(&arm_key(),&m[..24]),None);
        let mut other=arm_key(); other.pc+=1;
        assert_eq!(c.lookup(&other,&m),None);
    }
}
