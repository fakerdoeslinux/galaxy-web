# Porting Nebula to the browser

State as of 2026-11-16. This is the plan this repo is written against; the demo in `src/` is the renderer probe,
not the port.

## 0. What the port actually is

Nebula is C++20: a host executable, a game module (`libsmg_game.so`, ~55 MB with LumaCore rows), and a renderer
with separate Vulkan and D3D11/D3D12 backends behind one abstraction (`runtime/src/render/core`). The game data
lives in a `.pak` store — 3.8 GB, 31,444 nodes on a current build — and none of it is redistributable.

So a browser port is three jobs, not one:

1. **Compile the tree to WebAssembly.** C++20, threads, `dlopen`-free builds, filesystem shim.
2. **Write a fourth renderer backend: WebGPU.** Vulkan cannot exist in a page. WebGPU is the equivalent class of
   API, and on Chrome/Linux it is implemented *on* Vulkan.
3. **Stream a multi-gigabyte store over HTTP.** No browser will download 3.8 GB to start a level.

There is no path where the existing `vk` backend runs in a tab. Anyone claiming otherwise is describing WebGL.

## 1. The graphics backend, mapped

The abstraction in `runtime/src/render/core` is already split from the backends, which is the reason this port is
possible at all. WebGPU's shape is close enough to Vulkan that the mapping is mechanical rather than clever:

| Nebula / Vulkan concept | WebGPU |
|---|---|
| `VkDevice` + queues | `GPUDevice`, single queue |
| `VkPipeline` + `vkCreateGraphicsPipelines` | `GPURenderPipeline` (created synchronously, no cache file) |
| descriptor set | `GPUBindGroup` (+ layout, immutable) |
| `vkCmdBeginRenderPass` | `beginRenderPass`, no subpasses |
| `VkBuffer` / `VkImage` + manual barriers | buffer/texture + automatic tracking; `usage` flags decide |
| push constants | **none** — falls back to a uniform buffer + dynamic offset, or a per-frame ring |
| timeline semaphore | **none** — submit ordering is implicit, `onSubmittedWorkDone` to wait |
| `VK_EXT_descriptor_indexing` (bindless) | **no** — array of `texture_2d<f32>` bindings, or one texture array. This is the biggest rewrite. |
| compute dispatch | `dispatchWorkgroups` — fine |
| `VK_KHR_ray_query` | not exposed; this GPU has none anyway (`[rt] caps vulkan: ray-queries=0`) |
| present mode MAILBOX | `presentMode: 'mailbox'` is not in the spec; `'fifo'` or `'immediate'` only |
| SPIR-V | WGSL, or SPIR-V via `-sUSE_WEBGPU` and Naga |

Shaders are HLSL (`runtime/src/render/**/*.hlsl`, ~2,600 files). DXC compiles those to SPIR-V for the Vulkan
backend, so the shortest route to WGSL is DXC → SPIR-V → [Naga](https://github.com/gfx-rs/naga) → WGSL, offline,
checked into the repo as generated sources. Do not ship a shader compiler to the browser.

New files, following the existing backend pattern:

```
runtime/src/render/webgpu/
  device_webgpu.cpp        GPUDevice, adapter limits, canvas configuration
  pipelines_webgpu.cpp     mirrors pipelines.cpp / pipelines_d3d12.cpp
  encoder_webgpu.cpp       mirrors core/encoder_vk.cpp
  draw_walker_webgpu.cpp   mirrors the vk/ffx_vk_forward.cpp walk
  textures_webgpu.cpp      replaces gpu_texture_backend_d3d12.cpp; no bindless array
  post_webgpu.cpp          bloom, AO, FSR-equivalent (drop FSR3 frame gen: no queues)
```

`graphics_backend` in `config.ini` (`runtime/src/render/render_config.cpp`) grows a third value, `webgpu`, and the
settings row in `runtime/src/ui/settings_graphics_page.cpp` learns the name. The restart-on-change behaviour
already implemented for `vulkan`/`opengl` applies unchanged.

## 2. WebAssembly build

Emscripten, not a from-scratch wasm target: the host uses SDL2, threads (`std::thread`, `pipelines` worker pool),
and `std::filesystem`.

```bash
emcmake cmake -S platform/sdl -B build/web \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_CXX_FLAGS="-fwasm-exceptions -pthread -sUSE_WEBGPU=1 -sASYNCIFY=0" \
  -DNEBULA_GPU_WEBGPU=ON
```

Hard requirements and what each one costs:

- **`-pthread`** needs `SharedArrayBuffer`, which needs COOP/COEP headers. GitHub Pages cannot set response
  headers, so on Pages the threaded build needs a service worker that re-serves responses with
  `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp`. That worker is
  ~40 lines and is the first thing to write when the real port starts. Safari has no COOP/COEP-free path either.
- **No `dlopen`.** `runtime/src/mod_plugins.cpp`, `luma_assets.cpp`, `mod_check.cpp`, `mod_start_check.cpp` and
  `host_extension.cpp` all `dlopen`. For wasm, mod support and host extensions compile out; the game module links
  in statically as a second wasm module or in the same one behind a symbol table.
- **No `execv`.** The restart path in `native/host/host_main.cpp` (`finish_and_exit(true)`) becomes
  `location.reload()` — which also means the graphics-backend switch on the web reloads the tab and boots with the
  new backend from `localStorage`-backed config instead of `config.ini`.
- **No JIT / no self-modifying code.** Check `runtime/src/luma_*` for generated trampolines; anything that
  rewrites its own text segment has to become a switch or an indirect call table.

## 3. Asset streaming

`game.pak` is a flat node store (31k nodes) and already supports random access through the runtime's
`galaxy_asset_store` target. On the web it becomes an HTTP resource with a tiny read-only network layer:

- `HEAD` the store, read the node table from a byte range at the tail, keep it in memory.
- Each node read is an `Range:` request for exactly its bytes, behind a small LRU + `caches.open()` (Cache API)
  so a level's assets are paid for once and served from disk afterwards.
- Optional offline mode: copy fetched bytes into OPFS (`navigator.storage.getDirectory()`), with a quota check
  before each write. 3.8 GB is over most browsers' default quota unless `navigator.storage.persist()` is granted.

None of this ships in this repo. The store is game data; the visitor supplies it, exactly as the native build
requires a dump. A "load your own store" `<input type=file>` picker plus OPFS import is the honest delivery
mechanism, and the plan's default.

## 4. Input and audio

- **Input:** Gamepad API. `navigator.getGamepads()` in the frame loop maps to the same pad state the native input
  layer consumes (`runtime/src/input`), including the DualSense gyro via `Gamepad.mapping === 'standard'` plus
  `Motion` events where they exist — no IOHIDManager, so gyro pointer quality on Linux will be worse than native.
  Keyboard and the settings menus map 1:1 to the existing `VK_*` path.
- **Audio:** Web Audio. The DSP is already host-side (`native/host` sound blocks: `sound blocks 3158` in a 10 s
  window), so the backend is an `AudioWorkletNode` pulling the same 16-bit blocks. Decoding stays in wasm.

## 5. Phase order, and what to expect

| Phase | Work | Gate |
|---|---|---|
| 1 | Emscripten build of the host with a null renderer; asset store over `fetch` | must reach `boot: renderer started` with no picture |
| 2 | WebGPU device + swapchain + clear colour to canvas | first correct frame, any content |
| 3 | Pipelines + draw walker: one scene target, no bindless, atlas instead | Galaxy's first 3D scene visible |
| 4 | Textures, post (bloom/AO), teardown | 3D scenes count matches native |
| 5 | Input + audio + menus + settings + save files (IndexedDB) | playable, not fast |
| 6 | Performance: indirect draws where allowed, one big uniform ring, 50 % resolution scale default | stable frame time on Vega-class hardware |

Phase 3 is the wall: Galaxy's renderer leans on bindless textures and many small draw calls, and WebGPU has no
bindless. Expect an atlas/sampler-array conversion of the texture backend and a draw batching pass, which is weeks
of work, not days.

Performance expectation, stated plainly: this machine does ~1.35–2.28 ms per frame in the native Vulkan backend
at 854×480. A wasm + WebGPU build on the same Radeon Vega will not match that. WebGPU per-draw validation, wasm
indirection, and the missing bindless path put a realistic first-playable number at 15–30 fps at 480p, with
resolution scale doing the rest. **The demo in this repo exists to measure that gap honestly before any of the
above is promised.**

## 6. What this repo will never contain

No game code, no ROM, no `.pak`, no ripped textures, no patched binaries. GPL-3.0-only for what is here.
