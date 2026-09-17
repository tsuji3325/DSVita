#!/usr/bin/env python3
from pathlib import Path

p = Path("src/presenter/ui.rs")
s = p.read_text(encoding="utf-8")

def rep(old, new):
    global s
    if old not in s:
        raise SystemExit(f"Japanese UI patch target not found: {old[:80]!r}")
    s = s.replace(old, new)

# Display-only translation layer. Internal setting keys and serialized values stay English.
rep(
    "use crate::presenter::{cjk_font, default_key_binding, show_controls_create_settings, show_layout_create_settings, show_retroachievements_settings, PRESENTER_SCREEN_HEIGHT, PRESENTER_SCREEN_WIDTH};",
    "use crate::presenter::{cjk_font, ja_jp, default_key_binding, show_controls_create_settings, show_layout_create_settings, show_retroachievements_settings, PRESENTER_SCREEN_HEIGHT, PRESENTER_SCREEN_WIDTH};",
)
rep(
    "let label = CString::from_str(group.into()).unwrap();",
    "let group_name: &str = group.into();\n        let label = CString::from_str(ja_jp::setting_group(group_name)).unwrap();",
)
rep(
    "let title = CString::new(setting.title).unwrap();",
    "let title = CString::new(ja_jp::setting_title(setting.title)).unwrap();",
)
rep(
    "let description = CString::new(setting.description).unwrap();",
    "let description = CString::new(ja_jp::setting_description(setting.description)).unwrap();",
)
rep(
    "let value = CString::new(setting.value.to_string()).unwrap();",
    "let bool_value = setting.value.as_bool().unwrap_or(false);\n            let value = CString::new(ja_jp::bool_value(bool_value)).unwrap();",
)
rep(
    "let value = CString::from_str(&inner.values[inner.selection]).unwrap();",
    "let value = CString::from_str(ja_jp::list_value(&inner.values[inner.selection])).unwrap();",
)
rep(
    "let val_cstr = CString::from_str(val).unwrap();",
    "let val_cstr = CString::from_str(ja_jp::list_value(val)).unwrap();",
)

# Common settings/menu labels. IDs after ## are deliberately kept stable.
for old, new in {
    'c"Global settings"': 'c"グローバル設定"',
    'c"Game settings"': 'c"ゲーム設定"',
    'c"Settings"': 'c"設定"',
    'c"Save"': 'c"保存"',
    'c"Back"': 'c"戻る"',
    'c"Resume"': 'c"ゲームに戻る"',
    'c"Quit game"': 'c"ゲームを終了"',
    'c"Quit app"': 'c"DSVitaを終了"',
    'c"Load state"': 'c"ステートをロード"',
    'c"Save state"': 'c"ステートを保存"',
    'c"Cheats"': 'c"チート"',
    'c"Controls"': 'c"操作設定"',
    'c"Screen layout"': 'c"画面レイアウト"',
    'c"Delete"': 'c"削除"',
    'c"Cancel"': 'c"キャンセル"',
    'c"Confirm"': 'c"決定"',
}.items():
    s = s.replace(old, new)

p.write_text(s, encoding="utf-8")
print("Japanese UI patch applied")
