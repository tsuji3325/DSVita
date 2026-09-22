//! Bounded observers for the remaining write/recompile hotspots. No policy changes.
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
pub(crate) const START: u32 = 0x0225E800;
pub(crate) const LEN: usize = 768;
pub(crate) const OFFSET: usize = (START & 0x3FFFFF) as usize;
pub(crate) const TARGETS: [u32; 4] = [0x0225E8CC,0x0225E9A4,0x0225E9B4,0x0225E9D8];
pub(crate) const CODE: u8 = 1;
pub(crate) const FOLDED: u8 = 2;
pub(crate) const SDK_READ: u8 = 4;
const FALLBACK: u8 = 8;
const ROW_LIMIT: usize = 1024;
fn canonical(a:u32)->u32 {0x02000000 | (a & 0x3FFFFF)}
fn contains(addr:u32,size:usize,byte:u32)->bool {
    size >= 0x400000 || ((byte.wrapping_sub(addr) & 0x3FFFFF) as usize) < size
}
pub(crate) fn watched_page(a:u32)->bool {
    a & 0x0F000000 == 0x02000000 && (START..START+LEN as u32).contains(&canonical(a))
}
#[inline]
pub(crate) fn touches(a:u32,size:usize)->bool {
    if size==0 {return false;}
    let begin=(a & 0x3FFFFF) as u64;
    let end=begin.saturating_add(size as u64);
    (begin < (OFFSET+LEN) as u64 && end > OFFSET as u64) || end > 0x400000+OFFSET as u64
}
pub(crate) fn capture(bytes:&[u8])->Box<[u8;LEN]> {Box::new(bytes.try_into().unwrap())}
#[derive(Default)]
struct Dependency {kinds:u8, first:[u32;3],last:[u32;3]}
#[derive(Default)]
struct Target {
    end:Option<u32>, bytes:Option<Box<[u8;LEN]>>, compiles:u64, same:u64, changed:u64, extent:u64, unsupported:u64,
    write_changed:u64, write_same:u64, write_unknown:u64, invalidating_changed:u64, invalidating_same:u64,
}
#[derive(Default)]
struct Counts {
    writes:u64, changed:u64, invalidating:u64, reasons:[u64;3], hits:[u64;4], invalidating_hits:[u64;4], last_overlay:u32,
}
impl Counts {
    fn add(&mut self,changed:bool,invalidating:bool,reason:usize,hits:u8,overlay:u32) {
        self.writes+=1;self.changed+=changed as u64;self.invalidating+=invalidating as u64;
        self.reasons[reason]+=1;self.last_overlay=overlay;
        for i in 0..4 {if hits & (1<<i)!=0 {self.hits[i]+=1;self.invalidating_hits[i]+=invalidating as u64;}}
    }
    fn text(&self)->String {format!("writes={} window_changed={} invalidating={} reason_preservable={} reason_boundary={} reason_dependency={} hit_code={} hit_folded={} hit_sdk_read={} hit_fallback={} invalidating_hit_code={} invalidating_hit_folded={} invalidating_hit_sdk_read={} invalidating_hit_fallback={} last_overlay_hint={}", self.writes,self.changed,self.invalidating,self.reasons[0],self.reasons[1],self.reasons[2],self.hits[0],self.hits[1],self.hits[2],self.hits[3],self.invalidating_hits[0],self.invalidating_hits[1],self.invalidating_hits[2],self.invalidating_hits[3],self.last_overlay)}
}
#[derive(Default)]
struct Changes {count:u64, first_old:u8,first_new:u8}
#[derive(Default)]
struct State {
    targets:[Target;4],deps:BTreeMap<usize,Dependency>,bytes:BTreeMap<usize,Changes>,totals:Counts,
    rows:BTreeMap<(u32,usize,bool,u32),Counts>,dropped:u64,dropped_invalidating:u64,
}
fn state()->&'static Mutex<State> {static S:OnceLock<Mutex<State>>=OnceLock::new();S.get_or_init(||Mutex::new(State::default()))}
pub(crate) fn reset(){*state().lock().unwrap()=State::default();}
pub(crate) fn clear_dependencies(){state().lock().unwrap().deps.clear();}
pub(crate) fn mark(addr:u32,size:usize,kind:u8,owner:u32) {
    if addr & 0xFF000000 != 0x02000000 || size==0 {return;}
    // Mirror the footprint's conservative all-main-RAM fallback.
    let fallback=size>=0x400000 || (addr as u64).saturating_add(size as u64)>0x03000000;
    if !fallback && !touches(addr,size){return;}
    let mut s=state().lock().unwrap();
    for i in 0..LEN {
        if fallback || contains(addr,size,START+i as u32) {
            let d=s.deps.entry(i).or_default();
            if fallback {d.kinds|=FALLBACK;} else {
                let slot=match kind {CODE=>0,FOLDED=>1,SDK_READ=>2,_=>unreachable!()};
                if d.kinds & kind==0 {d.first[slot]=owner;}
                d.kinds|=kind;d.last[slot]=owner;
            }
        }
    }
}
pub(crate) fn compiled(pc:u32,end:u32,thumb:bool,bytes:&[u8]) {
    let Some(i)=TARGETS.iter().position(|p|*p==pc) else{return;};
    let mut s=state().lock().unwrap();let t=&mut s.targets[i];t.compiles+=1;
    if thumb || end<=pc || end>START+LEN as u32 {t.unsupported+=1;t.end=None;t.bytes=None;return;}
    let range=(pc-START) as usize..(end-START) as usize;
    if let Some(old)=&t.bytes {
        if t.end!=Some(end) {t.extent+=1;} else if old[range.clone()]==bytes[range] {t.same+=1;} else {t.changed+=1;}
    }
    t.end=Some(end);t.bytes=Some(capture(bytes));
}
pub(crate) fn written(addr:u32,size:usize,arm9:bool,source:u32,overlay:u32,before:Box<[u8;LEN]>,after:&[u8],invalidating:bool,reason:usize) {
    assert_eq!(after.len(),LEN);
    let mut s=state().lock().unwrap();
    let hits=s.deps.iter().filter(|(i,_)|contains(addr,size,START+**i as u32)).fold(0,|a,(_,d)|a|d.kinds);
    let changed=before.as_slice()!=after;
    s.totals.add(changed,invalidating,reason,hits,overlay);
    let key=(addr,size,arm9,source);
    if s.rows.contains_key(&key)||s.rows.len()<ROW_LIMIT {s.rows.entry(key).or_default().add(changed,invalidating,reason,hits,overlay);}
    else{s.dropped+=1;s.dropped_invalidating+=invalidating as u64;}
    for (i,t) in s.targets.iter_mut().enumerate() {
        if let Some(end)=t.end {
            let range=(TARGETS[i]-START) as usize..(end-START) as usize;
            if before[range.clone()]!=after[range] {t.write_changed+=1;t.invalidating_changed+=invalidating as u64;}
            else {t.write_same+=1;t.invalidating_same+=invalidating as u64;}
        }else{t.write_unknown+=1;}
    }
    for i in 0..LEN {if before[i]!=after[i] {
        let b=s.bytes.entry(i).or_default();if b.count==0 {b.first_old=before[i];b.first_new=after[i];}b.count+=1;
    }}
}
pub(crate) fn append_report(out:&mut String) {
    let s=state().lock().unwrap();
    out.push_str("[remaining_write_dependencies]\nwatched_range=0x0225E800-0x0225EAFF scope=slow_main_RAM_write_paths aliases_normalized\n");
    out.push_str("reason=0_preservable_1_zero_or_page_boundary_2_historical_dependency hits=nonexclusive_requested_invalidation_span not_actual_changed_bytes\n");
    out.push_str("dependency_note=session_history_not_current_ownership code_includes_HLE_candidates sdk_read_includes_pattern_detection_reads owner=code_start_or_literal_read_instruction_pc\n");
    out.push_str("source_note=last_ARM9_bucket_and_phase_not_exact_writer row_key_excludes_overlay last_overlay_hint_not_ownership target_range_may_include_data direct_fast_writes_unobserved\n");
    out.push_str(&format!("row_limit={} dropped_row_observations={} dropped_invalidating_observations={}\ntotal {}\n",ROW_LIMIT,s.dropped,s.dropped_invalidating,s.totals.text()));
    for (i,t) in s.targets.iter().enumerate() {
        out.push_str(&format!("target=0x{:08X} compiles={} repeat_same={} repeat_changed={} different_extent={} unsupported={} last_end=0x{:08X} write_changed={} write_same={} write_unknown={} invalidating_changed={} invalidating_same={}\n",TARGETS[i],t.compiles,t.same,t.changed,t.extent,t.unsupported,t.end.unwrap_or(0),t.write_changed,t.write_same,t.write_unknown,t.invalidating_changed,t.invalidating_same));
    }
    let mut rows:Vec<_>=s.rows.iter().collect();rows.sort_by_key(|(k,c)|(std::cmp::Reverse(c.invalidating),std::cmp::Reverse(c.writes),**k));
    for ((a,n,cpu,source),c) in rows {out.push_str(&format!("dest=0x{:08X} physical=0x{:08X} requested_span={} cpu={} source_bucket=0x{:08X} phase={} {}\n",a,canonical(*a),n,if *cpu{"ARM9"}else{"ARM7"},source & !63,source & 63,c.text()));}
    for (i,d) in &s.deps {out.push_str(&format!("dependency_addr=0x{:08X} kinds={} code_first_pc=0x{:08X} code_last_pc=0x{:08X} folded_first_pc=0x{:08X} folded_last_pc=0x{:08X} sdk_first_pc=0x{:08X} sdk_last_pc=0x{:08X}\n",START+*i as u32,d.kinds,d.first[0],d.last[0],d.first[1],d.last[1],d.first[2],d.last[2]));}
    for (i,b) in &s.bytes {out.push_str(&format!("changed_addr=0x{:08X} writes={} first_old=0x{:02X} first_new=0x{:02X}\n",START+*i as u32,b.count,b.first_old,b.first_new));}
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn separate_code_literal_sdk_and_target_byte_changes() {
        reset();let mut b=[0;LEN];
        compiled(TARGETS[0],TARGETS[0]+80,false,&b);
        mark(START,4,FOLDED,0x02001000);mark(START,4,SDK_READ,0x02002000);
        mark(TARGETS[0],80,CODE,TARGETS[0]);
        let old=capture(&b);b[0]=1;written(START,1,true,0,3,old,&b,true,2);
        compiled(TARGETS[0],TARGETS[0]+80,false,&b);
        let old=capture(&b);b[(TARGETS[0]-START) as usize]=7;written(TARGETS[0],1,true,0,3,old,&b,true,2);
        compiled(TARGETS[0],TARGETS[0]+80,false,&b);
        let s=state().lock().unwrap();assert_eq!(s.targets[0].same,1);assert_eq!(s.targets[0].changed,1);
        assert_eq!(s.targets[0].invalidating_same,1);assert_eq!(s.targets[0].invalidating_changed,1);
        assert_eq!(s.totals.invalidating_hits,[1,1,1,0]);assert_eq!(s.deps[&0].first[1],0x02001000);
    }
    #[test]
    fn aliases_limits_unknown_extent_and_fallback_are_reported() {
        reset();assert!(touches(START+0x400000,4));assert!(!touches(START-4,4));assert!(!touches(START,0));
        mark(START+0x400000,1,FOLDED,123);
        let b=[0;LEN];compiled(TARGETS[0],START+LEN as u32+1,false,&b);
        for i in 0..ROW_LIMIT+1 {written(START,1,true,i as u32,0,capture(&b),&b,true,2);}
        {let s=state().lock().unwrap();assert_eq!(s.rows.len(),ROW_LIMIT);assert_eq!(s.dropped_invalidating,1);assert_eq!(s.targets[0].unsupported,1);assert_eq!(s.deps[&0].last[1],123);}
        clear_dependencies();mark(0x02FFFFFC,8,CODE,7);
        let s=state().lock().unwrap();assert_eq!(s.deps.len(),LEN);assert_eq!(s.deps[&0].kinds,FALLBACK);
    }
}
