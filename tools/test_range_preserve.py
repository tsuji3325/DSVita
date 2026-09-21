"""Exercise real invalidation methods and live-range registration on host fixtures."""
from pathlib import Path
import subprocess
import tempfile
source=Path('src/jit/jit_memory.rs').read_text()
def method(name):
 start=source.index('    pub fn '+name+'(')
 pos=source.index('{',start)+1; depth=1
 while depth:
  depth+=(source[pos]=='{')-(source[pos]=='}');pos+=1
 return source[start:pos]
prefix=r'''
#![allow(dead_code)]
use std::cell::UnsafeCell;
use std::collections::BTreeMap;
const JIT_LIVE_RANGE_PAGE_SIZE_SHIFT:u32=8;
const JIT_LIVE_RANGE_PAGE_SIZE:u32=256;
const DEFAULT_JIT_ENTRY:u32=0;
fn unlikely(v:bool)->bool{v}
macro_rules! debug_println {($($t:tt)*)=>{}}
mod compile_diag {pub const WRITE:usize=0;pub const OVERLAY:usize=1;pub fn invalidated(_:u32,_:usize,_:usize){}}
mod perf_diag {pub fn record_preserved_main_write(){}}
mod utils {pub fn align_up(v:usize,a:usize)->usize{(v+a-1)&!(a-1)}}
struct Map {live:Vec<UnsafeCell<u8>>,entries:BTreeMap<u32,u32>}
impl Map {
 fn new()->Self{Self{live:(0..2048).map(|_|UnsafeCell::new(0)).collect(),entries:BTreeMap::new()}}
 fn get_live_range(&self,a:u32)->*mut u8{self.live[((a&0x3FFFFF)>>11) as usize].get()}
 fn has_jit_block(&self,a:u32)->bool{unsafe{*self.get_live_range(a)&(1<<((a>>8)&7))!=0}}
 fn write_jit_entries(&mut self,a:u32,n:usize,v:u32){for i in (0..n).step_by(2){self.entries.insert((a+i as u32)&0x3FFFFF,v);}}
 fn entry(&self,a:u32)->u32{*self.entries.get(&(a&0x3FFFFF)).unwrap_or(&0)}
}
struct JitMemory {main_code_footprint:main_code_footprint::MainCodeFootprint,jit_memory_map:Map}
impl JitMemory {fn new()->Self{Self{main_code_footprint:main_code_footprint::MainCodeFootprint::new(),jit_memory_map:Map::new()}}}
struct Emu {jit:JitMemory}
impl Emu {
 fn new()->Self{Self{jit:JitMemory::new()}}
 fn register(&mut self,a:u32,end:u32,thumb:bool){self.jit_set_live_range(a,end,thumb);self.jit.jit_memory_map.write_jit_entries(a,(end-a) as usize,1);}
}
'''
tests=r'''
#[test]
fn data_write_preserves_entry_and_live_bit_then_real_code_write_invalidates() {
 for thumb in [false,true] {
  let mut e=Emu::new();e.register(0x0225F480,0x0225F4A0,thumb);
  e.jit.invalidate_block(0x0225F400,1);
  assert_eq!(e.jit.jit_memory_map.entry(0x0225F480),1);
  assert!(e.jit.jit_memory_map.has_jit_block(0x0225F480));
  e.jit.invalidate_block(0x0265F49F,1); // last byte, alias, no entry at odd address
  assert_eq!(e.jit.jit_memory_map.entry(0x0225F480),0);
  assert!(!e.jit.jit_memory_map.has_jit_block(0x0225F480));
 }
}
#[test]
fn literal_other_block_overlay_and_cross_page_writes_keep_invalidation() {
 let mut e=Emu::new();e.register(0x0225F4F8,0x0225F544,false);
 e.jit.main_code_footprint.mark(0x0225F440,4);
 e.jit.invalidate_block(0x0225F441,1);
 assert_eq!(e.jit.jit_memory_map.entry(0x0225F4F8),0);
 e.register(0x0225F4F8,0x0225F544,false);
 e.register(0x0225F480,0x0225F4A0,true);
 e.jit.invalidate_block(0x0225F480,2);
 assert_eq!(e.jit.jit_memory_map.entry(0x0225F4F8),0);
 e.register(0x0225F4F8,0x0225F544,false);
 e.jit.invalidate_blocks(0x0225F400,512); // overlay: never narrowed
 assert_eq!(e.jit.jit_memory_map.entry(0x0225F4F8),0);
 assert_eq!(e.jit.jit_memory_map.entry(0x0225F500),0);
 e.register(0x0225F4F8,0x0225F544,false);
 e.jit.invalidate_block(0x0225F4FF,2);
 assert_eq!(e.jit.jit_memory_map.entry(0x0225F4F8),0);
 assert_eq!(e.jit.jit_memory_map.entry(0x0225F500),0);
}
#[test]
fn stale_dependencies_remain_conservative_after_overlay_and_new_code() {
 let mut e=Emu::new();e.register(0x0225F480,0x0225F4A0,false);
 e.jit.invalidate_blocks(0x0225F400,256);
 e.register(0x0225F4C0,0x0225F4E0,false);
 e.jit.invalidate_block(0x0225F481,1);
 assert_eq!(e.jit.jit_memory_map.entry(0x0225F4C0),0);
}
'''
with tempfile.TemporaryDirectory() as tmp:
 tmp=Path(tmp)
 module='\n#[path = '+repr(str(Path('src/jit/main_code_footprint.rs').resolve())).replace("'",'"')+'] mod main_code_footprint;\n'
 # Host stable lacks this nightly pointer convenience method. The fixture maps
 # every tested address; use the equivalent checked dereference for host tests.
 methods='\n'.join(method(n) for n in ['invalidate_block','invalidate_blocks']).replace('.as_mut_unchecked()', '.as_mut().unwrap()')
 code=prefix+module+'impl JitMemory {\n'+methods+'\n}\nimpl Emu {\n'+method('jit_set_live_range')+'\n}\n'+tests
 p=tmp/'range_tests.rs';p.write_text(code)
 exe=tmp/'range_tests'
 subprocess.run(['rustc','+stable','--edition=2021','--test',str(p),'-o',str(exe)],check=True)
 subprocess.run([str(exe),'--test-threads=1'],cwd=tmp,check=True)
