// The star field: one instanced billboard per star, additive, no depth write.
// Instance data: direction (xyz), brightness, tint (rgb), twinkle seed.

struct Uniforms {
  view_proj  : mat4x4f,
  camera_pos : vec4f,
  sun_dir    : vec4f,
  params     : vec4f,
  screen     : vec4f,
};
@group(0) @binding(0) var<uniform> u : Uniforms;

struct Instance {
  @location(0) direction : vec3f,
  @location(1) brightness : f32,
  @location(2) tint : vec3f,
  @location(3) seed : f32,
};

struct VSOut {
  @builtin(position) clip : vec4f,
  @location(0) uv : vec2f,
  @location(1) tint : vec3f,
  @location(2) brightness : f32,
  @location(3) seed : f32,
};

const SKY_RADIUS : f32 = 2000.0;

@vertex
fn vs(@builtin(vertex_index) vi : u32, star : Instance) -> VSOut {
  // Two triangles per star, built from the vertex index: no vertex buffer for the quad itself.
  var corners = array<vec2f, 6>(
    vec2f(-1.0, -1.0), vec2f(1.0, -1.0), vec2f(-1.0, 1.0),
    vec2f(-1.0, 1.0), vec2f(1.0, -1.0), vec2f(1.0, 1.0),
  );
  let corner = corners[vi];

  let world = star.direction * SKY_RADIUS;
  var clip = u.view_proj * vec4f(world, 1.0);

  // A star keeps its pixel size whatever its distance: the offset is in pixels, scaled by w to survive the divide.
  let pixels = u.params.y * (0.7 + 0.6 * star.brightness);
  let px = corner * vec2f(2.0, 2.0) * vec2f(pixels / u.screen.x, pixels / u.screen.y) * clip.w;

  var out : VSOut;
  out.clip = vec4f(clip.xy + px, clip.z, clip.w);
  out.uv = corner;
  out.tint = star.tint;
  out.brightness = star.brightness;
  out.seed = star.seed;
  return out;
}

@fragment
fn fs(in : VSOut) -> @location(0) vec4f {
  let radius = length(in.uv);
  if (radius > 1.0) {
    discard;
  }
  // A soft round core with a faint halo.
  let core = smoothstep(1.0, 0.0, radius);
  let halo = pow(core, 4.0) * 0.6;
  let twinkle = 0.78 + 0.22 * sin(u.camera_pos.w * 1.35 + in.seed * 6.2831853);
  let light = (core * 0.75 + halo) * in.brightness * twinkle;
  return vec4f(in.tint * light, 1.0);
}
