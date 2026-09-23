use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

const SLOW_FRAME_US: u32 = 40_000;
const PC_BUCKET_SIZE: u32 = 0x40;
const NO_OVERLAY: u32 = u32::MAX;

const CPU_FRAME_COUNT: usize = 0;
const CPU_FRAME_US: usize = 1;
const CPU_FRAME_MAX_US: usize = 2;
const CPU_SLOW_COUNT: usize = 3;
const CPU_SLOW_US: usize = 4;
const CPU_SLOW_MAX_US: usize = 5;

const ROM_MISS_COUNT: usize = 6;
const ROM_READ_US: usize = 7;
const ROM_READ_MAX_US: usize = 8;
const ROM_CUR_MISSES: usize = 9;
const ROM_CUR_US: usize = 10;
const CPU_SLOW_ROM_MISSES: usize = 11;
const CPU_SLOW_ROM_US: usize = 12;

const RENDER_FRAME_COUNT: usize = 13;
const READY_WAIT_US: usize = 14;
const READY_WAIT_MAX_US: usize = 15;
const READY_WAIT_SLOW_COUNT: usize = 16;
const READY_WAIT_SLOW_US: usize = 17;
const READY_WAIT_SLOW_MAX_US: usize = 18;
const RENDER_WORK_US: usize = 19;
const RENDER_WORK_MAX_US: usize = 20;
const RENDER_SLOW_COUNT: usize = 21;
const RENDER_SLOW_US: usize = 22;
const RENDER_SLOW_MAX_US: usize = 23;

const VRAM_PREP_US: usize = 24;
const VRAM_PREP_MAX_US: usize = 25;
const VRAM_2D_US: usize = 26;
const VRAM_2D_MAX_US: usize = 27;
const VRAM_3D_US: usize = 28;
const VRAM_3D_MAX_US: usize = 29;
const DRAW_2D_US: usize = 30;
const DRAW_2D_MAX_US: usize = 31;
const WAIT_3D_US: usize = 32;
const WAIT_3D_MAX_US: usize = 33;
const RENDER_3D_US: usize = 34;
const RENDER_3D_MAX_US: usize = 35;
const OTHER_RENDER_US: usize = 36;
const OTHER_RENDER_MAX_US: usize = 37;

const SLOW_VRAM_PREP_US: usize = 38;
const SLOW_VRAM_2D_US: usize = 39;
const SLOW_VRAM_3D_US: usize = 40;
const SLOW_DRAW_2D_US: usize = 41;
const SLOW_WAIT_3D_US: usize = 42;
const SLOW_RENDER_3D_US: usize = 43;
const SLOW_OTHER_RENDER_US: usize = 44;

const STAT_COUNT: usize = 45;
static STATS: [AtomicU32; STAT_COUNT] = [const { AtomicU32::new(0) }; STAT_COUNT];

// Low six bits hold the execution phase; upper bits hold the 64-byte PC bucket.
// One atomic read gives a coherent PC/phase pair, including across JIT edges.
static CURRENT_ARM9_PC: AtomicU32 = AtomicU32::new(0);
static PRESERVED_MAIN_WRITES: AtomicU32 = AtomicU32::new(0);
pub(crate) const PHASE_JIT: u32 = 1;
pub(crate) const PHASE_INTERPRETER: u32 = 2;
pub(crate) const PHASE_COMPILE: u32 = 3;
pub(crate) const PHASE_DISPATCH: u32 = 4;
pub(crate) const PHASE_HLE: u32 = 5;
pub(crate) const PHASE_CART_CTRL: u32 = 6;
pub(crate) const PHASE_CART_DATA: u32 = 7;
pub(crate) const PHASE_CART_START: u32 = 8;
pub(crate) const PHASE_SCHEDULER: u32 = 9;
const PHASE_NAMES: [&str; 10] = ["inactive", "jit_and_helpers", "interpreter_and_helpers", "compile", "dispatch", "hle_and_helpers", "cart_ctrl", "cart_data", "cart_start", "scheduler"];

pub(crate) const CART_CTRL_READS: usize = 0;
pub(crate) const CART_CTRL_BUSY: usize = 1;
pub(crate) const CART_CTRL_NOT_READY: usize = 2;
pub(crate) const CART_DATA_READS: usize = 3;
pub(crate) const CART_DATA_REJECTED: usize = 4;
pub(crate) const CART_DATA_WORDS: usize = 5;
pub(crate) const CART_TRANSFERS: usize = 6;
pub(crate) const CART_REQUESTED_BYTES: usize = 7;
pub(crate) const CART_COMPLETIONS: usize = 8;
const CART_NAMES: [&str; 9] = ["ctrl_reads", "ctrl_busy_reads", "ctrl_busy_not_ready_reads", "data_reads", "data_rejected_reads", "data_words", "transfer_starts", "requested_bytes", "transfer_completions"];
static CART_FRAME: [AtomicU32; 9] = [const { AtomicU32::new(0) }; 9];
static CART_ALL: [AtomicU32; 9] = [const { AtomicU32::new(0) }; 9];
static CART_SLOW: [AtomicU32; 9] = [const { AtomicU32::new(0) }; 9];

// Only the emulation thread writes these counters; reset precedes thread start
// and reports follow its join. Load/store avoids exclusive RMW per I/O access.
// The sampler never reads the counters. Saturation is explicit in the report.
#[inline]
fn single_writer_add(slot: &AtomicU32, value: u32) {
    slot.store(slot.load(Ordering::Relaxed).saturating_add(value), Ordering::Relaxed);
}

#[inline]
pub(crate) fn record_cart(index: usize, value: u32) {
    single_writer_add(&CART_FRAME[index], value);
}

#[inline]
pub(crate) fn pack_pc_phase(pc: u32, phase: u32) -> u32 {
    (pc & !(PC_BUCKET_SIZE - 1)) | phase
}

#[inline]
pub(crate) fn publish_arm9_state(pc: u32, phase: u32) {
    CURRENT_ARM9_PC.store(pack_pc_phase(pc, phase), Ordering::Relaxed);
}

// Explicit save/restore, never a Drop guard: JIT exits can abandon Rust frames.
#[inline]
pub(crate) fn enter_phase(phase: u32) -> u32 {
    let previous = CURRENT_ARM9_PC.load(Ordering::Relaxed);
    if previous != 0 {
        CURRENT_ARM9_PC.store(pack_pc_phase(previous, phase), Ordering::Relaxed);
    }
    previous
}

#[inline]
pub(crate) fn restore_arm9_state(state: u32) {
    CURRENT_ARM9_PC.store(state, Ordering::Relaxed);
}

// ARM32 generated code writes one naturally aligned word with STR, the same
// operation as an AtomicU32 relaxed store on Vita. This slot has static lifetime;
// the sampler only reads it atomically. No read-modify-write or barrier is needed
// because the PC does not publish any other data.
#[cfg(target_arch = "arm")]
pub(crate) fn arm9_pc_slot() -> *mut u32 {
    CURRENT_ARM9_PC.as_ptr()
}
static LAST_OVERLAY_EVENT_ID: AtomicU32 = AtomicU32::new(NO_OVERLAY);
static CURRENT_FRAME_ID: AtomicU32 = AtomicU32::new(1);
static LAST_COMPLETED_FRAME_ID: AtomicU32 = AtomicU32::new(0);
static LAST_COMPLETED_FRAME_SLOW: AtomicBool = AtomicBool::new(false);

static ALL_PC_SAMPLES: OnceLock<Mutex<BTreeMap<u64, u32>>> = OnceLock::new();
static SLOW_PC_SAMPLES: OnceLock<Mutex<BTreeMap<u64, u32>>> = OnceLock::new();

#[inline]
fn sample_map_all() -> &'static Mutex<BTreeMap<u64, u32>> {
    ALL_PC_SAMPLES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

#[inline]
fn sample_map_slow() -> &'static Mutex<BTreeMap<u64, u32>> {
    SLOW_PC_SAMPLES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

#[inline]
fn add(index: usize, value: u32) {
    STATS[index].fetch_add(value, Ordering::Relaxed);
}

#[inline]
fn update_max(index: usize, value: u32) {
    let mut old = STATS[index].load(Ordering::Relaxed);
    while value > old {
        match STATS[index].compare_exchange_weak(old, value, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => break,
            Err(current) => old = current,
        }
    }
}

#[inline]
fn sample_key(pc: u32, overlay_hint: u32) -> u64 {
    ((overlay_hint as u64) << 32) | pc as u64
}

fn merge_histogram(target: &Mutex<BTreeMap<u64, u32>>, source: &BTreeMap<u64, u32>) {
    let mut target = target.lock().unwrap();
    for (&key, &count) in source {
        *target.entry(key).or_insert(0) += count;
    }
}

pub(crate) fn record_preserved_main_write() { single_writer_add(&PRESERVED_MAIN_WRITES, 1); }

pub(crate) fn reset() {
    PRESERVED_MAIN_WRITES.store(0, Ordering::Relaxed);
    crate::compile_diag::reset();
    crate::write_diag::reset();
    crate::dependency_diag::reset();
    crate::reuse_cache::reset();
    crate::compile_focus::reset();
    for stat in &STATS {
        stat.store(0, Ordering::Relaxed);
    }
    for counters in [&CART_FRAME, &CART_ALL, &CART_SLOW] {
        for counter in counters { counter.store(0, Ordering::Relaxed); }
    }
    CURRENT_ARM9_PC.store(0, Ordering::Relaxed);
    LAST_OVERLAY_EVENT_ID.store(NO_OVERLAY, Ordering::Relaxed);
    CURRENT_FRAME_ID.store(1, Ordering::Relaxed);
    LAST_COMPLETED_FRAME_ID.store(0, Ordering::Relaxed);
    LAST_COMPLETED_FRAME_SLOW.store(false, Ordering::Relaxed);
    sample_map_all().lock().unwrap().clear();
    sample_map_slow().lock().unwrap().clear();
}

#[inline]
pub(crate) fn publish_arm9_pc(pc: u32) {
    publish_arm9_state(pc, PHASE_JIT);
}

#[inline]
pub(crate) fn replace_arm9_pc(pc: u32) -> u32 {
    CURRENT_ARM9_PC.swap(pc, Ordering::Relaxed)
}

#[inline]
pub(crate) fn source_hint() -> u32 { CURRENT_ARM9_PC.load(Ordering::Relaxed) }

pub(crate) fn overlay_hint() -> u32 { LAST_OVERLAY_EVENT_ID.load(Ordering::Relaxed) }

pub(crate) fn set_overlay_event_id(id: u32) {
    LAST_OVERLAY_EVENT_ID.store(id, Ordering::Relaxed);
}

pub(crate) fn run_arm9_sampler(active: Arc<AtomicBool>) {
    let mut frame_id = CURRENT_FRAME_ID.load(Ordering::Acquire);
    let mut frame_samples = BTreeMap::<u64, u32>::new();

    while active.load(Ordering::Relaxed) {
        let observed_frame_id = CURRENT_FRAME_ID.load(Ordering::Acquire);
        if observed_frame_id != frame_id {
            let completed_id = LAST_COMPLETED_FRAME_ID.load(Ordering::Acquire);
            let completed_slow = LAST_COMPLETED_FRAME_SLOW.load(Ordering::Relaxed);
            if completed_id == frame_id && completed_slow {
                merge_histogram(sample_map_slow(), &frame_samples);
            }
            frame_samples.clear();
            frame_id = observed_frame_id;
        }

        let pc = CURRENT_ARM9_PC.load(Ordering::Relaxed);
        if pc != 0 {
            let overlay_hint = LAST_OVERLAY_EVENT_ID.load(Ordering::Relaxed);
            let key = sample_key(pc, overlay_hint);
            *frame_samples.entry(key).or_insert(0) += 1;
            {
                let mut all = sample_map_all().lock().unwrap();
                *all.entry(key).or_insert(0) += 1;
            }
        }

        std::thread::sleep(Duration::from_millis(1));
    }

    let completed_id = LAST_COMPLETED_FRAME_ID.load(Ordering::Acquire);
    let completed_slow = LAST_COMPLETED_FRAME_SLOW.load(Ordering::Relaxed);
    if completed_id == frame_id && completed_slow {
        merge_histogram(sample_map_slow(), &frame_samples);
    }
}

#[inline]
pub(crate) fn record_rom_page_read(micros: u32) {
    add(ROM_MISS_COUNT, 1);
    add(ROM_READ_US, micros);
    update_max(ROM_READ_MAX_US, micros);
    add(ROM_CUR_MISSES, 1);
    add(ROM_CUR_US, micros);
}

#[inline]
pub(crate) fn record_cpu_frame_interval(micros: u32) {
    crate::compile_diag::frame(micros);
    let frame_rom_misses = STATS[ROM_CUR_MISSES].swap(0, Ordering::Relaxed);
    let frame_rom_us = STATS[ROM_CUR_US].swap(0, Ordering::Relaxed);

    for index in 0..CART_NAMES.len() {
        let value = CART_FRAME[index].load(Ordering::Relaxed);
        CART_FRAME[index].store(0, Ordering::Relaxed);
        // ALL includes boot and final partial frame; SLOW includes only complete
        // classified frames, matching the CPU interval definition.
        single_writer_add(&CART_ALL[index], value);
        if micros >= SLOW_FRAME_US { single_writer_add(&CART_SLOW[index], value); }
    }
    if micros == 0 {
        return;
    }

    add(CPU_FRAME_COUNT, 1);
    add(CPU_FRAME_US, micros);
    update_max(CPU_FRAME_MAX_US, micros);

    let slow = micros >= SLOW_FRAME_US;
    if slow {
        add(CPU_SLOW_COUNT, 1);
        add(CPU_SLOW_US, micros);
        update_max(CPU_SLOW_MAX_US, micros);
        add(CPU_SLOW_ROM_MISSES, frame_rom_misses);
        add(CPU_SLOW_ROM_US, frame_rom_us);
    }

    // Publish completion metadata before advancing the frame id. The sampler's
    // Acquire load of CURRENT_FRAME_ID then sees a fully classified prior frame.
    let frame_id = CURRENT_FRAME_ID.load(Ordering::Relaxed);
    LAST_COMPLETED_FRAME_SLOW.store(slow, Ordering::Relaxed);
    LAST_COMPLETED_FRAME_ID.store(frame_id, Ordering::Relaxed);
    CURRENT_FRAME_ID.store(frame_id.wrapping_add(1), Ordering::Release);
}

#[allow(clippy::too_many_arguments)]
#[inline]
pub(crate) fn record_render_frame(
    ready_wait_us: u32,
    render_work_us: u32,
    vram_prep_us: u32,
    vram_2d_us: u32,
    vram_3d_us: u32,
    draw_2d_us: u32,
    wait_3d_us: u32,
    render_3d_us: u32,
    other_render_us: u32,
) {
    add(RENDER_FRAME_COUNT, 1);

    add(READY_WAIT_US, ready_wait_us);
    update_max(READY_WAIT_MAX_US, ready_wait_us);
    if ready_wait_us >= SLOW_FRAME_US {
        add(READY_WAIT_SLOW_COUNT, 1);
        add(READY_WAIT_SLOW_US, ready_wait_us);
        update_max(READY_WAIT_SLOW_MAX_US, ready_wait_us);
    }

    add(RENDER_WORK_US, render_work_us);
    update_max(RENDER_WORK_MAX_US, render_work_us);

    add(VRAM_PREP_US, vram_prep_us);
    update_max(VRAM_PREP_MAX_US, vram_prep_us);
    add(VRAM_2D_US, vram_2d_us);
    update_max(VRAM_2D_MAX_US, vram_2d_us);
    add(VRAM_3D_US, vram_3d_us);
    update_max(VRAM_3D_MAX_US, vram_3d_us);
    add(DRAW_2D_US, draw_2d_us);
    update_max(DRAW_2D_MAX_US, draw_2d_us);
    add(WAIT_3D_US, wait_3d_us);
    update_max(WAIT_3D_MAX_US, wait_3d_us);
    add(RENDER_3D_US, render_3d_us);
    update_max(RENDER_3D_MAX_US, render_3d_us);
    add(OTHER_RENDER_US, other_render_us);
    update_max(OTHER_RENDER_MAX_US, other_render_us);

    if render_work_us >= SLOW_FRAME_US {
        add(RENDER_SLOW_COUNT, 1);
        add(RENDER_SLOW_US, render_work_us);
        update_max(RENDER_SLOW_MAX_US, render_work_us);
        add(SLOW_VRAM_PREP_US, vram_prep_us);
        add(SLOW_VRAM_2D_US, vram_2d_us);
        add(SLOW_VRAM_3D_US, vram_3d_us);
        add(SLOW_DRAW_2D_US, draw_2d_us);
        add(SLOW_WAIT_3D_US, wait_3d_us);
        add(SLOW_RENDER_3D_US, render_3d_us);
        add(SLOW_OTHER_RENDER_US, other_render_us);
    }
}

#[inline]
fn get(index: usize) -> u32 {
    STATS[index].load(Ordering::Relaxed)
}

fn avg(total: u32, count: u32) -> u32 {
    if count == 0 { 0 } else { total / count }
}

fn top_samples(map: &Mutex<BTreeMap<u64, u32>>, limit: usize) -> (u32, String) {
    let map = map.lock().unwrap();
    let mut buckets = BTreeMap::<u64, u32>::new();
    for (&key, &count) in map.iter() {
        *buckets.entry(key & !63).or_insert(0) += count;
    }
    let total = buckets.values().copied().sum::<u32>();
    let mut entries = buckets.into_iter().collect::<Vec<_>>();
    entries.sort_unstable_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let mut out = String::new();
    for (rank, (key, count)) in entries.into_iter().take(limit).enumerate() {
        let overlay = (key >> 32) as u32;
        let pc = key as u32;
        let overlay_text = if overlay == NO_OVERLAY { "none".to_string() } else { overlay.to_string() };
        let per_mille = if total == 0 { 0 } else { count.saturating_mul(1000) / total };
        out.push_str(&format!(
            "{} overlay_hint={} pc_bucket=0x{:08X}-0x{:08X} samples={} permille={}\n",
            rank + 1,
            overlay_text,
            pc,
            pc + PC_BUCKET_SIZE - 1,
            count,
            per_mille,
        ));
    }
    (total, out)
}

fn append_phase_report(report: &mut String, label: &str, map: &Mutex<BTreeMap<u64, u32>>) {
    let map = map.lock().unwrap();
    let mut totals = [0u64; 10];
    for (&key, &count) in map.iter() {
        let phase = (key & 63) as usize;
        if phase < totals.len() { totals[phase] += count as u64; }
    }
    report.push_str(&format!("[{}_phase_samples]\n", label));
    for (phase, &samples) in totals.iter().enumerate().skip(1) {
        report.push_str(&format!("phase={} samples={}\n", PHASE_NAMES[phase], samples));
    }
    report.push_str(&format!("[top_{}_phase_pc_buckets]\n", label));
    let mut entries = map.iter().map(|(&key, &count)| (key, count)).collect::<Vec<_>>();
    entries.sort_unstable_by(|a,b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    for (key, count) in entries.into_iter().take(32) {
        let phase = (key & 63) as usize;
        let pc = key as u32 & !63;
        let overlay = (key >> 32) as u32;
        let hint = if overlay == NO_OVERLAY { "none".to_owned() } else { overlay.to_string() };
        report.push_str(&format!("phase={} overlay_hint={} pc_bucket=0x{:08X}-0x{:08X} samples={}\n", PHASE_NAMES.get(phase).unwrap_or(&"unknown"), hint, pc, pc+63, count));
    }
}

pub(crate) fn write_report() {
    let cpu_frames = get(CPU_FRAME_COUNT);
    let cpu_slow = get(CPU_SLOW_COUNT);
    let render_frames = get(RENDER_FRAME_COUNT);
    let ready_slow = get(READY_WAIT_SLOW_COUNT);
    let render_slow = get(RENDER_SLOW_COUNT);
    let (all_pc_samples, all_pc_top) = top_samples(sample_map_all(), 24);
    let (slow_pc_samples, slow_pc_top) = top_samples(sample_map_slow(), 24);

    let mut report = format!(
        concat!(
            "report_version=18\n",
            "pc_source=arm9_jit_block_boundary_and_interpreter_entry pc_note=last_guest_boundary_includes_host_helpers_not_instruction_exact\n",
            "slow_threshold_us={} pc_sample_interval_ms=1 pc_bucket_size={} overlay_hint_note=last_FS_ClearOverlayImage_event_not_ownership\n",
            "cpu_frames={} cpu_avg_us={} cpu_max_us={} cpu_slow_frames={} cpu_slow_avg_us={} cpu_slow_max_us={}\n",
            "rom_page_misses={} rom_read_total_us={} rom_read_max_us={} slow_frame_rom_misses={} slow_frame_rom_read_us={}\n",
            "render_frames={} ready_wait_avg_us={} ready_wait_max_us={} ready_wait_slow_frames={} ready_wait_slow_avg_us={} ready_wait_slow_max_us={}\n",
            "render_work_avg_us={} render_work_max_us={} render_slow_frames={} render_slow_avg_us={} render_slow_max_us={}\n",
            "all_vram_prep_us={} all_vram_prep_max_us={} all_vram_2d_us={} all_vram_2d_max_us={} all_vram_3d_us={} all_vram_3d_max_us={}\n",
            "all_draw_2d_us={} all_draw_2d_max_us={} all_wait_3d_us={} all_wait_3d_max_us={} all_render_3d_us={} all_render_3d_max_us={} all_other_render_us={} all_other_render_max_us={}\n",
            "slow_vram_prep_us={} slow_vram_2d_us={} slow_vram_3d_us={} slow_draw_2d_us={} slow_wait_3d_us={} slow_render_3d_us={} slow_other_render_us={}\n",
            "all_pc_samples={} slow_pc_samples={}\n",
            "[top_slow_pc_buckets]\n"
        ),
        SLOW_FRAME_US,
        PC_BUCKET_SIZE,
        cpu_frames,
        avg(get(CPU_FRAME_US), cpu_frames),
        get(CPU_FRAME_MAX_US),
        cpu_slow,
        avg(get(CPU_SLOW_US), cpu_slow),
        get(CPU_SLOW_MAX_US),
        get(ROM_MISS_COUNT),
        get(ROM_READ_US),
        get(ROM_READ_MAX_US),
        get(CPU_SLOW_ROM_MISSES),
        get(CPU_SLOW_ROM_US),
        render_frames,
        avg(get(READY_WAIT_US), render_frames),
        get(READY_WAIT_MAX_US),
        ready_slow,
        avg(get(READY_WAIT_SLOW_US), ready_slow),
        get(READY_WAIT_SLOW_MAX_US),
        avg(get(RENDER_WORK_US), render_frames),
        get(RENDER_WORK_MAX_US),
        render_slow,
        avg(get(RENDER_SLOW_US), render_slow),
        get(RENDER_SLOW_MAX_US),
        get(VRAM_PREP_US),
        get(VRAM_PREP_MAX_US),
        get(VRAM_2D_US),
        get(VRAM_2D_MAX_US),
        get(VRAM_3D_US),
        get(VRAM_3D_MAX_US),
        get(DRAW_2D_US),
        get(DRAW_2D_MAX_US),
        get(WAIT_3D_US),
        get(WAIT_3D_MAX_US),
        get(RENDER_3D_US),
        get(RENDER_3D_MAX_US),
        get(OTHER_RENDER_US),
        get(OTHER_RENDER_MAX_US),
        get(SLOW_VRAM_PREP_US),
        get(SLOW_VRAM_2D_US),
        get(SLOW_VRAM_3D_US),
        get(SLOW_DRAW_2D_US),
        get(SLOW_WAIT_3D_US),
        get(SLOW_RENDER_3D_US),
        get(SLOW_OTHER_RENDER_US),
        all_pc_samples,
        slow_pc_samples,
    );

    report.push_str(&slow_pc_top);
    report.push_str("[top_all_pc_buckets]\n");
    report.push_str(&all_pc_top);
    report.push_str("phase_note=coherent_pc_bucket_and_phase_jit_interpreter_hle_include_unmarked_helpers_arm32_boundary_tracking\n");
    append_phase_report(&mut report, "all", sample_map_all());
    append_phase_report(&mut report, "slow", sample_map_slow());
    report.push_str("[arm9_cart_counters]\nscope=arm9_bus_including_dma all_includes_boot_and_partial_frame slow_complete_frames_only counters_saturate_at_u32_max\n");
    for index in 0..CART_NAMES.len() {
        let all = CART_ALL[index].load(Ordering::Relaxed).saturating_add(CART_FRAME[index].load(Ordering::Relaxed));
        let slow = CART_SLOW[index].load(Ordering::Relaxed);
        report.push_str(&format!("{} all={} slow={}\n", CART_NAMES[index], all, slow));
    }

    crate::compile_diag::append_report(&mut report);
    crate::reuse_cache::append_report(&mut report);
    crate::compile_focus::append_report(&mut report);
    report.push_str(&format!("[jit_write_preserve]\npreserved_live_page_writes={} policy=single_page_main_RAM_disjoint_from_session_code_and_folded_data footprint_bytes=524288 stale_dependencies_retained=true\n", PRESERVED_MAIN_WRITES.load(Ordering::Relaxed)));
    #[cfg(target_os = "vita")]
    {
        let _ = std::fs::create_dir_all("ux0:data/dsvita");
        let _ = std::fs::write("ux0:data/dsvita/frame_perf.log", report);
    }

    #[cfg(not(target_os = "vita"))]
    {
        let _ = std::fs::write("frame_perf.log", report);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn wait_until(mut predicate: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !predicate() {
            assert!(Instant::now() < deadline, "sampler timed out");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn packed_phase_nesting_and_counter_frame_accounting() {
        reset();
        publish_arm9_state(0x020DCE55, PHASE_INTERPRETER);
        let original = pack_pc_phase(0x020DCE55, PHASE_INTERPRETER);
        let saved = enter_phase(PHASE_CART_DATA);
        assert_eq!(saved, original);
        assert_eq!(CURRENT_ARM9_PC.load(Ordering::Relaxed), pack_pc_phase(0x020DCE55, PHASE_CART_DATA));
        let nested = enter_phase(PHASE_SCHEDULER);
        restore_arm9_state(nested);
        restore_arm9_state(saved);
        assert_eq!(CURRENT_ARM9_PC.load(Ordering::Relaxed), original);
        let suspended = replace_arm9_pc(0);
        assert_eq!(enter_phase(PHASE_CART_DATA), 0);
        assert_eq!(CURRENT_ARM9_PC.load(Ordering::Relaxed), 0);
        restore_arm9_state(suspended);
        record_cart(CART_CTRL_READS, 7);
        record_cpu_frame_interval(0); // boot is ALL, never SLOW
        record_cart(CART_CTRL_READS, 11);
        record_cpu_frame_interval(39_999);
        record_cart(CART_CTRL_READS, 13);
        record_cpu_frame_interval(40_000);
        record_cart(CART_CTRL_READS, 5); // final partial frame
        assert_eq!(CART_ALL[CART_CTRL_READS].load(Ordering::Relaxed), 31);
        assert_eq!(CART_SLOW[CART_CTRL_READS].load(Ordering::Relaxed), 13);
        assert_eq!(CART_FRAME[CART_CTRL_READS].load(Ordering::Relaxed), 5);
        record_cart(CART_REQUESTED_BYTES, u32::MAX);
        record_cart(CART_REQUESTED_BYTES, 10);
        assert_eq!(CART_FRAME[CART_REQUESTED_BYTES].load(Ordering::Relaxed), u32::MAX);
        reset();
        assert_eq!(CART_ALL[CART_CTRL_READS].load(Ordering::Relaxed), 0);
        assert_eq!(CART_FRAME[CART_REQUESTED_BYTES].load(Ordering::Relaxed), 0);
    }

    #[test]
    fn samples_changing_pcs_classifies_slow_frames_and_writes_only_at_exit() {
        reset();
        let _ = std::fs::remove_file("frame_perf.log");
        let active = Arc::new(AtomicBool::new(true));
        let sampler_active = active.clone();
        let sampler = std::thread::spawn(move || run_arm9_sampler(sampler_active));
        let first = 0x020DD181;
        let second = 0x021E5180;
        let first_key = sample_key(pack_pc_phase(first, PHASE_JIT), 3);
        let second_key = sample_key(pack_pc_phase(second, PHASE_INTERPRETER), 3);
        set_overlay_event_id(3);
        publish_arm9_pc(first);
        wait_until(|| sample_map_all().lock().unwrap().get(&first_key).copied().unwrap_or(0) >= 5);
        publish_arm9_state(second, PHASE_INTERPRETER);
        wait_until(|| sample_map_all().lock().unwrap().get(&second_key).copied().unwrap_or(0) >= 5);
        let saved = replace_arm9_pc(0);
        assert_eq!(saved, pack_pc_phase(second, PHASE_INTERPRETER));
        assert_eq!(CURRENT_ARM9_PC.load(Ordering::Relaxed), 0);
        restore_arm9_state(saved);
        record_cpu_frame_interval(60_000);
        wait_until(|| sample_map_slow().lock().unwrap().contains_key(&second_key));
        publish_arm9_pc(0x02000E00);
        let normal_key = sample_key(pack_pc_phase(0x02000E00, PHASE_JIT), 3);
        wait_until(|| sample_map_all().lock().unwrap().get(&normal_key).copied().unwrap_or(0) >= 5);
        record_cpu_frame_interval(30_000);
        active.store(false, Ordering::Relaxed);
        sampler.join().unwrap();
        assert!(sample_map_slow().lock().unwrap().contains_key(&first_key));
        assert!(!sample_map_slow().lock().unwrap().contains_key(&normal_key));
        assert!(!std::path::Path::new("frame_perf.log").exists());
        write_report();
        let report = std::fs::read_to_string("frame_perf.log").unwrap();
        assert!(report.contains("report_version=18"));
        assert!(report.contains("cpu_slow_frames=1"));
        assert!(report.contains("[top_slow_pc_buckets]"));
        assert!(report.contains("[slow_phase_samples]"));
        assert!(report.contains("phase=interpreter_and_helpers overlay_hint=3 pc_bucket=0x021E5180"));
        assert!(report.contains("[arm9_cart_counters]"));
        assert!(report.contains("0x020DD180-0x020DD1BF"));
        reset();
        assert!(sample_map_all().lock().unwrap().is_empty());
        assert!(sample_map_slow().lock().unwrap().is_empty());
    }
}
