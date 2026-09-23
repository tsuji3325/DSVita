//! Bounded compile-time observers. Never read extra guest memory for diagnostics.
use std::sync::{Mutex, OnceLock};
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};
pub(crate) const TARGETS: [u32; 2] = [0x021FC8FC, 0x022048A4];
const LIMIT: usize = 512;
const IMMEDIATE_LIMIT: usize = 64;
#[derive(Default)]
struct Target {
    previous: Option<(u32, Vec<(u32,u16)>)>,
    decodes: u64, same: u64, changed: u64, extent: u64, unsupported: u64,
    inst_count: usize, immediate_count: usize, immediate: Vec<(u32,u32)>,
    compiles: u64, times: [u64; 4], maxima: [u64; 4],
    blocks: usize, host_bytes: usize,
    folded: Vec<(u32,u32,u32)>, previous_folded: Option<Vec<(u32,u32,u32)>>,
    folded_overflow: u64, folded_same: u64, folded_changed: u64, folded_incomplete: u64,
}
fn state() -> &'static Mutex<[Target;2]> {
    static STATE: OnceLock<Mutex<[Target;2]>> = OnceLock::new();
    STATE.get_or_init(||Mutex::new(Default::default()))
}
pub(crate) fn watched(pc:u32, thumb:bool)->bool { thumb && TARGETS.contains(&pc) }
pub(crate) fn decoded(pc:u32,end:u32,instructions:impl Iterator<Item=(u32,u16)>,immediates:impl Iterator<Item=(u32,u32)>) {
    let Some(i)=TARGETS.iter().position(|p|*p==pc) else{return;};
    let mut s=state().lock().unwrap();let t=&mut s[i];t.decodes+=1;
    t.folded.clear();t.folded_overflow=0;
    let code:Vec<_>=instructions.take(LIMIT+1).collect();
    if code.is_empty() || code.len()>LIMIT || end != pc + code.len() as u32*2 {
        t.unsupported+=1;t.previous=None;return;
    }
    t.inst_count=code.len();
    if let Some((old_end,old))=&t.previous {
        if *old_end!=end {t.extent+=1;} else if *old==code {t.same+=1;} else {t.changed+=1;}
    }
    t.previous=Some((end,code));
    t.immediate.clear();t.immediate_count=0;
    for value in immediates.take(LIMIT) {
        t.immediate_count+=1;
        if t.immediate.len()<IMMEDIATE_LIMIT {t.immediate.push(value);}
    }
}
pub(crate) fn folded(target:u32,pc:u32,addr:u32,value:u32) {
    let Some(i)=TARGETS.iter().position(|p|*p==target) else{return;};
    let mut s=state().lock().unwrap();let t=&mut s[i];
    if t.folded.len()<IMMEDIATE_LIMIT {t.folded.push((pc,addr,value));} else {t.folded_overflow+=1;}
}
pub(crate) fn compiled(pc:u32,times:[u64;4],blocks:usize,host_bytes:usize) {
    let Some(i)=TARGETS.iter().position(|p|*p==pc) else{return;};
    let mut s=state().lock().unwrap();let t=&mut s[i];t.compiles+=1;
    for i in 0..4 {t.times[i]+=times[i];t.maxima[i]=t.maxima[i].max(times[i]);}
    t.blocks=blocks;t.host_bytes=host_bytes;
    if t.folded_overflow!=0 {t.folded_incomplete+=1;t.previous_folded=None;}
    else {
        if let Some(old)=&t.previous_folded {
            if *old==t.folded {t.folded_same+=1;} else {t.folded_changed+=1;}
        }
        t.previous_folded=Some(t.folded.clone());
    }
}
// Only patch events in the two cached allocations under investigation, not every RAM write.
const ARM_PATCH_START:u32=0x0225E8CC;
const ARM_PATCH_END:u32=0x0225E91C;
const ARM_PATCH_ROWS:usize=20*16*2;
static ARM_PATCH_COUNT:[AtomicU32;ARM_PATCH_ROWS]=[const {AtomicU32::new(0)};ARM_PATCH_ROWS];
static ARM_PATCH_FIRST:[AtomicU32;ARM_PATCH_ROWS]=[const {AtomicU32::new(0)};ARM_PATCH_ROWS];

const THUMB_PATCH_START:u32=0x021FAED6;
const THUMB_PATCH_END:u32=0x021FB05E;
const THUMB_PATCH_INSTS:usize=((THUMB_PATCH_END-THUMB_PATCH_START)/2) as usize;
const THUMB_PATCH_ROWS:usize=THUMB_PATCH_INSTS*16*2;
static THUMB_PATCH_COUNT:[AtomicU32;THUMB_PATCH_ROWS]=[const {AtomicU32::new(0)};THUMB_PATCH_ROWS];
static THUMB_PATCH_FIRST:[AtomicU32;THUMB_PATCH_ROWS]=[const {AtomicU32::new(0)};THUMB_PATCH_ROWS];

pub(crate) fn patched(owner:u32,pc:u32,addr:u32,write:bool) {
    if owner==ARM_PATCH_START {
        if !(ARM_PATCH_START..ARM_PATCH_END).contains(&pc) || pc&3!=0 {return;}
        let index=((pc-ARM_PATCH_START) as usize/4*16+((addr>>24)&15) as usize)*2+write as usize;
        if ARM_PATCH_COUNT[index].fetch_add(1,Relaxed)==0 {ARM_PATCH_FIRST[index].store(addr,Relaxed);}
        return;
    }
    if owner==THUMB_PATCH_START {
        // JIT metadata tags Thumb guest PCs in bit0. Normalize only for reporting/indexing.
        if pc&1==0 {return;}
        let pc=pc&!1;
        if !(THUMB_PATCH_START..THUMB_PATCH_END).contains(&pc) || (pc-THUMB_PATCH_START)&1!=0 {return;}
        let index=((pc-THUMB_PATCH_START) as usize/2*16+((addr>>24)&15) as usize)*2+write as usize;
        if THUMB_PATCH_COUNT[index].fetch_add(1,Relaxed)==0 {THUMB_PATCH_FIRST[index].store(addr,Relaxed);}
    }
}
pub(crate) fn reset() {
    *state().lock().unwrap()=Default::default();
    for v in ARM_PATCH_COUNT.iter().chain(ARM_PATCH_FIRST.iter()).chain(THUMB_PATCH_COUNT.iter()).chain(THUMB_PATCH_FIRST.iter()) {v.store(0,Relaxed);}
}
pub(crate) fn append_report(out:&mut String) {
    let s=state().lock().unwrap();
    out.push_str("[compile_focus]\nscope=next4_ARM9_Thumb_overlay_hotspots fresh_decode_identity_no_extra_guest_reads timing_units=us stages=setup,emit,finalize,insert last_immediates_are_addresses_not_values identity_includes_cycles_not_external_data\n");
    for (i,t) in s.iter().enumerate() {
        out.push_str(&format!("pc=0x{:08X} decodes={} repeat_same={} repeat_changed={} different_extent={} unsupported={} compiles={} last_inst_count={} last_basic_blocks={} last_host_bytes={} setup_us={} emit_us={} finalize_us={} insert_us={} setup_max_us={} emit_max_us={} finalize_max_us={} insert_max_us={} last_immediate_operands={} reported_immediates={}\n",TARGETS[i],t.decodes,t.same,t.changed,t.extent,t.unsupported,t.compiles,t.inst_count,t.blocks,t.host_bytes,t.times[0],t.times[1],t.times[2],t.times[3],t.maxima[0],t.maxima[1],t.maxima[2],t.maxima[3],t.immediate_count,t.immediate.len()));
        for (pc,addr) in &t.immediate {out.push_str(&format!("target=0x{:08X} last_immediate_pc=0x{pc:08X} address=0x{addr:08X}\n",TARGETS[i]));}
        out.push_str(&format!("target=0x{:08X} folded_repeat_same={} folded_repeat_changed={} folded_incomplete_compiles={} last_folded_count={} last_folded_overflow={} folded_scope=values_already_read_by_ARM32_emitter_not_all_runtime_dependencies\n",TARGETS[i],t.folded_same,t.folded_changed,t.folded_incomplete,t.folded.len(),t.folded_overflow));
        for (pc,addr,value) in &t.folded {out.push_str(&format!("target=0x{:08X} last_folded_pc=0x{pc:08X} address=0x{addr:08X} value=0x{value:08X}\n",TARGETS[i]));}
    }
    out.push_str("[jit_patch_focus]\nscope=patch_slow_mem_completed_in_current_saved_E8CC_or_FAED6_allocation region=(address>>24)&15 thumb_pc_normalized=true\n");
    for i in 0..ARM_PATCH_ROWS {let count=ARM_PATCH_COUNT[i].load(Relaxed);if count==0 {continue;}
        out.push_str(&format!("owner=0x{ARM_PATCH_START:08X} guest_pc=0x{:08X} region=0x{:X} write={} patches={} first_address=0x{:08X}\n",ARM_PATCH_START+(i/32*4) as u32,(i/2)%16,i%2,count,ARM_PATCH_FIRST[i].load(Relaxed)));
    }
    for i in 0..THUMB_PATCH_ROWS {let count=THUMB_PATCH_COUNT[i].load(Relaxed);if count==0 {continue;}
        out.push_str(&format!("owner=0x{THUMB_PATCH_START:08X} guest_pc=0x{:08X} region=0x{:X} write={} patches={} first_address=0x{:08X}\n",THUMB_PATCH_START+(i/32*2) as u32,(i/2)%16,i%2,count,THUMB_PATCH_FIRST[i].load(Relaxed)));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn observe(code:&[(u32,u16)],imm:&[(u32,u32)]) {decoded(TARGETS[0],TARGETS[0]+code.len() as u32*2,code.iter().copied(),imm.iter().copied());}
    #[test]
    fn identity_extent_cycles_limits_and_stage_totals() {
        reset();let a=[(1,1),(2,2)];observe(&a,&[(TARGETS[0],0x02001000)]);observe(&a,&[]);
        observe(&[(1,1),(3,2)],&[]);observe(&[(1,1),(3,4)],&[]);observe(&[(1,1)],&[]);
        compiled(TARGETS[0],[1,2,3,4],5,6);compiled(TARGETS[0],[4,3,2,1],7,8);
        {let s=state().lock().unwrap();assert_eq!((s[0].same,s[0].changed,s[0].extent),(1,2,1));assert_eq!(s[0].times,[5;4]);assert_eq!(s[0].maxima,[4,3,3,4]);}
        observe(&vec![(1,1);LIMIT+1],&[]);
        let s=state().lock().unwrap();assert_eq!(s[0].unsupported,1);assert!(s[0].previous.is_none());
    }
    #[test]
    fn bounded_immediates_patch_scope_report_and_reset() {
        reset();assert!(watched(TARGETS[0],true));assert!(!watched(TARGETS[0],false));
        observe(&[(1,1)],&vec![(TARGETS[0],0x02001000);70]);
        patched(0,0x0225E8CC,0x02000000,true);patched(ARM_PATCH_START,ARM_PATCH_END,0x02000000,true);
        patched(ARM_PATCH_START,ARM_PATCH_START,0x04000100,false);patched(ARM_PATCH_START,ARM_PATCH_START,0x04000200,false);
        patched(THUMB_PATCH_START,THUMB_PATCH_START|1,0x04000130,false);patched(THUMB_PATCH_START,THUMB_PATCH_START|1,0x04000134,false);
        let mut out=String::new();append_report(&mut out);
        assert!(out.contains("last_immediate_operands=70 reported_immediates=64"));
        assert!(out.contains("owner=0x0225E8CC guest_pc=0x0225E8CC region=0x4 write=0 patches=2 first_address=0x04000100"));
        assert!(out.contains("owner=0x021FAED6 guest_pc=0x021FAED6 region=0x4 write=0 patches=2 first_address=0x04000130"));
        assert!(!out.contains("write=1 patches="));
        reset();let mut out=String::new();append_report(&mut out);assert!(!out.contains("first_address="));
    }
    #[test]
    fn folded_value_changes_and_overflow_are_not_reported_as_equal() {
        reset();
        for value in [1,1,2] {
            observe(&[(1,1)],&[]);folded(TARGETS[0],TARGETS[0],0x02001000,value);
            compiled(TARGETS[0],[0;4],1,4);
        }
        {let s=state().lock().unwrap();assert_eq!((s[0].folded_same,s[0].folded_changed),(1,1));}
        observe(&[(1,1)],&[]);
        for _ in 0..65 {folded(TARGETS[0],TARGETS[0],0x02001000,2);}
        compiled(TARGETS[0],[0;4],1,4);
        let s=state().lock().unwrap();assert_eq!(s[0].folded_incomplete,1);assert!(s[0].previous_folded.is_none());
    }
}
