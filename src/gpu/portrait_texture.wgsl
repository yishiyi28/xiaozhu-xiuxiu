@group(0) @binding(0) var source_texture: texture_2d<f32>;
@group(0) @binding(1) var blurred_texture: texture_2d<f32>;
@group(0) @binding(2) var mask_texture: texture_2d<f32>;
@group(0) @binding(3) var output_texture: texture_storage_2d<OUTPUT_FORMAT, write>;
@group(0) @binding(4) var<storage, read> config: array<vec4<f32>, 5>;

fn luma(color: vec3<f32>) -> f32 {
    return dot(color, vec3(0.299, 0.587, 0.114));
}

@compute @workgroup_size(8, 8)
fn portrait_texture(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<u32>(u32(config[0].x), u32(config[0].y));
    if id.x >= size.x || id.y >= size.y {
        return;
    }
    let coord = vec2<i32>(id.xy);
    let original = textureLoad(source_texture, coord, 0);
    let soft = textureLoad(blurred_texture, coord, 0);
    let mask = textureLoad(mask_texture, coord, 0);
    let skin = mask.r;
    let eyes = mask.g;
    let lips = mask.b;
    let teeth = mask.a;
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
        let dark_coord = vec2<i32>(i32(id.x), i32(id.y - shift));
        let dark_mask = textureLoad(mask_texture, dark_coord, 0);
        let circles = dark_mask.g * skin;
        color += (vec3(1.0) - color) * eye_settings.z / 100.0 * circles * 0.3;
    }
    if teeth > 0.0 && eye_settings.w > 0.0 {
        let gray = luma(color);
        let target_color = color * 0.35 + vec3(gray) * 0.65 + vec3(28.0 / 255.0);
        color += (target_color - color) * eye_settings.w / 100.0 * teeth;
    }
    if lips > 0.0 && lip_settings.x > 0.0 {
        color += (lip_settings.yzw - color) * lip_settings.x / 100.0 * lips;
    }
    textureStore(output_texture, coord, vec4(clamp(color, vec3(0.0), vec3(1.0)), original.a));
}
