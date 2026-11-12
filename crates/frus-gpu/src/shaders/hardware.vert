// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 KalybosPro <kalyboskokou1@gmail.com>

// The hardware buffer import pass (milestone 800): one triangle covering the target, whose
// texture coordinates span the buffer's crop. Compiled to SPIR-V with the NDK's glslc:
//   glslc -O --target-env=vulkan1.0 hardware.vert -o hardware.vert.spv
#version 450

// The crop, in the buffer's texture coordinates: offset in xy, size in zw.
layout(push_constant) uniform Crop {
    vec4 rect;
} crop;

layout(location = 0) out vec2 uv;

void main() {
    vec2 p = vec2(float((gl_VertexIndex << 1) & 2), float(gl_VertexIndex & 2));
    uv = crop.rect.xy + p * crop.rect.zw;
    gl_Position = vec4(p * 2.0 - 1.0, 0.0, 1.0);
}
