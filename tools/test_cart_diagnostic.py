"""Host regression harness: execute the actual before/after cartridge methods.

Only surrounding hardware types are fixtures. Both method sets are extracted
verbatim; compare guest results, buffer reads, ready flags, IRQs and event queues.
Run from repository root. Requires rustc stable and the parent commit in Git.
"""
from pathlib import Path
import re
import subprocess
import tempfile

BASE = 'bf7ff8d9497ec71f8a648aef7aef876bf71f1d88'
path = 'src/core/memory/cartridge.rs'
before = subprocess.check_output(['git', 'show', f'{BASE}:{path}'], text=True)
after = Path(path).read_text()

def method(source, name):
    match = re.search(r'    (?:pub )?fn ' + name + r'\(', source)
    assert match, name
    start = source.index('{', match.start())
    depth = 1
    end = start + 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[match.start():end]

names = ['cartridge_get_rom_ctrl', 'cartridge_get_rom_data_in', 'cartridge_set_rom_ctrl']
# The original behavior bodies must remain unchanged, not merely pass fixtures.
for name in names:
    original = method(before, name)
    inner = method(after, name + '_inner')
    assert original[original.index('{'):] == inner[inner.index('{'):], name

prefix = r'''
#![allow(dead_code)]
use std::ops::{Deref, Index, IndexMut};
macro_rules! debug_println { ($($x:tt)*) => {}; }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CpuType { ARM9, ARM7 }
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct RomCtrl(u32);
impl From<u32> for RomCtrl { fn from(x:u32)->Self { Self(x) } }
impl From<RomCtrl> for u32 { fn from(x:RomCtrl)->Self { x.0 } }
impl RomCtrl {
 fn data_word_status(self)->bool { self.0 & (1<<23) != 0 }
 fn block_start_status(self)->bool { self.0 & (1<<31) != 0 }
 fn resb_release_reset(self)->bool { self.0 & (1<<29) != 0 }
 fn data_block_size(self)->u8 { ((self.0>>24)&7) as u8 }
 fn bit(&mut self,b:u32,v:bool) { self.0=(self.0 & !(1<<b)) | ((v as u32)<<b); }
 fn set_data_word_status(&mut self,v:bool) { self.bit(23,v); }
 fn set_block_start_status(&mut self,v:bool) { self.bit(31,v); }
 fn set_resb_release_reset(&mut self,v:bool) { self.bit(29,v); }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct AuxSpiCnt(bool);
impl AuxSpiCnt { fn transfer_ready_irq(&self)->bool { self.0 } }
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Inner { rom_ctrl:RomCtrl, aux_spi_cnt:AuxSpiCnt, block_size:u16, read_count:u16, encrypted:bool, bus_cmd_out:u64 }
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Slots([Inner;2]);
impl Index<CpuType> for Slots { type Output=Inner; fn index(&self,c:CpuType)->&Inner { &self.0[c as usize] } }
impl IndexMut<CpuType> for Slots { fn index_mut(&mut self,c:CpuType)->&mut Inner { &mut self.0[c as usize] } }
#[derive(Clone, Debug, PartialEq, Eq)]
enum CmdMode { Header, Chip, Secure, Data, None }
#[derive(Clone, Debug, PartialEq, Eq)]
struct Io { file_size:u32, reads:Vec<(u32,usize)> }
impl Io {
 fn read_slice(&mut self,addr:u32,out:&mut [u8])->Result<(),()> {
   self.reads.push((addr,out.len()));
   for (i,b) in out.iter_mut().enumerate() { *b=(addr.wrapping_add(i as u32)*13) as u8; }
   Ok(())
 }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Cartridge { inner:Slots, io:Io, cmd_mode:CmdMode, read_buf:Vec<u8> }
#[derive(Clone, Debug, PartialEq, Eq)]
struct ImmEventType(CpuType);
impl ImmEventType { fn cartridge_word_read(cpu:CpuType)->Self { Self(cpu) } }
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Cm { events:Vec<ImmEventType> }
impl Cm { fn schedule_imm(&mut self,e:ImmEventType) { self.events.push(e); } }
enum InterruptFlag { NdsSlotTransferCompletion }
struct Emu<const D:bool> { cartridge:Cartridge, cm:Cm, irqs:Vec<CpuType> }
impl<const D:bool> Emu<D> {
 fn new(cpu:CpuType)->Self {
   let mut inner=Slots::default(); inner[cpu].aux_spi_cnt.0=true;
   inner[cpu].bus_cmd_out=u64::to_be(0xB700008000000000);
   Self { cartridge:Cartridge { inner, io:Io { file_size:0x100000, reads:Vec::new() }, cmd_mode:CmdMode::None, read_buf:vec![0;16384] }, cm:Cm::default(), irqs:Vec::new() }
 }
 fn cpu_send_interrupt(&mut self,c:CpuType,_:InterruptFlag) { self.irqs.push(c); }
}
mod utils { pub fn read_from_mem(b:&[u8],o:u32)->u32 { u32::from_le_bytes(b[o as usize..o as usize+4].try_into().unwrap()) } }
fn equal(a:&Emu<false>,b:&Emu<true>) { assert_eq!(a.cartridge,b.cartridge); assert_eq!(a.cm,b.cm); assert_eq!(a.irqs,b.irqs); }
'''

tests = r'''
#[test]
fn instrumented_card_matches_original_reads_events_and_interrupts() {
 for cpu in [CpuType::ARM9,CpuType::ARM7] {
  for size_code in [0,1,2,3,4,5,6,7] {
   for command in [0xB700008000000000u64, 0, 0xB800000000000000] {
    crate::perf_diag::reset();
    crate::perf_diag::publish_arm9_state(0x020DCE54,crate::perf_diag::PHASE_JIT);
    let mut a=Emu::<false>::new(cpu); let mut b=Emu::<true>::new(cpu);
    a.cartridge.inner[cpu].bus_cmd_out=command.to_be(); b.cartridge.inner[cpu].bus_cmd_out=command.to_be();
    let control=0x80000000 | (size_code<<24);
    a.cartridge_set_rom_ctrl(cpu,u32::MAX,control); b.cartridge_set_rom_ctrl(cpu,u32::MAX,control); equal(&a,&b);
    // Read before ready, poll, transfer every word, then read after completion.
    assert_eq!(a.cartridge_get_rom_data_in(cpu),b.cartridge_get_rom_data_in(cpu)); equal(&a,&b);
    let words=a.cartridge.inner[cpu].block_size as usize/4;
    for _ in 0..words {
      assert_eq!(a.cartridge_get_rom_ctrl(cpu),b.cartridge_get_rom_ctrl(cpu)); equal(&a,&b);
      assert_eq!(a.cartridge_get_rom_ctrl(cpu),b.cartridge_get_rom_ctrl(cpu)); equal(&a,&b);
      assert_eq!(a.cartridge_get_rom_data_in(cpu),b.cartridge_get_rom_data_in(cpu)); equal(&a,&b);
    }
    assert_eq!(a.cartridge_get_rom_data_in(cpu),b.cartridge_get_rom_data_in(cpu)); equal(&a,&b);
    assert_eq!(a.irqs.len(),1);
    let saved=crate::perf_diag::replace_arm9_pc(0);
    assert_eq!(saved,crate::perf_diag::pack_pc_phase(0x020DCE54,crate::perf_diag::PHASE_JIT));
    crate::perf_diag::record_cpu_frame_interval(50_000);
    crate::perf_diag::write_report();
    let report=std::fs::read_to_string("frame_perf.log").unwrap();
    let expected=if cpu==CpuType::ARM9 { words } else { 0 };
    assert!(report.contains(&format!("data_words all={} slow={}\n",expected,expected)));
   }
  }
 }
}
#[test]
fn partial_control_mask_and_ongoing_transfer_preserve_semantics() {
 crate::perf_diag::reset();
 let cpu=CpuType::ARM9;
 let mut a=Emu::<false>::new(cpu); let mut b=Emu::<true>::new(cpu);
 // Size is retained from old control, even though the start write has no size bits.
 a.cartridge.inner[cpu].rom_ctrl=RomCtrl(1<<24); b.cartridge.inner[cpu].rom_ctrl=RomCtrl(1<<24);
 a.cartridge_set_rom_ctrl(cpu,0x80000000,0x80000000); b.cartridge_set_rom_ctrl(cpu,0x80000000,0x80000000); equal(&a,&b);
 a.cartridge_set_rom_ctrl(cpu,0,0x80000000); b.cartridge_set_rom_ctrl(cpu,0,0x80000000); equal(&a,&b);
 crate::perf_diag::record_cpu_frame_interval(40_000);
 crate::perf_diag::write_report();
 let r=std::fs::read_to_string("frame_perf.log").unwrap();
 assert!(r.contains("transfer_starts all=1 slow=1\n"));
 assert!(r.contains("requested_bytes all=512 slow=512\n"));
}
'''

with tempfile.TemporaryDirectory() as tmp:
    tmp = Path(tmp)
    code = prefix + '\n#[path = ' + repr(str(Path('src/perf_diag.rs').resolve())).replace("'", '"') + '] mod perf_diag;\n'
    code += 'impl Emu<false> {\n' + '\n'.join(method(before,n) for n in names) + '\n}\n'
    code += 'impl Emu<true> {\n' + '\n'.join(method(after,n) for n in names+[n+'_inner' for n in names]) + '\n}\n'
    code += tests
    source = tmp/'cart_tests.rs'
    source.write_text(code)
    executable = tmp/'cart_tests'
    subprocess.run(['rustc','+stable','--edition=2021','--test',str(source),'-o',str(executable)],check=True)
    subprocess.run([str(executable),'--test-threads=1'],cwd=tmp,check=True)
