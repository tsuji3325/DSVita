use std::sync::atomic::{AtomicU32, Ordering};

const SLOW_FRAME_US: u32 = 40_000;

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

pub(crate) fn reset() {
    for stat in &STATS {
        stat.store(0, Ordering::Relaxed);
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
    let frame_rom_misses = STATS[ROM_CUR_MISSES].swap(0, Ordering::Relaxed);
    let frame_rom_us = STATS[ROM_CUR_US].swap(0, Ordering::Relaxed);

    add(CPU_FRAME_COUNT, 1);
    add(CPU_FRAME_US, micros);
    update_max(CPU_FRAME_MAX_US, micros);

    if micros >= SLOW_FRAME_US {
        add(CPU_SLOW_COUNT, 1);
        add(CPU_SLOW_US, micros);
        update_max(CPU_SLOW_MAX_US, micros);
        add(CPU_SLOW_ROM_MISSES, frame_rom_misses);
        add(CPU_SLOW_ROM_US, frame_rom_us);
    }
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

pub(crate) fn write_report() {
    let cpu_frames = get(CPU_FRAME_COUNT);
    let cpu_slow = get(CPU_SLOW_COUNT);
    let render_frames = get(RENDER_FRAME_COUNT);
    let ready_slow = get(READY_WAIT_SLOW_COUNT);
    let render_slow = get(RENDER_SLOW_COUNT);

    let report = format!(
        concat!(
            "report_version=1\n",
            "slow_threshold_us={}\n",
            "cpu_frames={} cpu_avg_us={} cpu_max_us={} cpu_slow_frames={} cpu_slow_avg_us={} cpu_slow_max_us={}\n",
            "rom_page_misses={} rom_read_total_us={} rom_read_max_us={} slow_frame_rom_misses={} slow_frame_rom_read_us={}\n",
            "render_frames={} ready_wait_avg_us={} ready_wait_max_us={} ready_wait_slow_frames={} ready_wait_slow_avg_us={} ready_wait_slow_max_us={}\n",
            "render_work_avg_us={} render_work_max_us={} render_slow_frames={} render_slow_avg_us={} render_slow_max_us={}\n",
            "all_vram_prep_us={} all_vram_prep_max_us={} all_vram_2d_us={} all_vram_2d_max_us={} all_vram_3d_us={} all_vram_3d_max_us={}\n",
            "all_draw_2d_us={} all_draw_2d_max_us={} all_wait_3d_us={} all_wait_3d_max_us={} all_render_3d_us={} all_render_3d_max_us={} all_other_render_us={} all_other_render_max_us={}\n",
            "slow_vram_prep_us={} slow_vram_2d_us={} slow_vram_3d_us={} slow_draw_2d_us={} slow_wait_3d_us={} slow_render_3d_us={} slow_other_render_us={}\n"
        ),
        SLOW_FRAME_US,
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
    );

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
