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
