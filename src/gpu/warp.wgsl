@group(0) @binding(0) var<storage, read> source: array<u32>;
@group(0) @binding(1) var<storage, read> grid: array<vec2<f32>>;
@group(0) @binding(2) var<storage, read_write> output: array<u32>;
@group(0) @binding(3) var<storage, read> config: array<vec4<f32>, 2>;

fn unpack_rgba(value: u32) -> vec4<f32> {
    return vec4<f32>(
        f32(value & 255u),
        f32((value >> 8u) & 255u),
        f32((value >> 16u) & 255u),
        f32(value >> 24u),
    ) / 255.0;
}

fn pack_rgba(color: vec3<f32>, alpha: f32) -> u32 {
    let c = clamp(color, vec3(0.0), vec3(1.0));
    return u32(round(c.r * 255.0))
        | (u32(round(c.g * 255.0)) << 8u)
        | (u32(round(c.b * 255.0)) << 16u)
        | (u32(round(clamp(alpha, 0.0, 1.0) * 255.0)) << 24u);
}

fn sample_source(point: vec2<f32>, size: vec2<u32>) -> vec4<f32> {
    let p = clamp(point, vec2(0.0), vec2<f32>(size) - vec2<f32>(1.0));
    let x0 = u32(floor(p.x));
    let y0 = u32(floor(p.y));
    let x1 = min(x0 + 1u, size.x - 1u);
    let y1 = min(y0 + 1u, size.y - 1u);
    let tx = p.x - f32(x0);
    let ty = p.y - f32(y0);
    let top = mix(unpack_rgba(source[y0 * size.x + x0]), unpack_rgba(source[y0 * size.x + x1]), tx);
    let bottom = mix(unpack_rgba(source[y1 * size.x + x0]), unpack_rgba(source[y1 * size.x + x1]), tx);
    return mix(top, bottom, ty);
}

@compute @workgroup_size(8, 8)
fn warp(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(u32(config[0].x), u32(config[0].y));
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    let cell = max(config[0].z, 1.0);
    let cols = u32(config[0].w);
    let rows = u32(config[1].x);
    let destination = vec2<f32>(f32(id.x), f32(id.y));
    let gx = clamp(destination.x / cell, 0.0, f32(cols - 1u));
    let gy = clamp(destination.y / cell, 0.0, f32(rows - 1u));
    let x0 = u32(floor(gx));
    let y0 = u32(floor(gy));
    let x1 = min(x0 + 1u, cols - 1u);
    let y1 = min(y0 + 1u, rows - 1u);
    let tx = gx - f32(x0);
    let ty = gy - f32(y0);
    let top = mix(grid[y0 * cols + x0], grid[y0 * cols + x1], tx);
    let bottom = mix(grid[y1 * cols + x0], grid[y1 * cols + x1], tx);
    let displacement = mix(top, bottom, ty);
    let sampled = sample_source(destination - displacement, size);
    output[id.y * size.x + id.x] = pack_rgba(sampled.rgb, sampled.a);
}
