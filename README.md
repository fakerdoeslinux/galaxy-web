# galaxy-web

A WebGPU renderer demo for the browser, and the written port plan for running
[Nebula](https://github.com/keepvaibin/nebula) — the native Linux re-implementation of *Super Mario Galaxy* —
in a browser tab.

The page you get is **not the game**. It is a procedural planet, its star field, real GPU timing, and an honest
report of what the browser's graphics stack gave us. It exists to do two things:

1. prove the browser is running the same class of hardware path Nebula's Vulkan renderer uses, and
2. be the measuring stick the port plan is written against.

**[Open it](https://fakerdoeslinux.github.io/galaxy-web/)** · **[Port plan](https://fakerdoeslinux.github.io/galaxy-web/docs/)**

## What it does

- One render pass, 4× MSAA, depth-tested planet drawn over an additive instanced star field.
- No textures and no binary assets: the surface is 3D value noise in the fragment shader, so the whole site is
  source you can read.
- `timestamp-query` when the adapter has it, read through a 4-deep staging ring so the CPU never stalls on the GPU.
- Resolution scale 50 / 75 / 100 %, device pixel ratio capped at 2.
- Drag to orbit, wheel to zoom; it drifts on its own after ~2.5 s idle.

## Why WebGPU and not "Vulkan"

Browsers do not expose Vulkan, and they never will — Vulkan is a driver-level API and a page is not allowed near
one. WebGPU is the browser's Vulkan-class API: explicit pipelines, explicit bind groups, explicit barriers, compute
shaders, storage buffers, one command encoder per frame. On Linux, Chrome's WebGPU is implemented on top of Vulkan
(Dawn → `vk`), so on a RADV machine like this one the draw actually lands on the same `libvulkan` stack Nebula's
own Vulkan backend uses. What you gain over WebGL is not "Vulkan" — it is no driver-side validation guessing, no
per-draw state recompiles, and real timestamp queries.

## Run it locally

Any static server works; `file://` will not, because the shaders are fetched.

```bash
cd galaxy-web
python3 -m http.server 8080
# then open http://localhost:8080
```

Needs a browser with WebGPU: Chrome / Chromium 113+, Edge 113+, Firefox 141+, Safari 26+. On Chrome/Linux,
`chrome://gpu` should say "WebGPU: Hardware accelerated".

## Layout

```
index.html                 the page: canvas, HUD, resolution controls
src/style.css              HUD and panel
src/main.js                device, pipelines, geometry, frame loop, input, timing
src/shaders/planet.wgsl    procedural world: ocean, continents, ice, specular sea, atmosphere
src/shaders/starfield.wgsl instanced star billboards
src/shaders/uniforms.wgsl  the shared 128-byte uniform block (documentation copy)
docs/port-plan.md          how Nebula gets from native Vulkan to this
.github/workflows/pages.yml  build-free deploy to GitHub Pages
```

## The full port

Not done, and not hidden: shipping *Super Mario Galaxy* in a tab means compiling Nebula's C++ to WebAssembly and
writing a WebGPU backend against its renderer abstraction. `docs/port-plan.md` has the phase list, the exact files
that need to change, and the two things that gate it (asset streaming for a 3.8 GB store, and the fact that the
game data is not redistributable — this repo ships none of it).

## Licence

The demo code here is GPL-3.0-only, matching Nebula. No game code, no game assets, no ROM, no store.
