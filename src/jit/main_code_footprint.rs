//! Conservative session-wide main-RAM dependency bitmap (one bit per byte).
//! Never unmark on invalidation/eviction: stale dependencies may miss an
//! optimization, but cannot make an older resident block appear independent.
const MAIN_SIZE: usize = 0x400000;
const PAGE_SIZE: usize = 256;

pub struct MainCodeFootprint {
    words: Box<[u64]>,
}
impl MainCodeFootprint {
    pub fn new() -> Self { Self { words: vec![0; MAIN_SIZE / 64].into_boxed_slice() } }
    pub fn clear(&mut self) { self.words.fill(0); }
    fn main(addr: u32) -> bool { addr & 0xFF000000 == 0x02000000 }
    fn mask(first: usize, count: usize) -> u64 {
        (u64::MAX >> (64 - count)) << first
    }
    pub fn mark(&mut self, addr: u32, size: usize) {
        if !Self::main(addr) || size == 0 { return; }
        // Out-of-region or unreasonably large decoded dependencies disable the
        // optimization for this session instead of risking a truncated mask.
        if size >= MAIN_SIZE || (addr as u64 + size as u64) > 0x03000000 {
            self.words.fill(u64::MAX);
            return;
        }
        let mut offset = addr as usize & (MAIN_SIZE - 1);
        let mut remaining = size;
        while remaining != 0 {
            let bit = offset & 63;
            let n = remaining.min(64 - bit);
            self.words[offset / 64] |= Self::mask(bit, n);
            offset = (offset + n) & (MAIN_SIZE - 1);
            remaining -= n;
        }
    }
    /// Only single-page main-RAM writes are eligible. Other regions, wrapping
    /// ranges and bulk transfers keep the existing invalidation path unchanged.
    pub fn can_preserve(&self, addr: u32, size: usize) -> bool {
        if !Self::main(addr) || size == 0 || size > PAGE_SIZE - (addr as usize & (PAGE_SIZE - 1)) { return false; }
        let mut offset = addr as usize & (MAIN_SIZE - 1);
        let mut remaining = size;
        while remaining != 0 {
            let bit = offset & 63;
            let n = remaining.min(64 - bit);
            if self.words[offset / 64] & Self::mask(bit, n) != 0 { return false; }
            offset += n;
            remaining -= n;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn neighbor_data_is_disjoint_but_each_instruction_byte_stays_protected() {
        let mut f = MainCodeFootprint::new();
        f.mark(0x0225F4F8, 76);
        for a in 0x0225F400..0x0225F4F8 { assert!(f.can_preserve(a,1)); }
        for a in 0x0225F4F8..0x0225F544 { assert!(!f.can_preserve(a,1)); }
        assert!(!f.can_preserve(0x0225F4F7,2));
        assert!(f.can_preserve(0x0225F544,4));
        assert!(!f.can_preserve(0x0265F4F8,1)); // main-RAM mirror
        f.mark(0x0225F404,4); // folded literal or another block
        assert!(!f.can_preserve(0x0225F404,1));
        assert!(f.can_preserve(0x0225F408,1));
    }
    #[test]
    fn conservative_boundaries_reset_and_mirror_wrap() {
        let mut f=MainCodeFootprint::new();
        assert!(!f.can_preserve(0x020001FF,2));
        assert!(!f.can_preserve(0x02000000,0));
        assert!(!f.can_preserve(0x03000000,4));
        f.mark(0x023FFFFE,4);
        for a in [0x023FFFFE,0x023FFFFF,0x02000000,0x02000001] {assert!(!f.can_preserve(a,1));}
        assert!(f.can_preserve(0x02000002,1));
        f.clear(); assert!(f.can_preserve(0x02000000,1));
        f.mark(0x02000000,MAIN_SIZE); assert!(!f.can_preserve(0x02200000,4));
    }
    #[test]
    fn bit_masks_match_byte_reference_for_arm_thumb_and_unaligned_writes() {
        let mut f=MainCodeFootprint::new(); let mut reference=[false;512];
        for (start,len) in [(0,4),(31,2),(63,4),(127,2),(248,76),(400,8)] {
            f.mark(0x02000000+start as u32,len);
            reference[start..start+len].fill(true);
        }
        for start in 0..512 { for len in 1..=8 {
            if start+len > 512 {continue;}
            let expected=start/256==(start+len-1)/256 && !reference[start..start+len].iter().any(|b|*b);
            assert_eq!(f.can_preserve(0x02000000+start as u32,len),expected,"{start} {len}");
        }}
    }
}
