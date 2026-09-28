//! ROM-independent diagnostic profiler for repeated ARM9 JIT blocks.
//! It never enables reuse and never performs guest-memory reads. Folded values are
//! supplied only by the emitter after it has already read them for code generation.
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::sync::atomic::{AtomicU32, Ordering::Relaxed};

use crate::reuse_cache::Key;

const MAX_TRACKED: usize = 1024;
const MAX_REPORT: usize = 64;
const MAX_INSTS: usize = 512;
const MAX_DEPS: usize = 64;
const NO_PENDING: u32 = u32::MAX;
const FNV_OFFSET: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x100000001b3;

static PENDING_TAG: AtomicU32 = AtomicU32::new(NO_PENDING);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Identity {
    end: u32,
    context: [u32; 2],
    inst_hash: u64,
    dep_hash: u64,
    inst_count: u16,
    dep_count: u16,
}

#[derive(Default)]
struct Entry {
    observations: u32,
    identical_repeats: u32,
    changed: u32,
    current_run: u32,
    max_run: u32,
    actual_compiles: u32,
    reuse_probes: u32,
    actual_compile_us: u64,
    last: Option<Identity>,
}

struct Pending {
    tag: u32,
    identity: Identity,
    expected_deps: u16,
    dep_hash: u64,
    dep_count: u16,
}

#[derive(Default)]
struct State {
    entries: BTreeMap<u32, Entry>,
    pending: Option<Pending>,
    dropped_new_pcs: u32,
    unsupported_guard: u32,
    unsupported_shape: u32,
    incomplete_dependencies: u32,
}

fn state() -> &'static Mutex<State> {
    static STATE: OnceLock<Mutex<State>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(State::default()))
}

#[inline]
fn mix_byte(hash: u64, value: u8) -> u64 {
    (hash ^ value as u64).wrapping_mul(FNV_PRIME)
}

fn mix_u16(mut hash: u64, value: u16) -> u64 {
    for b in value.to_le_bytes() { hash = mix_byte(hash, b); }
    hash
}

fn mix_u32(mut hash: u64, value: u32) -> u64 {
    for b in value.to_le_bytes() { hash = mix_byte(hash, b); }
    hash
}

fn instruction_fingerprint(
    pc: u32,
    end: u32,
    thumb: bool,
    context: [u32; 2],
    instructions: impl Iterator<Item = (u32, u16)>,
) -> Option<(u64, u16)> {
    let mut hash = FNV_OFFSET;
    hash = mix_u32(hash, pc);
    hash = mix_u32(hash, end);
    hash = mix_byte(hash, thumb as u8);
    hash = mix_u32(hash, context[0]);
    hash = mix_u32(hash, context[1]);

    let mut count = 0usize;
    for (opcode, cycles) in instructions {
        if count >= MAX_INSTS { return None; }
        hash = mix_u32(hash, opcode);
        hash = mix_u16(hash, cycles);
        count += 1;
    }
    if count == 0 { return None; }
    Some((hash, count as u16))
}

fn dependency_fingerprint(dependencies: &[(u32, u32, u32)]) -> Option<(u64, u16)> {
    if dependencies.len() > MAX_DEPS { return None; }
    let mut hash = FNV_OFFSET;
    for &(pc, addr, value) in dependencies {
        hash = mix_u32(hash, pc);
        hash = mix_u32(hash, addr);
        hash = mix_u32(hash, value);
    }
    Some((hash, dependencies.len() as u16))
}

fn observe(s: &mut State, tag: u32, identity: Identity, actual_compile: bool, compile_us: u64) {
    if !s.entries.contains_key(&tag) && s.entries.len() >= MAX_TRACKED {
        s.dropped_new_pcs = s.dropped_new_pcs.saturating_add(1);
        return;
    }

    let entry = s.entries.entry(tag).or_default();
    entry.observations = entry.observations.saturating_add(1);
    if actual_compile {
        entry.actual_compiles = entry.actual_compiles.saturating_add(1);
        entry.actual_compile_us = entry.actual_compile_us.saturating_add(compile_us);
    } else {
        entry.reuse_probes = entry.reuse_probes.saturating_add(1);
    }

    match entry.last {
        Some(previous) if previous == identity => {
            entry.identical_repeats = entry.identical_repeats.saturating_add(1);
            entry.current_run = entry.current_run.saturating_add(1);
        }
        Some(_) => {
            entry.changed = entry.changed.saturating_add(1);
            entry.current_run = 1;
        }
        None => {
            entry.current_run = 1;
        }
    }
    entry.max_run = entry.max_run.max(entry.current_run);
    entry.last = Some(identity);
}

pub(crate) fn reset() {
    PENDING_TAG.store(NO_PENDING, Relaxed);
    *state().lock().unwrap() = State::default();
}

/// Count a block that cannot enter the current conservative generic policy because
/// of lifecycle/HLE conditions. No per-PC allocation is performed for rejects.
pub(crate) fn unsupported_guard() {
    let mut s = state().lock().unwrap();
    s.unsupported_guard = s.unsupported_guard.saturating_add(1);
}

/// Count a decoded block whose immediate-memory shape is not supported by the
/// current conservative generic policy.
pub(crate) fn unsupported_shape() {
    let mut s = state().lock().unwrap();
    s.unsupported_shape = s.unsupported_shape.saturating_add(1);
}

/// Observe a key already constructed by the existing fixed-target reuse path.
/// This lets v23 check whether the generic profiler rediscovers the known-good
/// HeartGold targets without changing their runtime behavior.
pub(crate) fn observe_existing_key(key: &Key) {
    let Some((inst_hash, inst_count)) = instruction_fingerprint(
        key.pc,
        key.end,
        key.thumb,
        key.context,
        key.instructions.iter().copied(),
    ) else {
        unsupported_shape();
        return;
    };
    let Some((dep_hash, dep_count)) = dependency_fingerprint(&key.dependencies) else {
        unsupported_shape();
        return;
    };

    let identity = Identity {
        end: key.end,
        context: key.context,
        inst_hash,
        dep_hash,
        inst_count,
        dep_count,
    };
    let tag = key.pc | key.thumb as u32;
    observe(&mut state().lock().unwrap(), tag, identity, false, 0);
}

/// Begin profiling a real compile. The caller has already analyzed whether every
/// immediate memory access fits the same conservative rules planned for generic
/// reuse. No guest data is read here.
pub(crate) fn begin_compile(
    pc: u32,
    end: u32,
    thumb: bool,
    context: [u32; 2],
    instructions: impl Iterator<Item = (u32, u16)>,
    expected_deps: usize,
) -> bool {
    if expected_deps > MAX_DEPS {
        unsupported_shape();
        return false;
    }
    let Some((inst_hash, inst_count)) = instruction_fingerprint(pc, end, thumb, context, instructions) else {
        unsupported_shape();
        return false;
    };

    let tag = pc | thumb as u32;
    let pending = Pending {
        tag,
        identity: Identity {
            end,
            context,
            inst_hash,
            dep_hash: FNV_OFFSET,
            inst_count,
            dep_count: 0,
        },
        expected_deps: expected_deps as u16,
        dep_hash: FNV_OFFSET,
        dep_count: 0,
    };

    let mut s = state().lock().unwrap();
    s.pending = Some(pending);
    PENDING_TAG.store(tag, Relaxed);
    true
}

/// Receive a folded value that the ARM9 emitter already read for normal code
/// generation. The atomic fast path avoids taking the profiler lock for blocks
/// that are not currently being profiled.
pub(crate) fn folded(owner: u32, thumb: bool, pc: u32, addr: u32, value: u32) {
    let tag = owner | thumb as u32;
    if PENDING_TAG.load(Relaxed) != tag { return; }

    let mut s = state().lock().unwrap();
    let Some(p) = &mut s.pending else { return; };
    if p.tag != tag { return; }
    if p.dep_count as usize >= MAX_DEPS {
        p.dep_count = u16::MAX;
        return;
    }
    p.dep_hash = mix_u32(p.dep_hash, pc);
    p.dep_hash = mix_u32(p.dep_hash, addr);
    p.dep_hash = mix_u32(p.dep_hash, value);
    p.dep_count = p.dep_count.saturating_add(1);
}

pub(crate) fn finish_compile(pc: u32, thumb: bool, compile_us: u64) {
    let tag = pc | thumb as u32;
    PENDING_TAG.store(NO_PENDING, Relaxed);

    let mut s = state().lock().unwrap();
    let Some(mut pending) = s.pending.take() else { return; };
    if pending.tag != tag || pending.dep_count != pending.expected_deps {
        s.incomplete_dependencies = s.incomplete_dependencies.saturating_add(1);
        return;
    }

    pending.identity.dep_hash = pending.dep_hash;
    pending.identity.dep_count = pending.dep_count;
    observe(&mut s, tag, pending.identity, true, compile_us);
}

pub(crate) fn append_report(out: &mut String) {
    let s = state().lock().unwrap();
    let candidate_count = s.entries.values().filter(|e| e.current_run >= 3).count();
    out.push_str("[generic_reuse_candidates]\n");
    out.push_str("mode=diagnostic_only auto_reuse=false rom_specific_pc_list=false extra_guest_reads=false fingerprint=FNV64_opcode_cycles_context_plus_emitter_observed_folded_values current_policy=ARM_no_immediate_memory_Thumb_only_foldable_u32_main_RAM_literals\n");
    out.push_str("candidate_rule=current_identical_run>=3 note=fingerprint_is_candidate_discovery_only_final_reuse_must_keep_exact_key_and_native_identity_checks\n");
    out.push_str(&format!(
        "tracked_pcs={} candidate_pcs={} max_tracked={} max_report={} dropped_new_pcs={} unsupported_guard={} unsupported_shape={} incomplete_dependencies={}\n",
        s.entries.len(), candidate_count, MAX_TRACKED, MAX_REPORT, s.dropped_new_pcs,
        s.unsupported_guard, s.unsupported_shape, s.incomplete_dependencies
    ));

    let mut rows: Vec<_> = s.entries.iter().filter(|(_, e)| e.observations >= 2).collect();
    rows.sort_by_key(|(tag, e)| (
        std::cmp::Reverse(e.current_run),
        std::cmp::Reverse(e.max_run),
        std::cmp::Reverse(e.observations),
        **tag,
    ));
    for (tag, e) in rows.into_iter().take(MAX_REPORT) {
        let last = e.last.unwrap();
        out.push_str(&format!(
            "pc=0x{:08X} thumb={} candidate={} observations={} identical_repeats={} changed={} current_run={} max_run={} actual_compiles={} reuse_probes={} actual_compile_us={} last_end=0x{:08X} inst_count={} dep_count={} context_arm7={} context_fs_clear=0x{:08X}\n",
            tag & !1, tag & 1, (e.current_run >= 3) as u8, e.observations,
            e.identical_repeats, e.changed, e.current_run, e.max_run,
            e.actual_compiles, e.reuse_probes, e.actual_compile_us, last.end,
            last.inst_count, last.dep_count, last.context[0], last.context[1]
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(pc: u32, dep: u32) -> Key {
        Key {
            pc,
            end: pc + 4,
            thumb: true,
            context: [0, 0x02001000],
            instructions: vec![(0x4800, 1), (0x4770, 2)],
            dependencies: vec![(pc, 0x02010000, dep)],
        }
    }

    #[test]
    fn stable_existing_keys_become_candidates_without_enabling_reuse() {
        reset();
        observe_existing_key(&key(0x02100000, 7));
        observe_existing_key(&key(0x02100000, 7));
        observe_existing_key(&key(0x02100000, 7));
        let s = state().lock().unwrap();
        let e = &s.entries[&0x02100001];
        assert_eq!(e.current_run, 3);
        assert_eq!(e.identical_repeats, 2);
        assert_eq!(e.changed, 0);
        assert_eq!(e.reuse_probes, 3);
    }

    #[test]
    fn dependency_change_breaks_the_stable_run() {
        reset();
        observe_existing_key(&key(0x02100000, 7));
        observe_existing_key(&key(0x02100000, 7));
        observe_existing_key(&key(0x02100000, 8));
        let s = state().lock().unwrap();
        let e = &s.entries[&0x02100001];
        assert_eq!(e.changed, 1);
        assert_eq!(e.current_run, 1);
        assert_eq!(e.max_run, 2);
    }

    #[test]
    fn emitter_dependencies_finalize_a_real_compile_without_guest_reads() {
        reset();
        assert!(begin_compile(
            0x02102000,
            0x02102004,
            true,
            [1, 2],
            [(0x4800, 1), (0x4770, 2)].into_iter(),
            1,
        ));
        folded(0x02102000, true, 0x02102000, 0x02010000, 0x12345678);
        finish_compile(0x02102000, true, 55);
        let s = state().lock().unwrap();
        let e = &s.entries[&0x02102001];
        assert_eq!(e.actual_compiles, 1);
        assert_eq!(e.actual_compile_us, 55);
        assert_eq!(e.last.unwrap().dep_count, 1);
        assert_eq!(s.incomplete_dependencies, 0);
    }

    #[test]
    fn missing_folded_dependency_is_rejected_conservatively() {
        reset();
        assert!(begin_compile(
            0x02103000,
            0x02103004,
            true,
            [1, 2],
            [(0x4800, 1), (0x4770, 2)].into_iter(),
            1,
        ));
        finish_compile(0x02103000, true, 10);
        let s = state().lock().unwrap();
        assert!(s.entries.is_empty());
        assert_eq!(s.incomplete_dependencies, 1);
    }
}
