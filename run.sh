#!/usr/bin/env bash
# Usage: run.sh torus|synth|galaxy   (inside a herdr pane)
# NixOS: wgpu needs the 64-bit Vulkan loader plus the system Mesa drivers (RADV etc.).
# Set VULKAN_LOADER_LIB to skip the search.
if [ -z "$VULKAN_LOADER_LIB" ]; then
  for d in /nix/store/*-vulkan-loader-*/lib; do
    # ELF class byte 2 = 64-bit; the store also holds the i686 loader
    [ "$(od -An -tx1 -j4 -N1 "$d/libvulkan.so.1" 2>/dev/null | tr -d ' ')" = 02 ] && VULKAN_LOADER_LIB=$d && break
  done
fi
export LD_LIBRARY_PATH=${VULKAN_LOADER_LIB:+$VULKAN_LOADER_LIB:}/run/opengl-driver/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
exec "$(dirname "$0")/target/release/idle" "$@" 2>/dev/null
