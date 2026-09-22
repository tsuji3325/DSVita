"""Host checks for cache identity/lifetime and the real entry-restoration method."""
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
assert asm.index('if asm.emit_nitrosdk_func(guest_pc, thumb)') < asm.index('let reuse_key =')
assert 'inst.imm_transfer_addr(guest_pc + i as u32 * 4).is_none()' in asm
assert 'asm.cpu == ARM9 && !thumb && crate::reuse_cache::TARGETS.contains(&guest_pc)' in asm
assert asm.index('drop(reuse_key);') < asm.index('entry(guest_pc);')

start=memory.index('    pub(crate) fn jit_restore_reuse(')
pos=memory.index('{',start)+1; depth=1
while depth:
    depth+=(memory[pos]=='{')-(memory[pos]=='}');pos+=1
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
fn restored_entry_covers_entire_block_and_reestablishes_both_cpu_write_guards() {
 let mut e=Emu{jit:Jit{mem:vec![0;256],jit_memory_map:Map{writes:vec![]}},live:vec![],protected:vec![]};
 let entry=e.jit_restore_reuse(0x0225E8CC,0x0225E91C,32);
 assert_eq!(entry as usize,e.jit.mem.as_ptr() as usize+32);
 let (pc,n,saved)=e.jit.jit_memory_map.writes[0];
 assert_eq!((pc,n),(0x0225E8CC,80));assert_eq!(saved.0,entry);
 assert_eq!(e.live,vec![(0x0225E8CC,0x0225E91C,false)]);
 assert_eq!(e.protected,vec![(ARM9,0x0225E8CC,0x0225E91C,false),(ARM7,0x0225E8CC,0x0225E91C,false)]);
}
'''
with tempfile.TemporaryDirectory() as tmp:
    tmp=Path(tmp)
    module='\n#[path = "'+str(Path('src/reuse_cache.rs').resolve())+'"] mod reuse_cache;\n'
    src=tmp/'reuse.rs';src.write_text(prefix+module+'impl Emu {\n'+restore+'\n}\n'+tests)
    exe=tmp/'reuse'
    subprocess.run(['rustc','+stable','--edition=2021','--test',str(src),'-o',str(exe)],check=True)
    subprocess.run([str(exe),'--test-threads=1'],check=True)
