//! Bounded, in-memory ARM9 compile diagnostics. No locks survive guest execution.
use std::collections::{BTreeMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

const MAX_PCS: usize = 8192;
const MAX_EVENTS: usize = 4096;
pub(crate) const WRITE: usize = 0;
pub(crate) const OVERLAY: usize = 1;
pub(crate) const EVICTION: usize = 2;
pub(crate) const VRAM: usize = 3;
const REASONS: [&str; 4] = ["write", "overlay", "eviction", "vram"];

#[derive(Default)]
struct Pc {
    decode_calls: u64,
    decode_us: u64,
    count: u64,
    us: [u64; 4],
    max_us: u64,
    guest_bytes: u64,
    host_bytes: u64,
    end: u32,
    last_seq: u64,
    overlap: [u64; 4],
    history_gap: u64,
    overlay_hint: u32,
    frame_count: u64,
    frame_us: u64,
    slow_count: u64,
    slow_us: u64,
}
struct Event { seq: u64, start: u32, end: u64, reason: usize }
#[derive(Default)]
struct State {
    pcs: BTreeMap<u32, Pc>,
    events: VecDeque<Event>,
    seq: u64,
    invalidations: [u64; 4],
    eviction_batches: u64,
    dropped_records: u64,
    frame_count: u64,
    frame_us: u64,
    slow_count: u64,
    slow_us: u64,
    all_count: u64,
    all_us: u64,
    pending: Vec<u32>,
}
fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(State::default()))
}
pub(crate) fn reset() { *state().lock().unwrap() = State::default(); }
pub(crate) fn clock(arm9: bool) -> Option<Instant> { arm9.then(Instant::now) }
pub(crate) fn elapsed(start: Option<Instant>) -> u64 {
    start.map_or(0, |t| t.elapsed().as_micros().min(u64::MAX as u128) as u64)
}
pub(crate) fn decoded(pc: u32, us: u64) {
    let mut s = state().lock().unwrap();
    if !s.pcs.contains_key(&pc) && s.pcs.len() >= MAX_PCS { s.dropped_records += 1; return; }
    let p = s.pcs.entry(pc).or_default();
    p.decode_calls += 1;
    p.decode_us += us;
}
pub(crate) fn invalidated(start: u32, size: usize, reason: usize) {
    let mut s = state().lock().unwrap();
    s.seq += 1;
    s.invalidations[reason] += 1;
    let seq = s.seq;
    if s.events.len() == MAX_EVENTS { s.events.pop_front(); }
    s.events.push_back(Event { seq, start: start & !1, end: (start & !1) as u64 + size as u64, reason });
}
pub(crate) fn eviction_batch() { state().lock().unwrap().eviction_batches += 1; }
pub(crate) fn compiled(pc: u32, end: u32, host_bytes: usize, us: [u64; 4], overlay_hint: u32) {
    let mut s = state().lock().unwrap();
    let total: u64 = us.iter().sum();
    s.frame_count += 1;
    s.frame_us += total;
    s.all_count += 1;
    s.all_us += total;
    let mut overlap = [false; 4];
    let mut gap = false;
    if let Some(p) = s.pcs.get(&pc) {
        if p.count != 0 {
            gap = s.events.front().is_some_and(|e| e.seq > p.last_seq + 1);
            for e in s.events.iter().rev() {
                if e.seq <= p.last_seq { break; }
                if ((pc & !1) as u64) < e.end && (e.start as u64) < p.end as u64 {
                    overlap[e.reason] = true;
                }
            }
        }
    }
    let seq = s.seq;
    if s.pcs.get(&pc).is_some_and(|p| p.frame_count == 0) { s.pending.push(pc); }
    let Some(p) = s.pcs.get_mut(&pc) else { return; };
    p.frame_count += 1;
    p.frame_us += total;
    p.count += 1;
    for i in 0..4 { p.us[i] += us[i]; p.overlap[i] += overlap[i] as u64; }
    p.max_us = p.max_us.max(total);
    p.guest_bytes += (end - (pc & !1)) as u64;
    p.host_bytes += host_bytes as u64;
    p.end = end;
    p.last_seq = seq;
    p.history_gap += gap as u64;
    p.overlay_hint = overlay_hint;
}
pub(crate) fn frame(micros: u32) {
    let mut s = state().lock().unwrap();
    if micros >= 40_000 { s.slow_count += s.frame_count; s.slow_us += s.frame_us; }
    s.frame_count = 0;
    s.frame_us = 0;
    // Only visit PCs compiled in this interval; ordinary frames do no map scan.
    let mut pending = std::mem::take(&mut s.pending);
    for pc in pending.drain(..) {
        let p = s.pcs.get_mut(&pc).unwrap();
        if micros >= 40_000 { p.slow_count += p.frame_count; p.slow_us += p.frame_us; }
        p.frame_count = 0;
        p.frame_us = 0;
    }
    s.pending = pending;
}
pub(crate) fn append_report(out: &mut String) {
    let s = state().lock().unwrap();
    out.push_str("[arm9_compile_detail]\ntiming_stages=decode,analyze,emit_finalize,insert units=us scope=all_including_boot_and_partial_frame\n");
    out.push_str("timing_note=excludes_HLE_matching_and_execution_and_diagnostic_bookkeeping decode_calls_include_HLE_candidates\n");
    out.push_str("repeat_note=same_PC_and_mode_not_same_code overlap_counts=repeat_compiles_with_prior_range_overlap_since_last_compile not_proof_of_cause aliases_not_normalized history_gap=older_events_lost\n");
    out.push_str(&format!("pc_limit={} event_limit={} tracked_pcs={} dropped_decode_records={} eviction_batches={} slow_completed_compiles={} slow_compile_us={}\n", MAX_PCS, MAX_EVENTS, s.pcs.len(), s.dropped_records, s.eviction_batches, s.slow_count, s.slow_us));
    out.push_str(&format!("all_compiles={} all_compile_us={}\n", s.all_count, s.all_us));
    for i in 0..4 { out.push_str(&format!("invalidated_{}_ranges={}\n", REASONS[i], s.invalidations[i])); }
    let mut rows: Vec<_> = s.pcs.iter().collect();
    rows.sort_by_key(|(pc,p)| (std::cmp::Reverse(p.us.iter().sum::<u64>()), **pc));
    for (pc,p) in rows {
        out.push_str(&format!("slow_pc=0x{:08X} thumb={} compiles={} total_us={}\n", pc & !1, pc & 1, p.slow_count, p.slow_us));
        out.push_str(&format!("pc=0x{:08X} thumb={} compiles={} repeats={} total_us={} max_us={} decode_calls={} decode_all_us={} stages_us={},{},{},{} guest_bytes={} host_bytes={} overlap_write={} overlap_overlay={} overlap_eviction={} overlap_vram={} history_gap={} last_overlay_hint={}\n", pc & !1, pc & 1, p.count, p.count.saturating_sub(1), p.us.iter().sum::<u64>(), p.max_us, p.decode_calls, p.decode_us, p.us[0],p.us[1],p.us[2],p.us[3],p.guest_bytes,p.host_bytes,p.overlap[0],p.overlap[1],p.overlap[2],p.overlap[3],p.history_gap,p.overlay_hint));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeats_track_overlap_and_lost_history_without_counting_unrelated_ranges() {
        reset();
        decoded(0x1001, 2);
        compiled(0x1001, 0x1040, 80, [2,3,4,5], 3);
        invalidated(0x1030, 16, OVERLAY);
        invalidated(0x2000, 16, WRITE);
        decoded(0x1001, 3);
        compiled(0x1001, 0x1040, 90, [3,4,5,6], 4);
        frame(40_000);
        for _ in 0..MAX_EVENTS+1 { invalidated(0x3000, 16, WRITE); }
        decoded(0x1001, 1);
        compiled(0x1001, 0x1040, 80, [1,1,1,1], 4);
        let s = state().lock().unwrap();
        let p = &s.pcs[&0x1001];
        assert_eq!(p.count, 3);
        assert_eq!(p.overlap, [0,1,0,0]);
        assert_eq!(p.history_gap, 1);
        assert_eq!(s.slow_us, 32);
        assert_eq!(s.slow_count, 2);
        assert_eq!(p.slow_count, 2);
        assert_eq!(p.slow_us, 32);
        assert_eq!(s.events.len(), MAX_EVENTS);
    }
    #[test]
    fn records_are_bounded_and_decode_only_is_not_a_compile() {
        reset();
        for pc in 0..MAX_PCS as u32+2 { decoded(pc*2, 1); }
        let s = state().lock().unwrap();
        assert_eq!(s.pcs.len(), MAX_PCS);
        assert_eq!(s.dropped_records, 2);
        assert_eq!(s.pcs[&0].count, 0);
    }
}
