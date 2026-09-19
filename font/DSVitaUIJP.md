# DSVita Japanese UI font

DSVitaUIJP.ttf is a renamed subset of Noto Sans JP, licensed under SIL OFL 1.1.
The license is included in the installed VPK at licenses/DSVitaUIJP-OFL.txt.

Source: https://github.com/google/fonts/blob/main/ofl/notosansjp/NotoSansJP%5Bwght%5D.ttf
Verified Git blob SHA-1: cdd8f083c1f5928ff3361f8cda4d3fc9462cbe89
License Git blob SHA-1: 1c9f43281b8f216c5461fe9ac729afbade7724e4

Generated with fonttools 4.59.2: instantiate weight 400, subset to the 222 unique
BMP code points above U+00FF in ja_jp.rs and apply_japanese_ui.py's original
translation strings, retain name records, and rename family/PostScript names
to DSVitaUIJP (style Regular). Base Latin glyphs continue to use Open Sans.

When adding translated UI strings, regenerate the subset and static RANGES in
ja_jp::init_font, and verify every translated character exists in the font cmap.
This font covers UI strings only; optional game-title CJK font support is unchanged.
