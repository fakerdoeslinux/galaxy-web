// galaxy-web: the shared uniform block. Layout must match src/main.js (UPLOAD_BYTES).
struct Uniforms {
  view_proj  : mat4x4f,  // 64 bytes
  camera_pos : vec4f,    // xyz eye, w seconds since start
  sun_dir    : vec4f,    // xyz light direction (from the sun towards the scene), w exposure
  params     : vec4f,    // x aspect, y star size in pixels, z planet radius, w resolution scale
  screen     : vec4f,    // x pixel width, y pixel height, z device pixels per CSS pixel, w frames drawn
};
