#!/usr/bin/env bash
# UI 冒烟：在 Xvfb + 软件 GL 下启动 Siren，自动截图到 target/ui-smoke/。
#
# 用法：
#   scripts/ui-smoke.sh                                  # 只截首页
#   SMOKE_PLAYLIST=569850893 scripts/ui-smoke.sh         # 启动即打开歌单（截图 01）
#   SMOKE_PLAYLIST=569850893 SMOKE_SCROLL=40 scripts/ui-smoke.sh   # 再滚 40 格截底部（02）
#
# 可调环境变量：
#   SMOKE_PLAYLIST  歌单 id（走 `SIREN_START=playlist:<id>`）
#   SMOKE_SCROLL    滚轮格数（0 = 不滚；默认 0）
#   SMOKE_WAIT      等待首帧的最长秒数（默认 12）
#   SMOKE_LOAD_WAIT 首帧后再等几秒让页面数据加载（默认 3）
#   SMOKE_OUT       输出目录（默认 target/ui-smoke）
#   SMOKE_DISPLAY   Xvfb 显示号（默认 :99）
#   SMOKE_NO_BUILD  设为 1 跳过 cargo build
#   SMOKE_BIN       指定二进制（默认 target/debug/siren）
#
# 依赖：Xvfb、ImageMagick（import/convert）、python3（ctypes 调 libXtst）。
# 说明：使用真实配置目录以读取登录态；日志/截图写 target/，XDG_RUNTIME_DIR 指到
# 临时目录，避免和正在运行的 Siren 抢单实例锁；结束会 kill 测试进程。
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
OUT=${SMOKE_OUT:-"$ROOT/target/ui-smoke"}
DISPLAY_NUM=${SMOKE_DISPLAY:-:99}
WAIT=${SMOKE_WAIT:-12}
LOAD_WAIT=${SMOKE_LOAD_WAIT:-3}
SCROLL=${SMOKE_SCROLL:-0}
PLAYLIST=${SMOKE_PLAYLIST:-}
NO_BUILD=${SMOKE_NO_BUILD:-0}
BIN=${SMOKE_BIN:-"$ROOT/target/debug/siren"}
WINDOW_W=1500
WINDOW_H=860

for tool in Xvfb import convert python3; do
    command -v "$tool" >/dev/null 2>&1 || { echo "缺少依赖：$tool" >&2; exit 1; }
done

if [ "$NO_BUILD" != "1" ]; then
    (cd "$ROOT" && cargo build -p siren-app)
fi
[ -x "$BIN" ] || { echo "找不到 $BIN（先 cargo build -p siren-app）" >&2; exit 1; }

rm -rf "$OUT"
mkdir -p "$OUT/logs" "$OUT/runtime"
chmod 700 "$OUT/runtime"

APID=""
XPID=""
cleanup() {
    [ -n "$APID" ] && kill -9 "$APID" 2>/dev/null || true
    [ -n "$XPID" ] && kill "$XPID" 2>/dev/null || true
    wait 2>/dev/null || true
}
trap cleanup EXIT

Xvfb "$DISPLAY_NUM" -screen 0 1600x900x24 -nolisten tcp -extension GLX >"$OUT/xvfb.log" 2>&1 &
XPID=$!
sleep 1.5

export DISPLAY="$DISPLAY_NUM"
app_env=(
    XDG_SESSION_TYPE=x11
    WAYLAND_DISPLAY=
    XDG_RUNTIME_DIR="$OUT/runtime"
    WGPU_BACKEND=gl
    LIBGL_ALWAYS_SOFTWARE=1
    GALLIUM_DRIVER=llvmpipe
    MESA_LOADER_DRIVER_OVERRIDE=llvmpipe
    __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json
    SIREN_X=0 SIREN_Y=0 SIREN_W=$WINDOW_W SIREN_H=$WINDOW_H
    SIREN_LOG_DIR="$OUT/logs"
    SIREN_LOG_STDERR=1
    SIREN_LOG_LEVEL=info
)
[ -n "$PLAYLIST" ] && app_env+=("SIREN_START=playlist:$PLAYLIST")
env "${app_env[@]}" "$BIN" >"$OUT/app.log" 2>&1 &
APID=$!

shot() { import -window root "$OUT/$1.png" 2>/dev/null || true; }

# 等首帧：截图平均亮度 > 0.01 认为已经渲染（全黑 = 还没出画面）。
ready=0
for _ in $(seq 1 "$WAIT"); do
    sleep 1
    shot 01-start
    if [ -f "$OUT/01-start.png" ]; then
        mean=$(convert "$OUT/01-start.png" -colorspace Gray -format "%[fx:mean]" info: 2>/dev/null || echo 0)
        if awk "BEGIN{exit !($mean > 0.01)}"; then
            ready=1
            break
        fi
    fi
done
if [ "$ready" != "1" ]; then
    echo "!! 画面未渲染，见 $OUT/app.log" >&2
    tail -20 "$OUT/app.log" >&2
    exit 1
fi
[ -n "$PLAYLIST" ] && sleep "$LOAD_WAIT" && shot 01-start

if [ "$SCROLL" -gt 0 ] 2>/dev/null; then
    # 在列表区域发滚轮（button 5），等缓动结束后截底部。
    python3 - "$DISPLAY_NUM" "$SCROLL" <<'PY'
import ctypes
import sys
import time

display = sys.argv[1].encode()
notches = int(sys.argv[2])
x11 = ctypes.cdll.LoadLibrary("libX11.so.6")
xtst = ctypes.cdll.LoadLibrary("libXtst.so.6")
x11.XOpenDisplay.restype = ctypes.c_void_p
conn = x11.XOpenDisplay(display)
if not conn:
    raise SystemExit("无法连接 X 显示")


def flush() -> None:
    x11.XFlush(ctypes.c_void_p(conn))


# 指针放在列表中部（避开左右栏），滚轮事件才会落到列表上。
xtst.XTestFakeMotionEvent(ctypes.c_void_p(conn), -1, 800, 500, 0)
flush()
time.sleep(0.2)
for _ in range(notches):
    xtst.XTestFakeButtonEvent(ctypes.c_void_p(conn), 5, 1, 0)
    xtst.XTestFakeButtonEvent(ctypes.c_void_p(conn), 5, 0, 0)
flush()
time.sleep(0.8)
PY
    shot 02-bottom
fi

echo "截图输出：$OUT"
ls -1 "$OUT"/*.png 2>/dev/null || true
