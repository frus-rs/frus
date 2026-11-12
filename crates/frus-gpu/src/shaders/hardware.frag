// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

// The hardware buffer import pass (milestone 800): samples the imported buffer through a
// combined image sampler. For a YUV buffer the sampler is an immutable one carrying a
// sampler Y'CbCr conversion, so the texture read already answers non-linear R'G'B': written
// through a UNORM view, the bytes are what an sRGB texture holds. WGSL has no way to name
// such a sampler, hence GLSL. Compiled to SPIR-V with the NDK's glslc:
//   glslc -O --target-env=vulkan1.0 hardware.frag -o hardware.frag.spv
#version 450

layout(set = 0, binding = 0) uniform sampler2D frame;

layout(location = 0) in vec2 uv;
layout(location = 0) out vec4 color;

void main() {
    color = texture(frame, uv);
}
