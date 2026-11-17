// The planet: a procedural world lit by one sun, with an ocean, continents, ice caps,
// a specular sea and an atmospheric rim. No textures: the surface comes from 3D value noise,
// so the demo ships as source only.

struct Uniforms {
  view_proj  : mat4x4f,
  camera_pos : vec4f,
  sun_dir    : vec4f,
  params     : vec4f,
  screen     : vec4f,
};
@group(0) @binding(0) var<uniform> u : Uniforms;

struct VSOut {
  @builtin(position) clip : vec4f,
  @location(0) normal : vec3f,
  @location(1) world : vec3f,
};

@vertex
fn vs(@location(0) position : vec3f, @location(1) normal : vec3f) -> VSOut {
  var out : VSOut;
  out.world = position * u.params.z;
  out.clip = u.view_proj * vec4f(out.world, 1.0);
  out.normal = normal;
  return out;
}

fn hash3(p : vec3f) -> f32 {
  var q = fract(p * 0.3183099 + vec3f(0.71, 0.113, 0.419));
  q *= 17.0;
  return fract(q.x * q.y * q.z * (q.x + q.y + q.z));
}

fn value_noise(p : vec3f) -> f32 {
  let i = floor(p);
  let f = fract(p);
  let w = f * f * (3.0 - 2.0 * f);
  let c000 = hash3(i + vec3f(0.0, 0.0, 0.0));
  let c100 = hash3(i + vec3f(1.0, 0.0, 0.0));
  let c010 = hash3(i + vec3f(0.0, 1.0, 0.0));
  let c110 = hash3(i + vec3f(1.0, 1.0, 0.0));
  let c001 = hash3(i + vec3f(0.0, 0.0, 1.0));
  let c101 = hash3(i + vec3f(1.0, 0.0, 1.0));
  let c011 = hash3(i + vec3f(0.0, 1.0, 1.0));
  let c111 = hash3(i + vec3f(1.0, 1.0, 1.0));
  let x00 = mix(c000, c100, w.x);
  let x10 = mix(c010, c110, w.x);
  let x01 = mix(c001, c101, w.x);
  let x11 = mix(c011, c111, w.x);
  return mix(mix(x00, x10, w.y), mix(x01, x11, w.y), w.z);
}

fn fbm(p_in : vec3f) -> f32 {
  var p = p_in;
  var sum = 0.0;
  var amplitude = 0.5;
  for (var octave = 0; octave < 5; octave++) {
    sum += amplitude * value_noise(p);
    p *= 2.03;
    amplitude *= 0.5;
  }
  return sum;
}

@fragment
fn fs(in : VSOut) -> @location(0) vec4f {
  let n = normalize(in.normal);
  let to_eye = normalize(u.camera_pos.xyz - in.world);
  let to_sun = normalize(u.sun_dir.xyz);

  // Continents: two noise fields, so coastlines are not one blob shape everywhere. The 5-octave
  // sum has a mean near 0.47 and a spread near 0.09, which is what puts the sea level at 0.44.
  let broad = fbm(n * 3.0 + vec3f(3.7, 1.1, 9.4));
  let detail = fbm(n * 8.0 - vec3f(1.9, 4.3, 2.6));
  let height = broad * 0.78 + detail * 0.22;
  let sea_level = 0.44;
  let land = smoothstep(sea_level - 0.012, sea_level + 0.012, height);

  // Ocean: deeper away from the shelf, with the shelf slightly brighter.
  let shelf = smoothstep(sea_level - 0.16, sea_level, height);
  let depth = 1.0 - shelf;
  let deep = vec3f(0.012, 0.045, 0.115);
  let shallow = vec3f(0.045, 0.20, 0.34);
  var albedo = mix(shallow, deep, depth);

  // Land: lowlands green, highlands brown to grey, snow on the peaks.
  let rock = smoothstep(0.52, 0.62, height);
  let peak = smoothstep(0.64, 0.76, height);
  let lowland = mix(vec3f(0.115, 0.24, 0.095), vec3f(0.16, 0.30, 0.12), detail);
  let highland = vec3f(0.30, 0.24, 0.16);
  let snow = vec3f(0.88, 0.91, 0.95);
  albedo = mix(albedo, mix(lowland, highland, rock), land);
  albedo = mix(albedo, snow, peak * land * 0.75);

  // Ice caps, softened at the edge.
  let polar = smoothstep(0.80, 0.94, abs(n.y) + (detail - 0.5) * 0.06);
  albedo = mix(albedo, vec3f(0.86, 0.91, 0.97), polar * 0.9);

  let ndl = dot(n, to_sun);
  let lambert = clamp(ndl, 0.0, 1.0);
  // A wide terminator: the sun is a disc, not a point.
  let terminator = smoothstep(-0.10, 0.26, ndl);
  let sun = vec3f(1.0, 0.96, 0.90);

  // The sea is glossy: a tight specular highlight over the lit hemisphere.
  let ocean = (1.0 - land) * (1.0 - polar);
  let half_vector = normalize(to_sun + to_eye);
  let specular = pow(clamp(dot(n, half_vector), 0.0, 1.0), 96.0) * ocean * terminator;

  var color = albedo * sun * (lambert * terminator + 0.05);
  color += sun * specular * 1.7;

  // Atmosphere: a bright limb on the sunlit side, a thin one in shadow.
  let ndv = clamp(dot(n, to_eye), 0.0, 1.0);
  let rim = pow(1.0 - ndv, 3.4);
  let lit = smoothstep(-0.35, 0.55, ndl);
  let sky = vec3f(0.22, 0.52, 1.0);
  color += sky * rim * (0.10 + 0.85 * lit);

  // Exposure then a gentle filmic curve, so the sunlit limb rolls off instead of clipping.
  color *= u.sun_dir.w;
  color = color / (color + vec3f(1.0));
  color = pow(color, vec3f(1.0 / 2.2));
  return vec4f(color, 1.0);
}
