"""Run the real main-memory write macro against before/after fixtures."""
from pathlib import Path
import subprocess
import tempfile
BASE = '937beff079a4377343912cea207871d54779c7b9'
path = 'src/core/memory/mem.rs'
before = subprocess.check_output(['git','show',f'{BASE}:{path}'],text=True)
after = Path(path).read_text()
def macro(source):
    start=source.index('macro_rules! write_main {')
    end=source.index('\nmacro_rules! write_wram',start)
    return source[start:end]
old,new=macro(before),macro(after)
# The original memory-write operations and surrounding paths stay identical.
assert before.replace(old,'') == after.replace(new,'').replace('write_main!(CPU, ', 'write_main!(')
prefix=r'''
#![allow(dead_code)]
const ARM9:bool=true;
mod regions { pub const MAIN_SIZE:u32=0x400000; pub struct Region { pub shm_offset:usize } pub const MAIN_REGION:Region=Region{shm_offset:0}; }
mod perf_diag { pub fn source_hint()->u32 {0} pub fn overlay_hint()->u32 {3} }
struct Mem { shm:Vec<u8> }
#[derive(Default)]
struct Jit { calls:Vec<(u32,usize)> }
impl Jit {
 fn invalidate_block(&mut self,a:u32,n:usize) {self.calls.push((a,n));}
 fn diagnostic_target_live_invalidation(&self,_a:u32,_n:usize)->bool {true}
 fn diagnostic_dependency_invalidation(&self,_a:u32,_n:usize)->(bool,usize) {(true,2)}
}
struct Emu { mem:Mem,jit:Jit }
impl Emu {fn new()->Self {Self{mem:Mem{shm:vec![0;0x400000]},jit:Jit::default()}}}
'''
tests=r'''
fn original(cpu:bool, addr:u32, span:usize, data:&[u8], emu:&mut Emu, effects:&mut u32) {
 write_main_old!(addr,span,emu,offset, { *effects+=1; emu.mem.shm[offset as usize..offset as usize+data.len()].copy_from_slice(data); });
}
fn diagnostic(cpu:bool, addr:u32, span:usize, data:&[u8], emu:&mut Emu, effects:&mut u32) {
 write_main!(cpu,addr,span,emu,offset, { *effects+=1; emu.mem.shm[offset as usize..offset as usize+data.len()].copy_from_slice(data); });
}
#[test]
fn actual_write_macro_preserves_memory_effects_and_invalidation_arguments() {
 for cpu in [true,false] {
  for (addr,span,len) in [(0x0225E8CC,4,4),(0x0265E9B4,96,96),(0x02010000,4,4),(0x0225F400,4,4),(0x0225F4F8,4,4),(0x0265F4F8,64,64),(0x0225F4F8,64,4),(0x0225F3F0,64,64),(0x0225F5FC,8,8)] {
   crate::write_diag::reset();
   crate::dependency_diag::reset();
   let mut a=Emu::new();let mut b=Emu::new();let mut ac=0;let mut bc=0;
   let off=crate::write_diag::OFFSET;
   crate::write_diag::compiled(0x0225F544,false,&b.mem.shm[off..off+crate::write_diag::LEN]);
   let data=vec![7;len];
   original(cpu,addr,span,&data,&mut a,&mut ac);
   diagnostic(cpu,addr,span,&data,&mut b,&mut bc);
   assert_eq!(a.mem.shm,b.mem.shm);assert_eq!(a.jit.calls,b.jit.calls);assert_eq!((ac,bc),(1,1));
   // Same-value writes must still perform the original invalidation.
   original(cpu,addr,span,&data,&mut a,&mut ac);
   diagnostic(cpu,addr,span,&data,&mut b,&mut bc);
   assert_eq!(a.mem.shm,b.mem.shm);assert_eq!(a.jit.calls,b.jit.calls);assert_eq!((ac,bc),(2,2));
  }
 }
}
'''
with tempfile.TemporaryDirectory() as tmp:
    tmp=Path(tmp)
    module='\n#[path = '+repr(str(Path('src/write_diag.rs').resolve())).replace("'",'"')+'] mod write_diag;\n'
    module += '\n#[path = '+repr(str(Path('src/dependency_diag.rs').resolve())).replace("'",'"')+'] mod dependency_diag;\n'
    source=tmp/'write_tests.rs'
    source.write_text(prefix+module+old.replace('write_main {','write_main_old {')+new+tests)
    executable=tmp/'write_tests'
    subprocess.run(['rustc','+stable','--edition=2021','--test',str(source),'-o',str(executable)],check=True)
    subprocess.run([str(executable),'--test-threads=1'],cwd=tmp,check=True)
