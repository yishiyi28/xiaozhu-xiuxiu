@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var output: texture_storage_2d<OUTPUT_FORMAT, write>;
@group(0) @binding(2) var<storage, read> config: array<vec4<f32>>;
// Untouched source, needed by the two-pass sharpening filters.
@group(0) @binding(3) var extra: texture_2d<f32>;

// Eight-bit documents round at every read to match the CPU reference. High
// depth documents only round at the source's native 16-bit grid.
const HIGH_PRECISION: bool = HIGH_PRECISION_FLAG;
const PRECISION_SCALE: f32 = PRECISION_SCALE_VALUE;

fn quantized(value: vec4<f32>) -> vec4<f32> {
    return floor(clamp(value, vec4(0.0), vec4(1.0)) * PRECISION_SCALE + 0.5)
        / PRECISION_SCALE;
}

// Match the CPU filter boundary: a straight-alpha backdrop at document depth.
fn pixel(position: vec2<i32>) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(source));
    return quantized(textureLoad(source, clamp(position, vec2(0), size - 1), 0));
}

fn premultiplied(position: vec2<i32>) -> vec4<f32> {
    let p = pixel(position);
    return vec4(p.rgb * p.a, p.a);
}

// The untouched input, quantized the same way as `pixel`.
fn extra_pixel(position: vec2<i32>) -> vec4<f32> {
    let size = vec2<i32>(textureDimensions(extra));
    return quantized(textureLoad(extra, clamp(position, vec2(0), size - 1), 0));
}

fn sample_premultiplied(point: vec2<f32>) -> vec4<f32> {
    let size = vec2<f32>(textureDimensions(source));
    if any(point < vec2(0.0)) || any(point >= size) { return vec4(0.0); }
    let low = vec2<i32>(floor(point - 0.5));
    let fraction = fract(point - 0.5);
    let a = mix(premultiplied(low), premultiplied(low + vec2(1, 0)), fraction.x);
    let b = mix(premultiplied(low + vec2(0, 1)), premultiplied(low + vec2(1, 1)), fraction.x);
    return mix(a, b, fraction.y);
}

fn noise(position: vec2<u32>, seed: u32) -> f32 {
    var value = position.x * 374761393u + position.y * 668265263u + seed;
    value = (value ^ (value >> 13u)) * 1274126177u;
    value ^= value >> 16u;
    return f32(value) / 4294967295.0 * 2.0 - 1.0;
}

@compute @workgroup_size(8, 8)
fn filter_layer(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(source);
    if any(id.xy >= size) { return; }
    let position = vec2<i32>(id.xy);
    let settings = config[0];
    let mode = u32(settings.x);
    // Accumulating modes start from zero; each branch assigns `result`.
    var result = vec4(0.0);
    if mode <= 1u {
        let radius = i32(settings.y);
        let direction = select(vec2(0, 1), vec2(1, 0), mode == 0u);
        for (var k = -radius; k <= radius; k++) {
            let p = clamp(position + direction * k, vec2(0), vec2<i32>(size) - 1);
            var value = textureLoad(source, p, 0);
            if mode == 0u {
                if HIGH_PRECISION {
                    value = premultiplied(p);
                } else {
                    // Gaussian Blur historically truncates premultiplied bytes.
                    value = floor(premultiplied(p) * 255.0 + 0.00001) / 255.0;
                }
            }
            result += value * config[u32(k + radius) + 1u].x;
        }
        if mode == 1u {
            if HIGH_PRECISION {
                // Float path mirrors `blur_float`: divide the premultiplied
                // backlog by alpha and keep the result in linear f32.
                result = vec4(clamp(result.rgb / max(result.a, 0.000001),
                    vec3(0.0), vec3(1.0)), result.a);
            } else {
                result = floor(clamp(result, vec4(0.0), vec4(1.0)) * 255.0 + 0.5);
                result = vec4(select(result.rgb, floor(result.rgb * 255.0 / max(result.a, 1.0)), result.a > 0.0), result.a) / 255.0;
            }
        }
    } else if mode == 2u {
        let steps = u32(settings.y);
        for (var i = 0u; i < steps; i++) {
            let offset = (f32(i) + 0.5) / f32(steps) - 0.5;
            result += sample_premultiplied(vec2<f32>(id.xy) + 0.5 + offset * settings.zw);
        }
        result = vec4(result.rgb / max(result.a, 0.000001), result.a / f32(steps));
    } else if mode == 3u {
        result = pixel(position);
        for (var c = 0u; c < 3u; c++) {
            result[c] += noise(id.xy, 3187u + select(c * 12345u, 0u, settings.z != 0.0)) * settings.y;
        }
    } else if mode == 4u {
        let uv = (vec2<f32>(id.xy) + 0.5) / vec2<f32>(size) * 2.0 - 1.0;
        let radius = dot(uv, uv);
        let k = 1.0 + settings.y * radius / 100.0;
        result = sample_premultiplied((uv * k + 1.0) * 0.5 * vec2<f32>(size));
        result = vec4(result.rgb / max(result.a, 0.000001) * (1.0 - settings.z * radius * 0.005), result.a);
    } else if mode == 5u || mode == 6u {
        // Vertical half of the blur; the untouched source lives in `extra`.
        let radius = i32(settings.y);
        var accumulated = vec4(0.0);
        for (var k = -radius; k <= radius; k++) {
            let p = clamp(position + vec2(0, k), vec2(0), vec2<i32>(size) - 1);
            // Keep the intermediate in float, like `blur_straight` does.
            accumulated += textureLoad(source, p, 0) * config[u32(k + radius) + 1u].x;
        }
        // The 8-bit CPU reference rounds the premultiplied blur to bytes and
        // divides alpha back out with integer truncation. The float path keeps
        // the whole blur (and the difference test) in f32.
        var blurred = vec3(0.0);
        if HIGH_PRECISION {
            blurred = clamp(accumulated.rgb / max(accumulated.a, 0.000001),
                vec3(0.0), vec3(1.0));
        } else {
            let bytes = floor(clamp(accumulated, vec4(0.0), vec4(1.0)) * 255.0 + 0.5);
            blurred = min(floor(bytes.rgb * 255.0 / max(bytes.a, 1.0)), vec3(255.0)) / 255.0;
        }
        let original = extra_pixel(position);
        if mode == 5u {
            let difference = original.rgb - blurred;
            var rgb = original.rgb;
            for (var c = 0u; c < 3u; c++) {
                if HIGH_PRECISION {
                    if abs(difference[c]) > settings.w / 255.0 {
                        rgb[c] = original.rgb[c] + difference[c] * settings.z / 100.0;
                    }
                } else if abs(difference[c]) * 255.0 > settings.w {
                    rgb[c] = original.rgb[c] + difference[c] * settings.z / 100.0;
                }
            }
            // The composite pass expects straight alpha, like every other mode.
            result = vec4(clamp(rgb, vec3(0.0), vec3(1.0)), original.a);
        } else {
            let rgb = clamp(vec3(0.5) + original.rgb - blurred, vec3(0.0), vec3(1.0));
            result = vec4(rgb, original.a);
        }
    } else if mode == 7u {
        let p = pixel(position);
        let levels = max(settings.y, 2.0);
        let step = floor(p.rgb * levels);
        let rgb = quantized(vec4(clamp(step / (levels - 1.0),
            vec3(0.0), vec3(1.0)), 1.0)).rgb;
        result = vec4(rgb, p.a);
    } else if mode == 8u {
        let p = pixel(position);
        let luma = dot(p.rgb * 255.0, vec3(0.2126, 0.7152, 0.0722));
        let value = select(0.0, 1.0, luma >= settings.y);
        result = vec4(vec3(value), p.a);
    } else if mode == 9u {
        let cell = max(settings.y, 1.0);
        let coordinate = vec2<f32>(id.xy);
        let block = floor(coordinate / cell);
        let center = min(block * cell + cell * 0.5, vec2<f32>(size) - vec2<f32>(0.5));
        let sampled = sample_premultiplied(center);
        result = vec4(sampled.rgb / max(sampled.a, 0.000001), sampled.a);
    } else if mode == 10u {
        let uv = (vec2<f32>(id.xy) + 0.5) / vec2<f32>(size) * 2.0 - 1.0;
        let radius = sqrt(dot(uv, uv) * 0.5);
        let midpoint = settings.z / 100.0;
        let softness = max(settings.w / 100.0, 0.0001);
        let t = clamp((radius - midpoint) / softness, 0.0, 1.0);
        let falloff = t * t * (3.0 - 2.0 * t);
        let factor = 1.0 - settings.y / 100.0 * falloff;
        let p = pixel(position);
        result = vec4(clamp(p.rgb * factor, vec3(0.0), vec3(1.0)), p.a);
    }
    textureStore(output, position, result);
}
