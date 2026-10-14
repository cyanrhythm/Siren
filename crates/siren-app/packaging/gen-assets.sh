#!/usr/bin/env bash
# 由 assets/packaging/*.svg 生成安装包用的派生资源（.ico / .bmp）。
# 依赖：rsvg-convert、ImageMagick（magick）。任选仓库内目录运行均可。
#
# 说明：
# - installer.ico / uninstall.ico：多尺寸（16/32/48/64/128/256）供 NSIS 使用。
# - install-header.bmp / install-sidebar.bmp：MUI2 的页眉(150×57)与侧栏(164×314)设计，
#   这里按 2× 光栅化；MUI2 默认 *_STRETCH=FitControl，会自动缩放到控件，高 DPI 下更清晰。
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
DIR=assets/packaging
SIZES="16 32 48 64 128 256"

for name in installer uninstall; do
	tmp=$(mktemp -d)
	for s in $SIZES; do
		rsvg-convert -w "$s" -h "$s" "$DIR/$name.svg" -o "$tmp/$s.png"
	done
	magick "$tmp/16.png" "$tmp/32.png" "$tmp/48.png" "$tmp/64.png" "$tmp/128.png" "$tmp/256.png" "$DIR/$name.ico"
	rm -rf "$tmp"
done

rsvg-convert -w 300 -h 114 "$DIR/install-header.svg" -o /tmp/_siren_header.png
magick /tmp/_siren_header.png -type truecolor "BMP3:$DIR/install-header.bmp"

rsvg-convert -w 328 -h 628 "$DIR/install-sidebar.svg" -o /tmp/_siren_sidebar.png
magick /tmp/_siren_sidebar.png -type truecolor "BMP3:$DIR/install-sidebar.bmp"

rm -f /tmp/_siren_header.png /tmp/_siren_sidebar.png
echo "regenerated: $DIR/{installer,uninstall}.ico $DIR/install-{header,sidebar}.bmp"
