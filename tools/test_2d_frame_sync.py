#!/usr/bin/env python3
"""V24 source-level guardrails; GitHub Actions also cross-compiles the Vita binary."""
from pathlib import Path

settings = Path("src/settings.rs").read_text()
guest = Path("src/core/graphics/gpu.rs").read_text()
renderer = Path("src/core/graphics/gpu_renderer.rs").read_text()
memory = Path("src/core/graphics/gpu_mem_buf.rs").read_text()
report = Path("src/perf_diag.rs").read_text()
ja = Path("src/presenter/ja_jp.rs").read_text()

assert 'SettingId::FrameSync2DDiagnostic => Setting::new(' in settings
assert 'SettingValue::Bool(false)' in settings.split("SettingId::FrameSync2DDiagnostic => Setting::new(", 1)[1].split("SettingGroup::Graphics", 1)[0]
assert 'true,' in settings.split("SettingId::FrameSync2DDiagnostic => Setting::new(", 1)[1].split("SettingGroup::Graphics", 1)[0]
assert 'pub fn frame_sync_2d_diagnostic(&self) -> bool' in settings
assert '"Synchronize 2D frames (diagnostic)" => ' in ja
assert 'self.settings.frame_sync_2d_diagnostic()' in guest
assert 'force_2d_frame_sync,' in guest
assert 'reload_registers(&self.mem.vram, self.settings.frame_sync_2d_diagnostic())' in guest
assert 'fn wait_for_completed_frame(&self, at_reload: bool) -> bool' in renderer
assert 'if force_2d_frame_sync && !self.wait_for_completed_frame(false)' in renderer
assert 'if force_2d_frame_sync && !self.wait_for_completed_frame(true)' in renderer
assert 'render_frame_complete.store(false, Ordering::Release);' in renderer
assert 'render_frame_complete.store(true, Ordering::Release);' in renderer
assert 'render_complete_condvar.notify_all();' in renderer
assert 'Duration::from_millis(1000)' in renderer
assert 'self.vram.maps.read_all_obj_a(' in memory
assert 'self.vram.maps.read_all_obj_b(' in memory
assert 'report_version=24' in report
assert '[2d_frame_sync_diagnostic]' in report
assert 'forced_unsampled_vblanks=' in report
assert 'forced_wait_timeouts=' in report
print("2D frame-sync diagnostic wiring checks passed")
