//! Japanese UI strings for DSVita.
//!
//! Keep internal setting keys/serialized values in English.  This module is
//! intentionally display-only so existing settings.ini files remain compatible.

pub fn setting_group(name: &str) -> &str {
    match name {
        "Emulation" => "エミュレーション",
        "Graphics" => "グラフィック",
        "Screen" => "画面",
        "System" => "システム",
        _ => name,
    }
}

pub fn setting_title(name: &str) -> &str {
    match name {
        "Arm7 Emulation" => "ARM7エミュレーション",
        "HLE OS irq handler" => "HLE OS割り込み処理",
        "Framelimit" => "速度制限",
        "Audio" => "音声",
        "Audio stretching" => "音声ストレッチ",
        "Geometry 3D frameskip" => "3Dジオメトリ フレームスキップ",
        "Upscale 3D factor" => "3D内部解像度",
        "Screen Layout" => "画面レイアウト",
        "Wide 3D screen" => "3Dワイドスクリーン",
        "Swap screens" => "上下画面を入れ替える",
        "Top screen scale" => "上画面のサイズ",
        "Bottom screen scale" => "下画面のサイズ",
        "Tap corner to swap screens" => "画面端タップで上下画面を入れ替える",
        "Stream top screen" => "上画面をストリーミング",
        "Language" => "ゲーム内言語",
        "Controls" => "操作設定",
        "Joystick as D-Pad" => "左スティックを十字キーとして使用",
        "Right stick function" => "右スティック機能",
        "Touch camera sensitivity" => "タッチカメラ感度",
        "Touch camera pivot X" => "タッチカメラ中心 X",
        "Touch camera pivot Y" => "タッチカメラ中心 Y",
        "Rear touch as touchscreen" => "背面タッチをDSタッチ画面として使用",
        "Show debug statistics" => "デバッグ情報を表示",
        "Retroachievements" => "RetroAchievements",
        _ => name,
    }
}

pub fn setting_description(text: &str) -> &str {
    match text {
        "How the ARM7 co-processor is emulated. AccurateLle is slowest but most compatible. SoundHle is ~10%% faster and Hle ~15-20%% faster, but both reduce compatibility. Use AccurateLle if a game crashes, freezes or misbehaves." => "ARM7コプロセッサのエミュレーション方式です。AccurateLleは最も低速ですが互換性が高い方式です。SoundHleは約10%、Hleは約15～20%高速ですが、互換性が低下します。ゲームがクラッシュ、フリーズ、または正常に動作しない場合はAccurateLleを使用してください。",
        "Emulates the system interrupt handler at a higher level for extra speed. May cause crashes in some games." => "システムの割り込み処理を高レベルでエミュレートして高速化します。一部のゲームではクラッシュの原因になる場合があります。",
        "Caps the emulation speed relative to real hardware. Set to 'off' to run as fast as possible." => "実機を基準にエミュレーション速度を制限します。可能な限り高速に動作させる場合は「off」にします。",
        "Turn audio off for a small performance boost." => "音声を無効にすると、わずかに動作が軽くなる場合があります。",
        "Stretches audio to prevent crackling when a game runs below full speed. Adds a little latency." => "処理速度が不足した際に音声を伸縮して音割れを抑えます。わずかに遅延が増えます。",
        "Skips redundant 3D frames for better performance at the cost of some latency. Turn off if a game has 3D glitches or renders 3D on both screens." => "不要な3Dフレームを省略して動作を軽くします。多少の遅延が増えます。3D表示が崩れるゲームや上下両画面に3Dを描画するゲームでは無効にしてください。",
        "Renders 3D graphics at a higher internal resolution. Higher values look sharper but run slower." => "3Dグラフィックをより高い内部解像度で描画します。値を上げるほど鮮明になりますが、処理は重くなります。",
        "How the two screens are arranged on the display. In-game: PS + L or PS + R cycles through layouts." => "DSの2画面をVita上にどのように配置するか設定します。ゲーム中はPS + LまたはPS + Rでレイアウトを切り替えられます。",
        "Experimental widescreen hack for 3D. Can cause glitches. Only available with the single, focus-overlap or custom layouts." => "3D表示をワイド化する実験的な機能です。表示が崩れる場合があります。対応する画面レイアウトでのみ利用できます。",
        "Swaps the top and bottom screens. In-game: PS + Cross." => "DSの上画面と下画面を入れ替えます。ゲーム中はPS + ×で切り替えられます。",
        "Size of the top screen. In-game: PS + Square cycles sizes." => "上画面の表示サイズです。ゲーム中はPS + □でサイズを切り替えられます。",
        "Size of the bottom screen. In-game: PS + Circle cycles sizes." => "下画面の表示サイズです。ゲーム中はPS + ○でサイズを切り替えられます。",
        "Tap the bottom-right corner of the screen to swap the large and small screens (same as PS + Cross)." => "画面右下をタップすると、大画面と小画面を入れ替えます（PS + ×と同じ動作です）。",
        "Preferred in-game language. Only applies if the game actually includes it." => "ゲーム内で優先する言語です。その言語を収録しているゲームでのみ有効です。",
        "Custom button mapping to use. Create profiles under Global settings." => "使用するボタン配置を選択します。プロファイルはグローバル設定から作成できます。",
        "Use the left analog stick as the D-Pad." => "左アナログスティックをDSの十字キーとして使用します。",
        "How fast the touch drag swipes at full stick deflection, in percent. Higher values turn the camera faster." => "右スティックを最大まで倒したときのタッチドラッグ速度を%で設定します。値を上げるほどカメラ操作が速くなります。",
        "Horizontal center of the right stick touch drag, in DS touchscreen pixels." => "右スティックによるタッチドラッグの横方向の中心位置をDSタッチ画面のピクセル単位で設定します。",
        "Vertical center of the right stick touch drag, in DS touchscreen pixels." => "右スティックによるタッチドラッグの縦方向の中心位置をDSタッチ画面のピクセル単位で設定します。",
        "Show FPS and other debug information while playing." => "ゲーム中にFPSなどのデバッグ情報を表示します。",
        "Enables RetroAchievements. Log in first via Global settings." => "RetroAchievementsを有効にします。先にグローバル設定からログインしてください。",
        _ => text,
    }
}

pub fn bool_value(value: bool) -> &'static str {
    if value { "オン" } else { "オフ" }
}

pub fn list_value(value: &str) -> &str {
    match value {
        "off" | "Off" => "オフ",
        "Touch camera" => "タッチカメラ",
        "L and R triggers" => "L/Rトリガー",
        "Japanese" => "日本語",
        "English" => "英語",
        "French" => "フランス語",
        "German" => "ドイツ語",
        "Italian" => "イタリア語",
        "Spanish" => "スペイン語",
        _ => value,
    }
}



/// Merge only the Japanese UI glyphs, before the first atlas build.
/// The embedded font and ranges have static lifetime; no user font is required.
pub unsafe fn init_font() {
    use crate::presenter::imgui::root::{ImFontAtlas_AddFontFromMemoryTTF, ImFontConfig, ImFontConfig_ImFontConfig, ImGui};
    static RANGES: &[u16] = &[9633, 9633, 9675, 9675, 12289, 12289, 12290, 12290, 12300, 12300, 12301, 12301, 12354, 12354, 12356, 12356, 12358, 12358, 12360, 12360, 12363, 12363, 12364, 12364, 12365, 12365, 12367, 12367, 12370, 12370, 12373, 12373, 12375, 12375, 12376, 12376, 12377, 12377, 12378, 12378, 12379, 12379, 12381, 12381, 12383, 12383, 12384, 12384, 12390, 12390, 12391, 12391, 12392, 12392, 12393, 12393, 12394, 12394, 12395, 12395, 12398, 12398, 12399, 12399, 12411, 12411, 12414, 12414, 12415, 12415, 12418, 12418, 12420, 12420, 12424, 12424, 12425, 12425, 12426, 12426, 12427, 12427, 12428, 12428, 12431, 12431, 12434, 12434, 12449, 12449, 12450, 12450, 12451, 12451, 12452, 12452, 12454, 12454, 12456, 12456, 12458, 12458, 12459, 12459, 12460, 12460, 12461, 12461, 12463, 12463, 12464, 12464, 12466, 12466, 12467, 12467, 12469, 12469, 12471, 12471, 12472, 12472, 12473, 12473, 12474, 12474, 12475, 12475, 12479, 12479, 12481, 12481, 12483, 12483, 12484, 12484, 12486, 12486, 12487, 12487, 12488, 12488, 12489, 12489, 12490, 12490, 12496, 12496, 12500, 12500, 12501, 12501, 12503, 12503, 12505, 12505, 12506, 12506, 12508, 12508, 12511, 12511, 12512, 12512, 12513, 12513, 12515, 12515, 12517, 12517, 12519, 12519, 12521, 12521, 12522, 12522, 12523, 12523, 12524, 12524, 12525, 12525, 12527, 12527, 12531, 12531, 12540, 12540, 19968, 19968, 19978, 19978, 19979, 19979, 19981, 19981, 20001, 20001, 20013, 20013, 20102, 20102, 20114, 20114, 20280, 20280, 20301, 20301, 20302, 20302, 20316, 20316, 20351, 20351, 20445, 20445, 20498, 20498, 20516, 20516, 20687, 20687, 20778, 20778, 20808, 20808, 20837, 20837, 20869, 20869, 20966, 20966, 20999, 20999, 21033, 21033, 21046, 21046, 21066, 21066, 21106, 21106, 21177, 21177, 21205, 21205, 21270, 21270, 21313, 21313, 21336, 21336, 21407, 21407, 21454, 21454, 21487, 21487, 21491, 21491, 21512, 21512, 21516, 21516, 21521, 21521, 22240, 22240, 22522, 22522, 22577, 22577, 22580, 22580, 22679, 22679, 22768, 22768, 22810, 22810, 22823, 22823, 23383, 23383, 23384, 23384, 23450, 23450, 23455, 23455, 23550, 23550, 23567, 23567, 23569, 23569, 23849, 23849, 24038, 24038, 24120, 24120, 24230, 24230, 24310, 24310, 24335, 24335, 24515, 24515, 24540, 24540, 24615, 24615, 24773, 24773, 24863, 24863, 25104, 25104, 25147, 25147, 25233, 25233, 25246, 25246, 25551, 25551, 25563, 25563, 25805, 25805, 26041, 26041, 26085, 26085, 26126, 26126, 26367, 26367, 26368, 26368, 26377, 26377, 26412, 26412, 27178, 27178, 27231, 27231, 27491, 27491, 27770, 27770, 28310, 28310, 28961, 28961, 29702, 29702, 29992, 29992, 30011, 30011, 30053, 30053, 30340, 30340, 30465, 30465, 31034, 31034, 31471, 31471, 32004, 32004, 32066, 32066, 32294, 32294, 32302, 32302, 32622, 32622, 32972, 32972, 33021, 33021, 33521, 33521, 34920, 34920, 35201, 35201, 35299, 35299, 35328, 35328, 35373, 35373, 35486, 35486, 36275, 36275, 36605, 36605, 36796, 36796, 36895, 36895, 36933, 36933, 36984, 36984, 37096, 37096, 37197, 37197, 37325, 37325, 37682, 37682, 38480, 38480, 38500, 38500, 38555, 38555, 38754, 38754, 38899, 38899, 39443, 39443, 39640, 39640, 39854, 39854, 65288, 65288, 65289, 65289, 65374, 65374, 0];
    let font = include_bytes!("../../font/DSVitaUIJP.ttf");
    let mut config: ImFontConfig = std::mem::zeroed();
    ImFontConfig_ImFontConfig(&mut config);
    config.FontDataOwnedByAtlas = false;
    config.MergeMode = true;
    config.OversampleH = 1;
    config.OversampleV = 1;
    let loaded = ImFontAtlas_AddFontFromMemoryTTF((*ImGui::GetIO()).Fonts, font.as_ptr() as _, font.len() as _, 22f32, &config, RANGES.as_ptr());
    assert!(!loaded.is_null(), "Bundled Japanese UI font failed to load");
}
