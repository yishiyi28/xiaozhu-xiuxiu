#!/usr/bin/env bash
# Run inside an Ubuntu container with release files mounted at /packages.
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends \
    xvfb xauth libvulkan1 mesa-vulkan-drivers libxkbcommon0 libxkbcommon-x11-0 \
    libwayland-client0 libx11-6 libx11-xcb1 libxcb1 libxcursor1 libxi6 \
    libxrandr2 libxfixes3

export XDG_RUNTIME_DIR=/tmp/xuan-runtime
mkdir -m700 -p "$XDG_RUNTIME_DIR"
mkdir -p '/tmp/xuan compatibility'
cd '/tmp/xuan compatibility'
unset WAYLAND_DISPLAY

xuan_appimages=(/packages/*.AppImage)
[[ ${#xuan_appimages[@]} == 1 ]]
xuan_appimage=${xuan_appimages[0]}
env APPIMAGE_EXTRACT_AND_RUN=1 LD_DEBUG=libs "$xuan_appimage" --version 2>loader.log
if [[ $(getconf GNU_LIBC_VERSION) == 'glibc 2.31' ]]; then
    # Ubuntu 20.04 cannot launch the native glibc 2.35 binary. Prove that the
    # AppImage uses its private libc, then exercise graphics with that libc.
    grep -E 'calling init: .*/usr/lib/glibc/libc.so.6' loader.log
fi
timeout 120s xvfb-run -a env APPIMAGE_EXTRACT_AND_RUN=1 \
    "$xuan_appimage" --demo --screenshot 'appimage demo.png'
test -s 'appimage demo.png'

if [[ ${XUAN_CHECK_NATIVE:-0} == 1 ]]; then
    tar -xzf /packages/xiaozhu-xiuxiu-*-linux-x86_64.tar.gz
    timeout 120s xvfb-run -a ./xiaozhu-xiuxiu-*-linux-x86_64/bin/xiaozhu-xiuxiu \
        --demo --screenshot 'native demo.png'
    test -s 'native demo.png'
fi
