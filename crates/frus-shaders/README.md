# frus-shaders

Custom fragment shaders and a GPU pass, as an application uses them (milestone 792): an
`AnimatedShader` plasma, a `ShaderMask` shimmer over a title, and a `GpuPass` drawing a turning
triangle with its own wgpu pipeline. "Full screen" shows the plasma alone; a tap stops it.

```bash
frus run            # the desktop
frus run -d web     # the browser (WebGPU, or WebGL 2)
frus run -d android # a phone
```

See [the shaders guide](../../docs/guide/shaders.md).
