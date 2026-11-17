// Offscreen mirror of galaxy-web/src/main.js: same pipeline descriptors, same buffer layouts, same
// uniform block, same shaders. Renders one 512x512 frame on this machine's Vulkan device and reports
// whether the pipelines validate and the picture has content.
//
// This exists because a machine can be missing a browser with a WebGPU adapter; it proves the
// descriptors and the WGSL agree, on the real device. Run: cargo run --release [planet.wgsl] [starfield.wgsl] [out.png]

use std::borrow::Cow;

fn sphere(segments: usize, rings: usize) -> (Vec<f32>, Vec<u32>) {
    let mut vertices = Vec::with_capacity((segments + 1) * (rings + 1) * 6);
    for ring in 0..=rings {
        let polar = (ring as f32 / rings as f32) * std::f32::consts::PI;
        let (sin_polar, cos_polar) = polar.sin_cos();
        for segment in 0..=segments {
            let azimuth = (segment as f32 / segments as f32) * std::f32::consts::TAU;
            let x = sin_polar * azimuth.cos();
            let y = cos_polar;
            let z = sin_polar * azimuth.sin();
            vertices.extend_from_slice(&[x, y, z, x, y, z]);
        }
    }
    let mut indices = Vec::with_capacity(segments * rings * 6);
    for ring in 0..rings {
        for segment in 0..segments {
            let a = (ring * (segments + 1) + segment) as u32;
            let b = a + segments as u32 + 1;
            indices.extend_from_slice(&[a, a + 1, b, a + 1, b + 1, b]);
        }
    }
    (vertices, indices)
}

fn stars(count: usize) -> Vec<f32> {
    let palette: [[f32; 3]; 5] = [
        [0.72, 0.82, 1.0],
        [0.86, 0.91, 1.0],
        [1.0, 1.0, 1.0],
        [1.0, 0.94, 0.82],
        [1.0, 0.84, 0.66],
    ];
    // Deterministic PRNG (xorshift), so the check is repeatable.
    let mut seed = 0x9E3779B9u32;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32
    };
    let mut data = Vec::with_capacity(count * 8);
    for star in 0..count {
        let u = next();
        let y = 2.0 * u - 1.0;
        let radius = (1.0 - y * y).max(0.0).sqrt();
        let theta = next() * std::f32::consts::TAU;
        let mut direction = [radius * theta.cos(), y, radius * theta.sin()];
        let length = (direction[0] * direction[0] + direction[1] * direction[1] + direction[2] * direction[2]).sqrt();
        direction.iter_mut().for_each(|value| *value /= length);
        let magnitude = next().powf(2.6);
        let tint = palette[(next() * palette.len() as f32) as usize % palette.len()];
        data.extend_from_slice(&[
            direction[0],
            direction[1],
            direction[2],
            0.35 + magnitude * 1.9,
            tint[0],
            tint[1],
            tint[2],
            next(),
        ]);
        let _ = star;
    }
    data
}

fn perspective(field_of_view: f32, aspect: f32, near: f32, far: f32) -> [[f32; 4]; 4] {
    let f = 1.0 / (field_of_view / 2.0).tan();
    [
        [f / aspect, 0.0, 0.0, 0.0],
        [0.0, f, 0.0, 0.0],
        [0.0, 0.0, far / (near - far), -1.0],
        [0.0, 0.0, (far * near) / (near - far), 0.0],
    ]
}

fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [[f32; 4]; 4] {
    let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
    };
    let normalize = |a: [f32; 3]| {
        let l = dot(a, a).sqrt();
        [a[0] / l, a[1] / l, a[2] / l]
    };
    let z = normalize(sub(eye, target));
    let x = normalize(cross(up, z));
    let y = cross(z, x);
    [
        [x[0], y[0], z[0], 0.0],
        [x[1], y[1], z[1], 0.0],
        [x[2], y[2], z[2], 0.0],
        [-dot(x, eye), -dot(y, eye), -dot(z, eye), 1.0],
    ]
}

fn multiply(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut out = [[0.0f32; 4]; 4];
    for column in 0..4 {
        for row in 0..4 {
            let mut sum = 0.0;
            for k in 0..4 {
                sum += a[k][row] * b[column][k];
            }
            out[column][row] = sum;
        }
    }
    out
}

fn main() {
    let width = 512u32;
    let height = 512u32;
    let star_count = 2600usize;

    // Default to the page's own shaders; the positional arguments override them.
    let shader = |argument: usize, name: &str| {
        let path = match std::env::args().nth(argument) {
            Some(given) => std::path::PathBuf::from(given),
            None => std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src/shaders").join(name),
        };
        std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
    };
    let planet_source = shader(1, "planet.wgsl");
    let star_source = shader(2, "starfield.wgsl");

    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .expect("adapter");
    let info = adapter.get_info();
    println!("adapter: {} ({:?}, {:?})", info.name, info.backend, info.device_type);

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).expect("device");
    device.on_uncaptured_error(Box::new(|error| panic!("uncaptured: {error}")));

    let format = wgpu::TextureFormat::Bgra8Unorm;
    let samples = 4u32;

    println!("shaders: planet {} bytes, starfield {} bytes", planet_source.len(), star_source.len());
    let planet_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("planet.wgsl"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(&planet_source)),
    });
    let star_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("starfield.wgsl"),
        source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(&star_source)),
    });

    let uniform = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("uniforms"),
        size: 128,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("shared"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("shared"),
        layout: &layout,
        entries: &[wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() }],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("shared"),
        bind_group_layouts: &[&layout],
        push_constant_ranges: &[],
    });

    let planet_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("planet"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &planet_module,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: 24,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
                    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 12, shader_location: 1 },
                ],
            }],
        },
        fragment: Some(wgpu::FragmentState {
            module: &planet_module,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: Some(wgpu::Face::Back),
            front_face: wgpu::FrontFace::Ccw,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth24Plus,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
        multiview: None,
        cache: None,
    });

    let star_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("starfield"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &star_module,
            entry_point: Some("vs"),
            compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: 32,
                step_mode: wgpu::VertexStepMode::Instance,
                attributes: &[
                    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
                    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 12, shader_location: 1 },
                    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 16, shader_location: 2 },
                    wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32, offset: 28, shader_location: 3 },
                ],
            }],
        },
        fragment: Some(wgpu::FragmentState {
            module: &star_module,
            entry_point: Some("fs"),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::SrcAlpha,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::Zero,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth24Plus,
            depth_write_enabled: false,
            depth_compare: wgpu::CompareFunction::Always,
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState { count: samples, ..Default::default() },
        multiview: None,
        cache: None,
    });

    // Geometry, same shapes as the page.
    let (vertices, indices) = sphere(96, 48);
    let planet_vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("planet vertices"),
        size: (vertices.len() * 4) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&planet_vertex_buffer, 0, bytemuck::cast_slice(&vertices));
    let planet_index_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("planet indices"),
        size: (indices.len() * 4) as u64,
        usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&planet_index_buffer, 0, bytemuck::cast_slice(&indices));

    let star_data = stars(star_count);
    let star_buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("stars"),
        size: (star_count * 32) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&star_buffer, 0, bytemuck::cast_slice(&star_data));

    let msaa = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("msaa"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: samples,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: samples,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth24Plus,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let resolved = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("resolved"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    // The uniform block, written exactly as writeUniforms does (32 floats, 128 bytes).
    let aspect = width as f32 / height as f32;
    let yaw = 0.6f32;
    let pitch = 0.22f32;
    let distance = 3.15f32;
    let eye = [
        distance * pitch.cos() * yaw.sin(),
        distance * pitch.sin(),
        distance * pitch.cos() * yaw.cos(),
    ];
    let view_projection = multiply(perspective(45f32.to_radians(), aspect, 0.05, 6000.0), look_at(eye, [0.0, 0.0, 0.0], [0.0, 1.0, 0.0]));
    let sun = {
        let v = [-0.62f32, 0.35, 0.70];
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        [v[0] / l, v[1] / l, v[2] / l]
    };
    let mut uniforms = [0.0f32; 32];
    for column in 0..4 {
        for row in 0..4 {
            uniforms[column * 4 + row] = view_projection[column][row];
        }
    }
    uniforms[16..20].copy_from_slice(&[eye[0], eye[1], eye[2], 2.0]);
    uniforms[20..24].copy_from_slice(&[sun[0], sun[1], sun[2], 1.55]);
    uniforms[24..28].copy_from_slice(&[aspect, 2.2, 1.0, 1.0]);
    uniforms[28..32].copy_from_slice(&[width as f32, height as f32, 1.0, 1.0]);
    queue.write_buffer(&uniform, 0, bytemuck::cast_slice(&uniforms));

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame") });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("scene"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &msaa.create_view(&Default::default()),
                resolve_target: Some(&resolved.create_view(&Default::default())),
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.004, g: 0.008, b: 0.024, a: 1.0 }),
                    store: wgpu::StoreOp::Discard,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth.create_view(&Default::default()),
                depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_pipeline(&star_pipeline);
        pass.set_vertex_buffer(0, star_buffer.slice(..));
        pass.draw(0..6, 0..star_count as u32);
        pass.set_pipeline(&planet_pipeline);
        pass.set_vertex_buffer(0, planet_vertex_buffer.slice(..));
        pass.set_index_buffer(planet_index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..indices.len() as u32, 0, 0..1);
    }

    let bytes_per_row = width * 4;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: (bytes_per_row * height) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        resolved.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    queue.submit([encoder.finish()]);

    readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::Wait).expect("poll");
    let pixels = readback.slice(..).get_mapped_range();

    // How much of the frame is not the clear colour, and how bright it gets.
    let mut colors = std::collections::HashSet::new();  // quantised RGB buckets inside the disc
    let mut hue = (0i64, 0i64);                          // sum and count of blue-minus-green, for the spread
    let mut lit = 0u64;
    let mut brightest = 0u8;
    let mut sum = 0u64;
    for pixel in pixels.chunks_exact(4) {
        let value = pixel[0].max(pixel[1]).max(pixel[2]);
        sum += value as u64;
        brightest = brightest.max(value);
        if value > 12 {
            lit += 1;
            colors.insert((pixel[2] >> 4, pixel[1] >> 4, pixel[0] >> 4));
            hue.0 += pixel[0] as i64 - pixel[1] as i64;  // BGRA: blue minus green
            hue.1 += 1;
        }
    }
    for (label, x, y) in [("centre", width / 2, height / 2), ("left", width / 4, height / 2), ("top", width / 2, height / 4)] {
        let at = (y * bytes_per_row + x * 4) as usize;
        println!("{label} pixel BGRA = {:?}", &pixels[at..at + 4]);
    }
    let total = (width * height) as u64;
    println!("frame {width}x{height} samples={samples}: bright pixels {lit}/{total} ({:.1}%), peak {brightest}, mean {:.2}",
        100.0 * lit as f64 / total as f64,
        sum as f64 / total as f64);
    println!("surface: {} quantised colours over {lit} lit pixels, mean blue-green {:+.1}",
        colors.len(),
        hue.0 as f64 / hue.1.max(1) as f64);

    // A coarse luminance map, so the check is readable in a terminal as well as in the PNG.
    let ramp = b" .:-=+*#%@";
    let columns = 64usize;
    let rows = 32usize;
    let mut picture = String::new();
    for row in 0..rows {
        for column in 0..columns {
            let x = column * width as usize / columns;
            let y = row * height as usize / rows;
            let at = y * bytes_per_row as usize + x * 4;
            let value = pixels[at].max(pixels[at + 1]).max(pixels[at + 2]) as usize;
            picture.push(ramp[(value * (ramp.len() - 1) / 255).min(ramp.len() - 1)] as char);
        }
        picture.push('\n');
    }
    print!("{picture}");

    // A hue map: which channel wins. Ocean reads as "b", continents as "g"/"r", the limb glow as "B".
    println!("hue map ('b' ocean, 'g' green land, 'r' rock, '.' mid):");
    let mut hues = String::new();
    for row in 0..rows {
        for column in 0..columns {
            let x = column * width as usize / columns;
            let y = row * height as usize / rows;
            let at = y * bytes_per_row as usize + x * 4;
            let (blue, green, red) = (pixels[at], pixels[at + 1], pixels[at + 2]);
            let value = blue.max(green).max(red);
            let glyph = if value < 12 { ' ' } else if blue > green + 6 { 'b' } else if green > blue + 6 { 'g' } else if red > green + 6 { 'r' } else { '·' };
            hues.push(glyph);
        }
        hues.push('\n');
    }
    print!("{hues}");

    let out = std::env::args().nth(3).unwrap_or_else(|| "frame.png".to_string());
    let written = pixmap::write_png(std::path::Path::new(&out), &pixels, width, height, bytes_per_row as usize);
    println!("png: {} bytes", written);

    // A working frame: the planet disc covers the middle of the screen, it is bright, and its surface
    // is not one flat colour (which is what a broken normal or a constant noise field looks like).
    let ok = lit > total / 5 && brightest > 60 && colors.len() > 24;
    println!("RESULT: {}", if ok { "the pipelines validate and the frame has content" } else { "the frame is empty or black" });
    if !ok {
        std::process::exit(1);
    }
}

mod pixmap {
    use std::io::Write;

    fn crc32(data: &[u8]) -> u32 {
        let mut table = [0u32; 256];
        for (index, entry) in table.iter_mut().enumerate() {
            let mut value = index as u32;
            for _ in 0..8 {
                value = if value & 1 != 0 { 0xEDB8_8320 ^ (value >> 1) } else { value >> 1 };
            }
            *entry = value;
        }
        let mut crc = 0xFFFF_FFFFu32;
        for byte in data {
            crc = table[((crc ^ *byte as u32) & 0xFF) as usize] ^ (crc >> 8);
        }
        crc ^ 0xFFFF_FFFF
    }

    fn chunk(out: &mut Vec<u8>, kind: &[u8], body: &[u8]) {
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        let mut both = kind.to_vec();
        both.extend_from_slice(body);
        out.extend_from_slice(&both);
        out.extend_from_slice(&crc32(&both).to_be_bytes());
    }

    // BGRA rows in, RGBA PNG out. No dependencies: the check is about pixels, not about image libraries.
    pub fn write_png(path: &std::path::Path, bgra: &[u8], width: u32, height: u32, stride: usize) -> usize {
        let mut raw = Vec::with_capacity((width * 4 + 1) as usize * height as usize);
        for row in 0..height as usize {
            raw.push(0u8);
            for column in 0..width as usize {
                let at = row * stride + column * 4;
                raw.extend_from_slice(&[bgra[at + 2], bgra[at + 1], bgra[at], bgra[at + 3]]);
            }
        }
        // zlib stream, stored blocks: no compression needed for a check.
        let mut zlib = vec![0x78u8, 0x01];
        let mut rest = raw.as_slice();
        while !rest.is_empty() {
            let take = rest.len().min(65_535);
            let last = if take == rest.len() { 1u8 } else { 0u8 };
            zlib.push(last);
            zlib.extend_from_slice(&(take as u16).to_le_bytes());
            zlib.extend_from_slice(&(!(take as u16)).to_le_bytes());
            zlib.extend_from_slice(&rest[..take]);
            rest = &rest[take..];
        }
        let (mut a, mut b) = (1u32, 0u32);
        for byte in &raw {
            a = (a + *byte as u32) % 65_521;
            b = (b + a) % 65_521;
        }
        zlib.extend_from_slice(&((b << 16) | a).to_be_bytes());

        let mut png = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        let mut header = Vec::new();
        header.extend_from_slice(&width.to_be_bytes());
        header.extend_from_slice(&height.to_be_bytes());
        header.extend_from_slice(&[8, 6, 0, 0, 0]);
        chunk(&mut png, b"IHDR", &header);
        chunk(&mut png, b"IDAT", &zlib);
        chunk(&mut png, b"IEND", &[]);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut file = std::fs::File::create(path).expect("png");
        file.write_all(&png).expect("png write");
        png.len()
    }
}
