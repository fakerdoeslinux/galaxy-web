# webgpu-check

An offscreen mirror of `src/main.js`: the same two pipeline descriptors, the same vertex and instance
layouts, the same 128-byte uniform block, and the same WGSL from `src/shaders/`. It renders one
512×512 frame through wgpu on the machine's real GPU and reports whether the descriptors and the
shaders agree.

It exists because a machine can be missing a browser with a working WebGPU adapter — this one is —
and then the page cannot be visualised at all. The mirror runs on the same Vulkan device the page
would land on under Chrome/Linux (Dawn → `vk`), so a pass here means the page's graphics side is
sound, and a failure here is a real failure.

## Run

```bash
cd tools/webgpu-check
cargo run --release            # shaders from ../../src/shaders, PNG to ./frame.png
cargo run --release -- a.wgsl b.wgsl out.png
```

Output: the adapter, a readback statistic, three sample pixels, an ASCII luminance map, an ASCII hue
map (ocean / continents / rock), the PNG path, and a pass/fail line.

Requires a Vulkan-capable GPU and `libvulkan`. wgpu picks the backend itself; on this machine that is
RADV.

## The pass condition

`lit > 20 % of the frame`, `peak > 60`, and more than 24 distinct quantised colours inside the disc.
Luminance alone is not enough: land and ocean albedos have nearly the same luminance, so a planet
whose noise field collapsed to one value still passes a brightness test. Colour variety is what
catches that.

## What it caught

- The sphere's triangle quads were wound clockwise seen from outside, so with `frontFace: 'ccw'` and
  `cullMode: 'back'` the renderer drew the *inside of the back hemisphere*. Every visible normal
  pointed away from the camera, `dot(normal, to_eye)` was 0 at every pixel, and the atmosphere rim
  term (`pow(1 - ndv, 3.4)`) was 1 over the whole disc. Fixed by emitting `a, a+1, b / a+1, b+1, b`.
- A sun direction almost parallel to the view direction, which flattened the shading into a disc of
  one tone. The harness now uses the same low side sun as the page.
