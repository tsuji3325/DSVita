"""Host checks for cache identity/lifetime and ARM/Thumb entry restoration."""
from pathlib import Path
import subprocess
import tempfile

memory = Path('src/jit/jit_memory.rs').read_text()
asm = Path('src/jit/jit_asm.rs').read_text()
mem = Path('src/core/memory/mem.rs').read_text()
macro = mem[mem.index('macro_rules! write_main'):mem.index('macro_rules! write_wram')]
assert 'capture(' not in macro and 'written(' not in macro
assert macro.index('$write;') < macro.index('invalidate_block(')
for name in ['init', 'reset_blocks']:
    body = memory.split('fn '+name+'(',1)[1].split('{',1)[1]
    assert body.lstrip().startswith(('self.reuse_cache.clear();','// Before ANY'))
assert memory.index('self.reuse_cache.clear();',memory.index('fn reset_blocks')) < memory.index('.pop_front()',memory.index('fn reset_blocks'))

assert asm.index('if asm.emit_nitrosdk_func(guest_pc, thumb)') < asm.index('let mut reuse_key =')
assert 'crate::reuse_cache::ARM_TARGETS.contains(&guest_pc)' in asm
assert 'crate::reuse_cache::THUMB_TARGETS.contains(&guest_pc)' in asm
reuse = Path('src/reuse_cache.rs').read_text()
assert '0x02203714' in reuse and '0x021F2E34' in reuse
assert '0x021F6AF4' in reuse and '0x021F7234' in reuse
assert '0x021F81A0' in reuse and '0x021F994C' in reuse
assert 'THUMB_TARGETS: [u32; 8]' in reuse
assert 'entries: [Option<Entry>; 12]' in reuse
assert 'inst.imm_transfer_addr(guest_pc + i as u32 * 4).is_none()' in asm
assert 'asm.analyzer.can_imm_load(addr)' in asm
assert '(addr & 0xFF000000) != regions::MAIN_OFFSET' in asm
assert 'asm.emu.mem_read::<{ ARM9 }, u32>(addr)' in asm
assert 'end: guest_pc_end + 2' in asm and 'thumb: true' in asm
assert 'entry(guest_pc | 1);' in asm
assert '(insert_entry as usize & !1)' in asm
assert 'accept_known_patch(' in memory
assert '0x0225E8CC' in Path('src/reuse_cache.rs').read_text()
assert '0x04100010' in Path('src/reuse_cache.rs').read_text()
assert 'patch_offset,' in memory and 'fast_mem,' in memory
patch_body=memory[memory.index('pub unsafe fn patch_slow_mem'):memory.index('\n    }\n}',memory.index('pub unsafe fn patch_slow_mem'))]
assert '.to_vec()' not in patch_body and 'Mutex' not in patch_body
assert memory.index('execute_patch_slow_mem::<true>') < memory.index('accept_known_patch(',memory.index('pub unsafe fn patch_slow_mem'))
assert asm.index('drop(reuse_key);') < asm.index('entry(guest_pc);')

start=memory.index('    pub(crate) fn jit_restore_reuse(')
pos=memory.index('{',start)+1
depth=1
while depth:
    depth+=(memory[pos]=='{')-(memory[pos]=='}')
    pos+=1
restore=memory[start:pos]

prefix=r'''
#![allow(dead_code)]
const ARM9:u8=0; const ARM7:u8=1;
#[derive(Clone,Copy)] struct JitEntry(*const extern "C" fn(u32));
struct Map { writes:Vec<(u32,usize,JitEntry)> }
impl Map {fn write_jit_entries(&mut self,pc:u32,n:usize,e:JitEntry){self.writes.push((pc,n,e));}}
struct Jit {mem:Vec<u8>,jit_memory_map:Map}
struct Emu {jit:Jit,live:Vec<(u32,u32,bool)>,protected:Vec<(u8,u32,u32,bool)>}
mod regions {pub struct Region;pub const MAIN_REGION:Region=Region;}
impl Emu {
fn jit_set_live_range(&mut self,pc:u32,end:u32,thumb:bool){self.live.push((pc,end,thumb));}
fn jit_protect_region<const CPU:u8>(&mut self,pc:u32,end:u32,thumb:bool,_:&regions::Region){self.protected.push((CPU,pc,end,thumb));}
}
'''

tests=r'''
#[test]
fn restored_arm_entry_covers_entire_block_and_reestablishes_both_cpu_write_guards() {
 let mut e=Emu{jit:Jit{mem:vec![0;256],jit_memory_map:Map{writes:vec![]}},live:vec![],protected:vec![]};
 let entry=e.jit_restore_reuse(0x0225E8CC,0x0225E91C,32,false);
 assert_eq!(entry as usize,e.jit.mem.as_ptr() as usize+32);
 let (pc,n,saved)=e.jit.jit_memory_map.writes[0];
 assert_eq!((pc,n),(0x0225E8CC,80));assert_eq!(saved.0,entry);
 assert_eq!(e.live,vec![(0x0225E8CC,0x0225E91C,false)]);
 assert_eq!(e.protected,vec![(ARM9,0x0225E8CC,0x0225E91C,false),(ARM7,0x0225E8CC,0x0225E91C,false)]);
}
#[test]
fn restored_thumb_entry_sets_bit_zero_and_uses_thumb_ranges() {
 let mut e=Emu{jit:Jit{mem:vec![0;256],jit_memory_map:Map{writes:vec![]}},live:vec![],protected:vec![]};
 let base=e.jit.mem.as_ptr() as usize;
 let entry=e.jit_restore_reuse(0x021FA25A,0x021FA412,64,true);
 assert_eq!(entry as usize,(base+64)|1);
 let (pc,n,saved)=e.jit.jit_memory_map.writes[0];
 assert_eq!((pc,n),(0x021FA25A,440));assert_eq!(saved.0,entry);
 assert_eq!(e.live,vec![(0x021FA25A,0x021FA412,true)]);
 assert_eq!(e.protected,vec![(ARM9,0x021FA25A,0x021FA412,true),(ARM7,0x021FA25A,0x021FA412,true)]);
}
'''

with tempfile.TemporaryDirectory() as tmp:
    tmp=Path(tmp)
    module='\n#[path = "'+str(Path('src/reuse_cache.rs').resolve())+'"] mod reuse_cache;\n'
    src=tmp/'reuse.rs'; src.write_text(prefix+module+'impl Emu {\n'+restore+'\n}\n'+tests)
    exe=tmp/'reuse'
    subprocess.run(['rustc','+stable','--edition=2021','--test',str(src),'-o',str(exe)],check=True)
    subprocess.run([str(exe),'--test-threads=1'],check=True)
