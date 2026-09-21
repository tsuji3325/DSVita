//! Targeted main-RAM observations only. Never changes invalidation decisions.
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

pub(crate) const TARGET: u32 = 0x0225F4F8;
pub(crate) const START: u32 = 0x0225F400;
pub(crate) const LEN: usize = 512;
pub(crate) const OFFSET: usize = (START & 0x3FFFFF) as usize;
const ROW_LIMIT: usize = 256;
fn canonical(addr: u32) -> u32 { 0x02000000 | (addr & 0x3FFFFF) }
pub(crate) fn watched_page(addr: u32) -> bool {
    addr & 0x0F000000 == 0x02000000 && (START..START + LEN as u32).contains(&canonical(addr))
}
#[inline]
pub(crate) fn touches(addr: u32, size: usize) -> bool {
    if size == 0 { return false; }
    let begin = (addr & 0x3FFFFF) as u64;
    let end = begin.saturating_add(size as u64);
    (begin < (OFFSET + LEN) as u64 && end > OFFSET as u64)
        || end > 0x400000 + OFFSET as u64
}
// Called only for main-RAM writes whose requested invalidation range touches
// the watched physical pages. No mutex is held across the actual memory write.
#[inline(never)]
pub(crate) fn capture(bytes: &[u8]) -> Box<[u8; LEN]> {
    Box::new(bytes.try_into().unwrap())
}
#[derive(Default)]
struct Counts { writes: u64, changed: u64, unchanged: u64, code_changed: u64, outside_only: u64, unknown: u64, invalidating: u64, invalidating_code_changed: u64, invalidating_code_unchanged: u64, invalidating_unknown: u64 }
impl Counts {
    fn add(&mut self, changed: bool, code: Option<bool>, invalidating: bool) {
        self.writes += 1;
        self.changed += changed as u64;
        self.unchanged += (!changed) as u64;
        match code { Some(true) => self.code_changed += 1, Some(false) => self.outside_only += changed as u64, None => self.unknown += 1 }
        if invalidating {
            self.invalidating += 1;
            match code { Some(true) => self.invalidating_code_changed += 1, Some(false) => self.invalidating_code_unchanged += 1, None => self.invalidating_unknown += 1 }
        }
    }
    fn text(&self) -> String {
        format!("writes={} window_changed={} window_unchanged={} compiled_range_changed={} outside_only={} range_unknown={} invalidating={} invalidating_range_changed={} invalidating_range_unchanged={} invalidating_range_unknown={}", self.writes,self.changed,self.unchanged,self.code_changed,self.outside_only,self.unknown,self.invalidating,self.invalidating_code_changed,self.invalidating_code_unchanged,self.invalidating_unknown)
    }
}
#[derive(Default)]
struct ByteChanges { count: u64, first_old: u8, first_new: u8 }
#[derive(Default)]
struct State {
    end: Option<u32>,
    compiled_bytes: Option<[u8; LEN]>,
    compiles: u64,
    repeat_same: u64,
    repeat_changed: u64,
    repeat_different_extent: u64,
    unsupported: u64,
    totals: Counts,
    // Destination is the caller's address; size is the requested invalidation
    // span (fixed-address transfers can write fewer distinct bytes).
    rows: BTreeMap<(u32, usize, bool, u32, u32), Counts>,
    dropped_rows: u64,
    bytes: BTreeMap<usize, ByteChanges>,
}
fn state() -> &'static Mutex<State> {
    static S: OnceLock<Mutex<State>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(State::default()))
}
pub(crate) fn reset() { *state().lock().unwrap() = State::default(); }
pub(crate) fn compiled(end: u32, thumb: bool, bytes: &[u8]) {
    let mut s = state().lock().unwrap();
    s.compiles += 1;
    if thumb || end <= TARGET || end > START + LEN as u32 {
        s.unsupported += 1;
        s.end = None;
        s.compiled_bytes = None;
        return;
    }
    let body = (TARGET - START) as usize..(end - START) as usize;
    if let Some(previous) = &s.compiled_bytes {
        if s.end != Some(end) { s.repeat_different_extent += 1; }
        else if previous[body.clone()] == bytes[body] { s.repeat_same += 1; }
        else { s.repeat_changed += 1; }
    }
    s.end = Some(end);
    s.compiled_bytes = Some(bytes.try_into().unwrap());
}
pub(crate) fn written(addr: u32, size: usize, arm9: bool, source_hint: u32, overlay_hint: u32, before: Box<[u8; LEN]>, after: &[u8], invalidating: bool) {
    assert_eq!(after.len(), LEN);
    let mut s = state().lock().unwrap();
    let changed = before.as_slice() != after;
    let code = s.end.map(|end| {
        let range = (TARGET - START) as usize..(end - START) as usize;
        before[range.clone()] != after[range]
    });
    s.totals.add(changed, code, invalidating);
    let key = (addr, size, arm9, source_hint, overlay_hint);
    if s.rows.contains_key(&key) || s.rows.len() < ROW_LIMIT {
        s.rows.entry(key).or_default().add(changed, code, invalidating);
    } else { s.dropped_rows += 1; }
    for i in 0..LEN {
        if before[i] != after[i] {
            let b = s.bytes.entry(i).or_default();
            if b.count == 0 { b.first_old = before[i]; b.first_new = after[i]; }
            b.count += 1;
        }
    }
}
pub(crate) fn append_report(out: &mut String) {
    let s = state().lock().unwrap();
    out.push_str("[target_write_detail]\ntarget_pc=0x0225F4F8 watched_physical_range=0x0225F400-0x0225F5FF\n");
    out.push_str("scope=main_RAM_slow_write_paths_including_ARM7_and_DMA aliases_normalized_for_filter source_hint=last_ARM9_bucket_and_phase_not_exact_writer requested_span=existing_invalidation_argument_not_always_distinct_write_bytes\n");
    out.push_str("comparison=byte_exact_last_compiled_guest_range_may_include_literal_data range_unknown=before_first_supported_compile_or_unsupported_extent direct_fast_writes_not_observed same_at_recompile_does_not_exclude_intermediate_changes\n");
    out.push_str("invalidating=existing_live_bit_on_watched_first_or_last_page_before_original_invalidation no_invalidation_decisions_changed\n");
    out.push_str(&format!("compiles={} repeat_same_bytes={} repeat_changed_bytes={} repeat_different_extent={} unsupported_extent_or_mode={} last_end=0x{:08X} row_limit={} dropped_row_observations={}\n",s.compiles,s.repeat_same,s.repeat_changed,s.repeat_different_extent,s.unsupported,s.end.unwrap_or(0),ROW_LIMIT,s.dropped_rows));
    out.push_str(&format!("total {}\n",s.totals.text()));
    let mut rows: Vec<_> = s.rows.iter().collect();
    rows.sort_by_key(|(k,c)| (std::cmp::Reverse(c.invalidating),std::cmp::Reverse(c.writes),**k));
    for ((addr,size,arm9,source,overlay),c) in rows {
        out.push_str(&format!("dest=0x{:08X} physical=0x{:08X} requested_span={} cpu={} source_bucket=0x{:08X} phase={} overlay_hint={} {}\n",addr,canonical(*addr),size,if *arm9 {"ARM9"} else {"ARM7"},source & !63,source & 63,overlay,c.text()));
    }
    for (offset,b) in &s.bytes {
        out.push_str(&format!("changed_addr=0x{:08X} writes={} first_old=0x{:02X} first_new=0x{:02X}\n", START + *offset as u32,b.count,b.first_old,b.first_new));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distinguishes_neighbor_data_code_changes_and_same_value_invalidations() {
        reset();
        let mut bytes = [0u8;LEN];
        compiled(TARGET+76,false,&bytes);
        let old=capture(&bytes); bytes[0]=1;
        written(START,4,true,0,3,old,&bytes,true);
        compiled(TARGET+76,false,&bytes);
        let old=capture(&bytes); bytes[(TARGET-START) as usize]=7;
        written(TARGET,4,true,0,3,old,&bytes,true);
        compiled(TARGET+76,false,&bytes);
        written(TARGET,4,true,0,3,capture(&bytes),&bytes,true);
        let s=state().lock().unwrap();
        assert_eq!(s.repeat_same,1); assert_eq!(s.repeat_changed,1);
        assert_eq!(s.totals.outside_only,1); assert_eq!(s.totals.unchanged,1);
        assert_eq!(s.totals.invalidating_code_changed,1); assert_eq!(s.totals.invalidating_code_unchanged,2);
    }
    #[test]
    fn aliases_boundaries_unknown_extents_and_row_overflow_are_explicit() {
        assert!(touches(0x0265F4F8,4)); assert!(watched_page(0x0265F4F8));
        assert!(!touches(START-4,4)); assert!(touches(START-4,5));
        assert!(!touches(START+LEN as u32,4)); assert!(!touches(START,0));
        reset(); let bytes=[0;LEN];
        compiled(START+LEN as u32+4,false,&bytes);
        for i in 0..ROW_LIMIT+1 { written(START,4,true,i as u32,0,capture(&bytes),&bytes,false); }
        let s=state().lock().unwrap();
        assert_eq!(s.rows.len(),ROW_LIMIT); assert_eq!(s.dropped_rows,1);
        assert_eq!(s.unsupported,1); assert_eq!(s.totals.unknown,(ROW_LIMIT+1) as u64);
    }
}
