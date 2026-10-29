@group(0) @binding(0) var<storage, read> source: array<u32>;
@group(0) @binding(1) var<storage, read> blurred: array<u32>;
@group(0) @binding(2) var<storage, read> masks: array<u32>;
@group(0) @binding(3) var<storage, read_write> output: array<u32>;
@group(0) @binding(4) var<storage, read> config: array<vec4<f32>, 5>;

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
    let r = u32(round(c.r * 255.0));
    let g = u32(round(c.g * 255.0));
    let b = u32(round(c.b * 255.0));
    let a = u32(round(clamp(alpha, 0.0, 1.0) * 255.0));
    return r | (g << 8u) | (b << 16u) | (a << 24u);
}

fn luma(color: vec3<f32>) -> f32 {
    return dot(color, vec3(0.299, 0.587, 0.114));
}

@compute @workgroup_size(8, 8)
fn portrait(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(u32(config[0].x), u32(config[0].y));
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    let index = id.y * size.x + id.x;
    let original = unpack_rgba(source[index]);
    let soft = unpack_rgba(blurred[index]);
    let packed_mask = masks[index];
    let skin = f32(packed_mask & 255u) / 255.0;
    let eyes = f32((packed_mask >> 8u) & 255u) / 255.0;
    let lips = f32((packed_mask >> 16u) & 255u) / 255.0;
    let teeth = f32(packed_mask >> 24u) / 255.0;
    let skin_settings = config[1];
    let eye_settings = config[2];
    let lip_settings = config[3];
    var color = original.rgb;

    let smooth_amount = skin_settings.x / 100.0 * skin;
    if smooth_amount > 0.0 {
        let texture = skin_settings.y / 100.0;
        let target_color = soft.rgb + (original.rgb - soft.rgb) * texture;
        color += (target_color - color) * smooth_amount;
    }
    let tone = skin_settings.z / 100.0 * skin;
    if tone > 0.0 {
        color += (config[4].rgb - color) * tone * 0.4;
    }
    let blemish = skin_settings.w / 100.0 * skin;
    if blemish > 0.0 && luma(color) < luma(soft.rgb) - 0.070588 {
        color += (soft.rgb - color) * blemish;
    }

    if eyes > 0.0 {
        let brighten = 1.0 + eye_settings.x / 100.0 * eyes * 0.35;
        let detail = eye_settings.y / 100.0 * eyes;
        color = (color + (color - soft.rgb) * detail * 1.5) * brighten;
    }
    let shift = u32(config[0].y / 12.0);
    if id.y >= shift && eye_settings.z > 0.0 {
        let dark_mask = masks[index - shift * size.x];
        let circles = f32((dark_mask >> 8u) & 255u) / 255.0 * skin;
        let amount = eye_settings.z / 100.0 * circles * 0.3;
        color += (vec3(1.0) - color) * amount;
    }
    if teeth > 0.0 && eye_settings.w > 0.0 {
        let gray = luma(color);
        let target_color = color * 0.35 + vec3(gray) * 0.65 + vec3(28.0 / 255.0);
        color += (target_color - color) * eye_settings.w / 100.0 * teeth;
    }
    if lips > 0.0 && lip_settings.x > 0.0 {
        color += (lip_settings.yzw - color) * lip_settings.x / 100.0 * lips;
    }

    output[index] = pack_rgba(color, original.a);
}
