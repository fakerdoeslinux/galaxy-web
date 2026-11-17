// galaxy-web — a WebGPU scene: a procedural world, its star field and an honest report of the
// browser's graphics backend. No game data is used or shipped; this is the renderer and the
// measuring stick the port plan in docs/port-plan.md is written against.

const QUERY_COUNT = 4;
const STAR_COUNT = 2600;
const MSAA = 4;
let samples = MSAA;  // 1 on a software adapter: multisample resolve is the first thing a CPU rasteriser chokes on

const canvas = document.getElementById('view');
const hudStats = document.getElementById('stats');
const hudFps = document.getElementById('perf-fps');
const hudMs = document.getElementById('perf-ms');
const hudSize = document.getElementById('perf-size');
const hudAdapter = document.getElementById('perf-adapter');
const hudDriver = document.getElementById('perf-driver');
const unsupported = document.getElementById('unsupported');

const state = {
  scale: 1,
  width: 1,
  height: 1,
  dpr: 1,
  yaw: 0.6,
  pitch: 0.22,
  distance: 3.15,
  dragging: false,
  lastX: 0,
  lastY: 0,
  idle: 0,
  time: 0,
  frames: 0,
  fps: 0,
  fpsAverage: 60,
  gpuMs: null,
};

// --- matrices ---------------------------------------------------------------------------------

function mat4Perspective(fieldOfView, aspect, near, far) {
  const f = 1 / Math.tan(fieldOfView / 2);
  const m = new Float32Array(16);
  m[0] = f / aspect;
  m[5] = f;
  m[10] = far / (near - far);
  m[11] = -1;
  m[14] = (far * near) / (near - far);
  return m;
}

function normalize3(v) {
  const l = Math.hypot(v[0], v[1], v[2]) || 1;
  return [v[0] / l, v[1] / l, v[2] / l];
}

function cross3(a, b) {
  return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
}

function dot3(a, b) {
  return a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
}

function mat4LookAt(eye, target, up) {
  const z = normalize3([eye[0] - target[0], eye[1] - target[1], eye[2] - target[2]]);
  const x = normalize3(cross3(up, z));
  const y = cross3(z, x);
  const m = new Float32Array(16);
  m[0] = x[0]; m[4] = x[1]; m[8] = x[2]; m[12] = -dot3(x, eye);
  m[1] = y[0]; m[5] = y[1]; m[9] = y[2]; m[13] = -dot3(y, eye);
  m[2] = z[0]; m[6] = z[1]; m[10] = z[2]; m[14] = -dot3(z, eye);
  m[15] = 1;
  return m;
}

function mat4Multiply(a, b) {
  const m = new Float32Array(16);
  for (let column = 0; column < 4; column++) {
    for (let row = 0; row < 4; row++) {
      let sum = 0;
      for (let k = 0; k < 4; k++) sum += a[k * 4 + row] * b[column * 4 + k];
      m[column * 4 + row] = sum;
    }
  }
  return m;
}

// --- geometry ---------------------------------------------------------------------------------

// A UV sphere with outward normals, interleaved as position.xyz + normal.xyz per vertex: one buffer,
// one 24-byte stride, one write. 96 x 48 segments: dense enough that the silhouette is round at
// 4x MSAA, small enough that the vertex buffer is 114 KB.
function sphereGeometry(segments = 96, rings = 48) {
  const vertexCount = (segments + 1) * (rings + 1);
  const vertices = new Float32Array(vertexCount * 6);
  let at = 0;
  for (let ring = 0; ring <= rings; ring++) {
    const polar = (ring / rings) * Math.PI;
    const sinPolar = Math.sin(polar);
    const cosPolar = Math.cos(polar);
    for (let segment = 0; segment <= segments; segment++) {
      const azimuth = (segment / segments) * Math.PI * 2;
      const x = sinPolar * Math.cos(azimuth);
      const y = cosPolar;
      const z = sinPolar * Math.sin(azimuth);
      vertices[at] = x;
      vertices[at + 1] = y;
      vertices[at + 2] = z;
      vertices[at + 3] = x;
      vertices[at + 4] = y;
      vertices[at + 5] = z;
      at += 6;
    }
  }
  const indices = new Uint32Array(segments * rings * 6);
  let index = 0;
  for (let ring = 0; ring < rings; ring++) {
    for (let segment = 0; segment < segments; segment++) {
      const a = ring * (segments + 1) + segment;
      const b = a + segments + 1;
      indices[index++] = a;
      indices[index++] = a + 1;
      indices[index++] = b;
      indices[index++] = a + 1;
      indices[index++] = b + 1;
      indices[index++] = b;
    }
  }
  return { vertices, indices };
}

function starGeometry(count) {
  // Instance data: direction (xyz), brightness, tint (rgb), twinkle seed.
  const data = new Float32Array(count * 8);
  const palette = [
    [0.72, 0.82, 1.0],
    [0.86, 0.91, 1.0],
    [1.0, 1.0, 1.0],
    [1.0, 0.94, 0.82],
    [1.0, 0.84, 0.66],
  ];
  for (let star = 0; star < count; star++) {
    // Even over the sphere, with a little clumping along a band for a galactic look.
    const u = Math.random();
    const y = 2 * u - 1;
    const radius = Math.sqrt(Math.max(0, 1 - y * y));
    const theta = Math.random() * Math.PI * 2;
    const band = Math.exp(-Math.pow((y - 0.15) / 0.22, 2)) * 0.35;
    const direction = normalize3([radius * Math.cos(theta), y + (Math.random() - 0.5) * band, radius * Math.sin(theta)]);
    const magnitude = Math.pow(Math.random(), 2.6);
    const tint = palette[Math.floor(Math.random() * palette.length)];
    const at = star * 8;
    data[at] = direction[0];
    data[at + 1] = direction[1];
    data[at + 2] = direction[2];
    data[at + 3] = 0.35 + magnitude * 1.9;
    data[at + 4] = tint[0];
    data[at + 5] = tint[1];
    data[at + 6] = tint[2];
    data[at + 7] = Math.random();
  }
  return data;
}

// --- the program ------------------------------------------------------------------------------

async function readShader(name) {
  const response = await fetch(`src/shaders/${name}.wgsl`);
  if (!response.ok) throw new Error(`src/shaders/${name}.wgsl: ${response.status}`);
  return response.text();
}

async function start() {
  if (!navigator.gpu) {
    showUnsupported('This browser has no navigator.gpu: WebGPU is off or not built in.');
    return;
  }
  // Adapter choice, in order: the fast discrete GPU, whatever the browser offers by default, then a
  // software fallback (SwiftShader / lavapipe) so the page still runs on a machine with no usable GPU.
  let adapter = await navigator.gpu.requestAdapter({ powerPreference: 'high-performance' });
  let software = false;
  if (!adapter) adapter = await navigator.gpu.requestAdapter();
  if (!adapter) {
    adapter = await navigator.gpu.requestAdapter({ forceFallbackAdapter: true });
    software = Boolean(adapter);
  }
  if (!adapter) {
    showUnsupported('navigator.gpu is there, but the browser found no graphics adapter it can use.');
    return;
  }
  if (software) {
    samples = 1;
    console.warn('[galaxy-web] no hardware adapter: using the browser\'s software fallback at 1x. Expect single-digit fps.');
  }
  const wanted = [];
  if (adapter.features.has('timestamp-query')) wanted.push('timestamp-query');
  const device = await adapter.requestDevice({ requiredFeatures: wanted });
  device.addEventListener('uncapturederror', (event) => {
    console.error('[galaxy-web] WebGPU error:', event.error.message);
  });

  const format = navigator.gpu.getPreferredCanvasFormat();
  const context = canvas.getContext('webgpu');
  context.configure({ device, format, alphaMode: 'opaque' });

  const [planetSource, starSource] = await Promise.all([readShader('planet'), readShader('starfield')]);

  const uniformBuffer = device.createBuffer({ size: 128, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });

  // One shared bind group layout: a single uniform block.
  const bindGroupLayout = device.createBindGroupLayout({
    entries: [{ binding: 0, visibility: GPUShaderStage.VERTEX | GPUShaderStage.FRAGMENT, buffer: { type: 'uniform' } }],
  });
  const bindGroup = device.createBindGroup({
    layout: bindGroupLayout,
    entries: [{ binding: 0, resource: { buffer: uniformBuffer } }],
  });
  const pipelineLayout = device.createPipelineLayout({ bindGroupLayouts: [bindGroupLayout] });

  const planetModule = device.createShaderModule({ code: planetSource, label: 'planet.wgsl' });
  const starModule = device.createShaderModule({ code: starSource, label: 'starfield.wgsl' });
  for (const [name, module] of [['planet', planetModule], ['starfield', starModule]]) {
    const info = await module.getCompilationInfo();
    for (const message of info.messages) {
      if (message.type === 'error') console.error(`[galaxy-web] ${name}.wgsl:${message.lineNum}: ${message.message}`);
    }
  }

  const planetPipeline = device.createRenderPipeline({
    label: 'planet',
    layout: pipelineLayout,
    vertex: {
      module: planetModule,
      entryPoint: 'vs',
      buffers: [{
        arrayStride: 24,
        attributes: [
          { shaderLocation: 0, offset: 0, format: 'float32x3' },
          { shaderLocation: 1, offset: 12, format: 'float32x3' },
        ],
      }],
    },
    fragment: { module: planetModule, entryPoint: 'fs', targets: [{ format }] },
    primitive: { topology: 'triangle-list', cullMode: 'back', frontFace: 'ccw' },
    depthStencil: { format: 'depth24plus', depthWriteEnabled: true, depthCompare: 'less' },
    multisample: { count: samples },
  });

  const starPipeline = device.createRenderPipeline({
    label: 'starfield',
    layout: pipelineLayout,
    vertex: {
      module: starModule,
      entryPoint: 'vs',
      buffers: [{
        arrayStride: 32,
        stepMode: 'instance',
        attributes: [
          { shaderLocation: 0, offset: 0, format: 'float32x3' },
          { shaderLocation: 1, offset: 12, format: 'float32' },
          { shaderLocation: 2, offset: 16, format: 'float32x3' },
          { shaderLocation: 3, offset: 28, format: 'float32' },
        ],
      }],
    },
    fragment: {
      module: starModule,
      entryPoint: 'fs',
      targets: [{
        format,
        blend: {
          color: { srcFactor: 'src-alpha', dstFactor: 'one', operation: 'add' },
          alpha: { srcFactor: 'zero', dstFactor: 'one', operation: 'add' },
        },
      }],
    },
    primitive: { topology: 'triangle-list', cullMode: 'none' },
    depthStencil: { format: 'depth24plus', depthWriteEnabled: false, depthCompare: 'always' },
    multisample: { count: samples },
  });

  // Geometry on the GPU.
  const sphere = sphereGeometry();
  const planetVertexBuffer = device.createBuffer({
    size: sphere.vertices.byteLength,
    usage: GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST,
  });
  device.queue.writeBuffer(planetVertexBuffer, 0, sphere.vertices);
  const planetIndexBuffer = device.createBuffer({
    size: sphere.indices.byteLength,
    usage: GPUBufferUsage.INDEX | GPUBufferUsage.COPY_DST,
  });
  device.queue.writeBuffer(planetIndexBuffer, 0, sphere.indices);

  const starBuffer = device.createBuffer({
    size: STAR_COUNT * 32,
    usage: GPUBufferUsage.VERTEX | GPUBufferUsage.COPY_DST,
  });
  device.queue.writeBuffer(starBuffer, 0, starGeometry(STAR_COUNT));

  // Timestamps, when the adapter offers them: real GPU work, not an estimate.
  const timers = adapter.features.has('timestamp-query') ? makeTimers(device) : null;

  let colorTexture = null;
  let depthTexture = null;

  function resize() {
    const rect = canvas.getBoundingClientRect();
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const width = Math.max(1, Math.floor(rect.width * dpr * state.scale));
    const height = Math.max(1, Math.floor(rect.height * dpr * state.scale));
    if (width === state.width && height === state.height && colorTexture) return;
    state.width = width;
    state.height = height;
    state.dpr = dpr;
    canvas.width = width;
    canvas.height = height;
    colorTexture?.destroy();
    depthTexture?.destroy();
    colorTexture = device.createTexture({
      size: [width, height],
      sampleCount: samples,
      format,
      usage: GPUTextureUsage.RENDER_ATTACHMENT,
    });
    depthTexture = device.createTexture({
      size: [width, height],
      sampleCount: samples,
      format: 'depth24plus',
      usage: GPUTextureUsage.RENDER_ATTACHMENT,
    });
  }

  const uniforms = new Float32Array(32);
  const eye = [0, 0, 0];
  // Low sun, almost perpendicular to the view direction, so the terminator crosses the disc.
  const sun = normalize3([-0.62, 0.35, 0.70]);

  function writeUniforms() {
    const aspect = state.width / state.height;
    const projection = mat4Perspective((45 * Math.PI) / 180, aspect, 0.05, 6000);
    const cosPitch = Math.cos(state.pitch);
    eye[0] = state.distance * cosPitch * Math.sin(state.yaw);
    eye[1] = state.distance * Math.sin(state.pitch);
    eye[2] = state.distance * cosPitch * Math.cos(state.yaw);
    const view = mat4LookAt(eye, [0, 0, 0], [0, 1, 0]);
    const viewProjection = mat4Multiply(projection, view);

    uniforms.set(viewProjection, 0);
    uniforms.set([eye[0], eye[1], eye[2], state.time], 16);
    uniforms.set([sun[0], sun[1], sun[2], 1.55], 20);
    uniforms.set([aspect, 2.2, 1.0, state.scale], 24);
    uniforms.set([state.width, state.height, state.dpr, state.frames], 28);
    device.queue.writeBuffer(uniformBuffer, 0, uniforms);
  }

  function frame(now) {
    const seconds = now / 1000;
    const delta = state.lastFrame ? Math.min(0.1, seconds - state.lastFrame) : 0;
    state.lastFrame = seconds;
    state.time += delta;
    state.frames++;
    state.idle += delta;
    if (!state.dragging && state.idle > 2.5) state.yaw += delta * 0.055;
    resize();
    writeUniforms();

    const encoder = device.createCommandEncoder();
    const pass = encoder.beginRenderPass({
      colorAttachments: [{
        view: colorTexture.createView(),
        resolveTarget: context.getCurrentTexture().createView(),
        clearValue: { r: 0.004, g: 0.008, b: 0.024, a: 1 },
        loadOp: 'clear',
        storeOp: 'discard',
      }],
      depthStencilAttachment: {
        view: depthTexture.createView(),
        depthClearValue: 1,
        depthLoadOp: 'clear',
        depthStoreOp: 'discard',
      },
      timestampWrites: timers ? timers.writes() : undefined,
    });
    pass.setBindGroup(0, bindGroup);
    pass.setPipeline(starPipeline);
    pass.setVertexBuffer(0, starBuffer);
    pass.draw(6, STAR_COUNT);
    pass.setPipeline(planetPipeline);
    pass.setVertexBuffer(0, planetVertexBuffer);
    pass.setIndexBuffer(planetIndexBuffer, 'uint32');
    pass.drawIndexed(sphere.indices.length);
    pass.end();
    if (timers) timers.resolve(encoder);
    device.queue.submit([encoder.finish()]);
    if (timers) timers.read((milliseconds) => { state.gpuMs = milliseconds; });

    const instant = delta > 0 ? 1 / delta : 0;
    state.fpsAverage = state.fpsAverage * 0.92 + instant * 0.08;
    updateHud();
    requestAnimationFrame(frame);
  }

  function updateHud() {
    if (state.frames % 6 !== 0) return;
    hudFps.textContent = `${state.fpsAverage.toFixed(0)} fps`;
    hudMs.textContent = state.gpuMs === null ? 'timestamps off' : `${state.gpuMs.toFixed(2)} ms`;
    hudSize.textContent = `${state.width}×${state.height} · ${format} · ${samples}x`;
    hudStats.textContent = `${STAR_COUNT} stars · ${state.width}×${state.height} · ${format}`;
  }

  const info = adapter.info ?? (adapter.requestAdapterInfo ? await adapter.requestAdapterInfo() : {});
  const describe = [info.vendor, info.architecture, info.device, info.description].filter(Boolean).join(' ');
  hudAdapter.textContent = describe || 'unknown adapter';
  hudDriver.textContent = `WebGPU${software ? ' · software' : ''} · ${format}`;
  console.info('[galaxy-web] adapter', info, 'features', [...adapter.features]);

  // --- input -----------------------------------------------------------------------------------

  canvas.addEventListener('pointerdown', (event) => {
    state.dragging = true;
    state.idle = 0;
    state.lastX = event.clientX;
    state.lastY = event.clientY;
    canvas.setPointerCapture(event.pointerId);
  });
  canvas.addEventListener('pointermove', (event) => {
    if (!state.dragging) return;
    state.yaw -= (event.clientX - state.lastX) * 0.005;
    state.pitch = Math.max(-1.35, Math.min(1.35, state.pitch + (event.clientY - state.lastY) * 0.005));
    state.lastX = event.clientX;
    state.lastY = event.clientY;
    state.idle = 0;
  });
  const release = (event) => {
    state.dragging = false;
    if (canvas.hasPointerCapture?.(event.pointerId)) canvas.releasePointerCapture(event.pointerId);
  };
  canvas.addEventListener('pointerup', release);
  canvas.addEventListener('pointercancel', release);
  canvas.addEventListener('wheel', (event) => {
    event.preventDefault();
    state.distance = Math.max(1.55, Math.min(7.5, state.distance * (1 + Math.sign(event.deltaY) * 0.08)));
    state.idle = 0;
  }, { passive: false });

  for (const button of document.querySelectorAll('#panel .scales button')) {
    button.addEventListener('click', () => {
      state.scale = Number(button.dataset.scale);
      for (const other of document.querySelectorAll('#panel .scales button')) {
        other.classList.toggle('on', other === button);
      }
      state.width = 0;  // forces the textures to be rebuilt at the new size
      resize();
    });
  }

  device.lost.then((info) => {
    console.error('[galaxy-web] device lost:', info.reason, info.message);
    showUnsupported(`The graphics device was lost (${info.reason}). Reload to start again.`);
  });

  resize();
  requestAnimationFrame(frame);
}

// Timestamp queries around the one render pass, read from a ring of 4 staging buffers so the CPU never
// waits on the GPU to ask how long the GPU took.
function makeTimers(device) {
  const querySet = device.createQuerySet({ type: 'timestamp', count: 2 });
  const resolveBuffer = device.createBuffer({
    size: 16,
    usage: GPUBufferUsage.QUERY_RESOLVE | GPUBufferUsage.COPY_SRC | GPUBufferUsage.COPY_DST,
  });
  const ring = [];
  for (let slot = 0; slot < QUERY_COUNT; slot++) {
    ring.push({
      buffer: device.createBuffer({ size: 16, usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ }),
      busy: false,
    });
  }
  let next = 0;
  let pending = 0;

  const pick = () => {
    for (let attempt = 0; attempt < QUERY_COUNT; attempt++) {
      const slot = ring[next];
      next = (next + 1) % QUERY_COUNT;
      if (!slot.busy) return slot;
    }
    return null;
  };

  let current = null;
  return {
    writes() {
      current = pick();
      if (!current) return undefined;
      return { querySet, beginningOfPassWriteIndex: 0, endOfPassWriteIndex: 1 };
    },
    resolve(encoder) {
      if (!current) return;
      current.busy = true;
      encoder.resolveQuerySet(querySet, 0, 2, resolveBuffer, 0);
      encoder.copyBufferToBuffer(resolveBuffer, 0, current.buffer, 0, 16);
    },
    read(callback) {
      if (!current) return;
      const slot = current;
      pending++;
      slot.buffer.mapAsync(GPUMapMode.READ).then(() => {
        const stamps = new BigUint64Array(slot.buffer.getMappedRange().slice(0));
        slot.buffer.unmap();
        slot.busy = false;
        pending--;
        const nanoseconds = Number(stamps[1] - stamps[0]);
        if (Number.isFinite(nanoseconds) && nanoseconds > 0) callback(nanoseconds / 1e6);
      }).catch(() => { slot.busy = false; pending--; });
      current = null;
    },
    get pending() { return pending; },
  };
}

function showUnsupported(why) {
  unsupported.hidden = false;
  document.getElementById('unsupported-why').textContent = why;
  document.getElementById('panel').hidden = true;
}

start().catch((error) => {
  console.error('[galaxy-web] start failed:', error);
  showUnsupported(`The renderer did not start: ${error.message}`);
});
