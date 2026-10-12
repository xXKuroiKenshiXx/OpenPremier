// Rec.2100 display-referred delivery. Working HDR RGB is Rec.2020, signed gamma 2.4,
// premultiplied, with linear 1.0 = 203 cd/m² (BT.2408 graphics/reference white).
// HLG uses a 1000 cd/m² reference display and system gamma 1.2 (BT.2100).
@group(0) @binding(2) var tex0: texture_2d<f32>;
@group(0) @binding(3) var tex1: texture_2d<f32>;

fn pq_decode(v: vec3<f32>) -> vec3<f32> {
    let p = pow(clamp(v, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(1.0 / 78.84375));
    return 10000.0 * pow(max(p - vec3<f32>(0.8359375), vec3<f32>(0.0)) /
        max(vec3<f32>(18.8515625) - 18.6875 * p, vec3<f32>(1e-7)), vec3<f32>(1.0 / 0.1593017578125));
}
fn pq_encode(nits: vec3<f32>) -> vec3<f32> {
    let p = pow(clamp(nits / 10000.0, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(0.1593017578125));
    return pow((vec3<f32>(0.8359375) + 18.8515625 * p) / (vec3<f32>(1.0) + 18.6875 * p), vec3<f32>(78.84375));
}
fn hlg_scene(v: vec3<f32>) -> vec3<f32> {
    let e = clamp(v, vec3<f32>(0.0), vec3<f32>(1.0));
    return select((exp((e - vec3<f32>(0.55991073)) / 0.17883277) + vec3<f32>(0.28466892)) / 12.0,
        e * e / 3.0, e <= vec3<f32>(0.5));
}
fn hlg_signal(scene: vec3<f32>) -> vec3<f32> {
    let s = clamp(scene, vec3<f32>(0.0), vec3<f32>(1.0));
    return select(0.17883277 * log(max(12.0 * s - vec3<f32>(0.28466892), vec3<f32>(1e-7))) + vec3<f32>(0.55991073),
        sqrt(3.0 * s), s <= vec3<f32>(1.0 / 12.0));
}
fn from_2020(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(dot(c, vec3<f32>(1.660491, -0.587641, -0.072850)),
        dot(c, vec3<f32>(-0.124550, 1.132900, -0.008349)),
        dot(c, vec3<f32>(-0.018151, -0.100579, 1.118730)));
}
fn to_2020(c: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(dot(c, vec3<f32>(0.627404, 0.329283, 0.043313)),
        dot(c, vec3<f32>(0.069097, 0.919540, 0.011362)),
        dot(c, vec3<f32>(0.016391, 0.088013, 0.895595)));
}
// prm(0): transfer 0 gamma 2.4, 1 sRGB, 2 linear, 3 PQ, 4 HLG.
// prm(1): primaries 0 Rec.709/default, 1 Rec.2020, 2 P3-D65, 3 SMPTE-C.
@fragment
fn fs_input_hdr(in: VOut) -> @location(0) vec4<f32> {
    let sample = textureSampleLevel(tex0, samp, in.uv, 0.0);
    let c = unpremul(sample).rgb;
    var linear = signed_linear(c);
    if (prm(0) == 1.0) {
        linear = select(pow(max((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(0.0)), vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
    } else if (prm(0) == 2.0) { linear = c; }
    else if (prm(0) == 3.0) { linear = pq_decode(c) / 203.0; }
    else if (prm(0) == 4.0) {
        let scene = hlg_scene(c);
        var coefficients = vec3<f32>(0.2126, 0.7152, 0.0722);
        if (prm(1) == 1.0) { coefficients = vec3<f32>(0.2627, 0.6780, 0.0593); }
        let y = max(dot(scene, coefficients), 0.0);
        linear = scene * pow(y, 0.2) * (1000.0 / 203.0);
    }
    if (prm(1) == 2.0) {
        linear = vec3<f32>(dot(linear, vec3<f32>(1.224940, -0.224940, 0.0)),
            dot(linear, vec3<f32>(-0.042057, 1.042057, 0.0)),
            dot(linear, vec3<f32>(-0.019638, -0.078636, 1.098274)));
    } else if (prm(1) == 3.0) {
        linear = vec3<f32>(dot(linear, vec3<f32>(0.939542, 0.050181, 0.010277)),
            dot(linear, vec3<f32>(0.017772, 0.965793, 0.016435)),
            dot(linear, vec3<f32>(-0.001622, -0.004371, 1.005993)));
    }
    if (prm(1) != 1.0) { linear = to_2020(linear); }
    return vec4<f32>(signed_encoding(linear) * sample.a, sample.a);
}
// prm(0): 1 PQ, 2 HLG. prm(1): keep straight color + alpha, otherwise composite
// onto black in linear light before the transfer function (opaque delivery formats).
@fragment
fn fs_delivery_hdr(in: VOut) -> @location(0) vec4<f32> {
    let sample = textureSampleLevel(tex0, samp, in.uv, 0.0);
    var linear = signed_linear(unpremul(sample).rgb);
    var alpha = sample.a;
    if (prm(1) < 0.5) { linear *= sample.a; alpha = 1.0; }
    linear = max(linear, vec3<f32>(0.0));
    var signal = pq_encode(linear * 203.0);
    if (prm(0) == 2.0) {
        let display = linear * (203.0 / 1000.0);
        let y = max(dot(display, vec3<f32>(0.2627, 0.6780, 0.0593)), 1e-9);
        signal = hlg_signal(display / pow(y, 1.0 / 6.0));
    }
    return vec4<f32>(signal * alpha, alpha);
}

// Compatibility effects operate in the editor's extended Rec.709 encoding. Convert only
// around those passes; untouched HDR sources stay in Rec.2020 to avoid half-float
// cancellation of dark channels in extremely saturated wide-gamut colors.
@fragment
fn fs_working_709(in: VOut) -> @location(0) vec4<f32> {
    let c = textureSampleLevel(tex0, samp, in.uv, 0.0);
    return vec4<f32>(signed_encoding(from_2020(signed_linear(unpremul(c).rgb))) * c.a, c.a);
}

// An SDR thumbnail, not an HDR grading monitor. A luminance-preserving Reinhard shoulder
// maps 203-nit reference white to ~75% display signal. Out-of-709 gamut clips at display.
fn hdr_preview(c: vec4<f32>, opaque: bool) -> vec4<f32> {
    let linear = from_2020(signed_linear(unpremul(c).rgb));
    let visible = linear * select(1.0, c.a, opaque);
    let y = max(dot(visible, vec3<f32>(0.2126, 0.7152, 0.0722)), 0.0);
    let display = max(visible / (1.0 + y), vec3<f32>(0.0));
    return vec4<f32>(clamp(signed_encoding(display), vec3<f32>(0.0), vec3<f32>(1.0)), select(c.a, 1.0, opaque));
}
@fragment
fn fs_hdr_preview(in: VOut) -> @location(0) vec4<f32> {
    return hdr_preview(textureSampleLevel(tex0, samp, in.uv, 0.0), false);
}
@fragment
fn fs_hdr_present(in: VOut) -> @location(0) vec4<f32> {
    let c = textureSampleLevel(tex0, samp, in.uv, 0.0);
    if (prm(0) > 0.5) { return vec4<f32>(vec3<f32>(c.a), 1.0); }
    return hdr_preview(c, true);
}
