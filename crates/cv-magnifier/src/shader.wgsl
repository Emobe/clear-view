// Uniforms — 96 bytes (crop 16 bytes + color_mode 4 bytes + interp_mode 4 bytes + cursor 8 bytes
// + edge_threshold 4 bytes + pointer_kind 4 bytes + mask size 8 bytes + hotspot 8 bytes
// + size 4 bytes + outline 4 bytes + fill colour 16 bytes + outline colour 16 bytes).
// color_mode:     0=None, 1=Inverted, 2=Greyscale, 3=GreyscaleInverted
// interp_mode:    0=Bilinear, 1=Bicubic (Catmull-Rom), 2=Sharp bilinear, 3=cleanEdge
// cursor_x/y:     pointer position in output window pixels; 0xFFFFFFFF on both when the pointer
//                 is out of view (magnifier.rs CURSOR_HIDDEN), which draws nothing
// edge_threshold: cleanEdge colour similarity threshold (0–1), used by interp_mode 3 only
// pointer_kind:   0=nothing, 1=circle (shape unknown), 2=the pointer shape in mask_tex (ADR 0010)
// mask_w/h:       size of the pointer mask in mask_tex, in mask pixels
// hot_x/y:        the mask pixel that sits on the pointer position
// size:           output pixels per mask pixel (a multiple of the cursor's own size)
// outline:        outline width in output pixels, at most MAX_OUTLINE
// fill, outline_colour: straight RGBA, 0–1
//
// gfx.rs appends clean_edge.wgsl to this file before compiling, which defines sample_clean_edge.
struct Uniforms {
    src_x:          f32,
    src_y:          f32,
    src_w:          f32,
    src_h:          f32,
    color_mode:     u32,
    interp_mode:    u32,
    cursor_x:       u32,
    cursor_y:       u32,
    edge_threshold: f32,
    pointer_kind:   u32,
    mask_w:         u32,
    mask_h:         u32,
    hot_x:          u32,
    hot_y:          u32,
    size:           f32,
    outline:        f32,
    fill:           vec4<f32>,
    outline_colour: vec4<f32>,
}

@group(0) @binding(0) var<uniform> u:          Uniforms;
@group(0) @binding(1) var          frame_tex:  texture_2d<f32>;
@group(0) @binding(2) var          frame_samp: sampler;
@group(0) @binding(3) var          mask_tex:   texture_2d<f32>;

const CURSOR_NONE:    u32 = 0xFFFFFFFFu;
const POINTER_CIRCLE: u32 = 1u;
const POINTER_SHAPE:  u32 = 2u;
// Widest outline the dilation loop in `draw_pointer` covers, in output pixels.
const MAX_OUTLINE:    i32 = 8;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0)       uv:  vec2<f32>,
}

// Fullscreen quad — 6 vertices, no vertex buffer.
// DXGI data is top-down; wgpu texture UV (0,0) = top-left. No flip needed.
@vertex
fn vs(@builtin(vertex_index) vi: u32) -> VOut {
    // NDC positions
    var xy = array<vec2<f32>, 6>(
        vec2(-1.0, -1.0), vec2( 1.0, -1.0), vec2(-1.0,  1.0),
        vec2(-1.0,  1.0), vec2( 1.0, -1.0), vec2( 1.0,  1.0),
    );
    // Texture UVs: NDC y flips → v = 1 - (ndc_y*0.5 + 0.5)
    var uv = array<vec2<f32>, 6>(
        vec2(0.0, 1.0), vec2(1.0, 1.0), vec2(0.0, 0.0),
        vec2(0.0, 0.0), vec2(1.0, 1.0), vec2(1.0, 0.0),
    );
    var out: VOut;
    out.pos = vec4(xy[vi], 0.0, 1.0);
    out.uv  = uv[vi];
    return out;
}

// Catmull-Rom cubic basis weights for fractional offset t in [0, 1].
// Returns weights for pixels at offsets -1, 0, +1, +2 from the floor.
fn cubic_weights(t: f32) -> vec4<f32> {
    let t2 = t * t;
    let t3 = t2 * t;
    return vec4(
        -0.5*t3 + 1.0*t2 - 0.5*t,
         1.5*t3 - 2.5*t2 + 1.0,
        -1.5*t3 + 2.0*t2 + 0.5*t,
         0.5*t3 - 0.5*t2,
    );
}

// Bicubic (Catmull-Rom) sample of frame_tex at UV coord uv.
// Uses textureLoad for exact texel reads — no sampler-level filtering.
fn sample_bicubic(uv: vec2<f32>) -> vec4<f32> {
    let dims  = vec2<f32>(textureDimensions(frame_tex, 0));
    let pixel = uv * dims - 0.5;
    let ip    = vec2<i32>(floor(pixel));
    let frac  = fract(pixel);
    let wx    = cubic_weights(frac.x);
    let wy    = cubic_weights(frac.y);
    let imax  = vec2<i32>(dims) - vec2(1);

    var col = vec4(0.0);
    for (var j = 0i; j < 4i; j = j + 1) {
        var row_col = vec4(0.0);
        for (var i = 0i; i < 4i; i = i + 1) {
            let coord = clamp(ip + vec2(i - 1, j - 1), vec2(0), imax);
            row_col += wx[i] * textureLoad(frame_tex, coord, 0);
        }
        col += wy[j] * row_col;
    }
    return clamp(col, vec4(0.0), vec4(1.0));
}

// Sharp bilinear (ADR 0006): each texel is drawn flat, and only the last ~1 output pixel
// before the next texel is blended. `texel` is the position in texel units; `scale` is
// output pixels per texel. At scale 1 this is plain bilinear.
fn sample_sharp(texel: vec2<f32>, dims: vec2<f32>, scale: vec2<f32>) -> vec4<f32> {
    let base   = floor(texel);
    let centre = fract(texel) - 0.5;
    let region = 0.5 - 0.5 / scale;
    let f      = (centre - clamp(centre, -region, region)) * scale + 0.5;
    return textureSampleLevel(frame_tex, frame_samp, (base + f) / dims, 0.0);
}

// Coverage of the pointer mask at `m`, in mask pixels (mask pixel i covers [i, i+1)). 0 outside
// the mask, so the outline can grow past its edge without smearing the border pixels.
fn mask_coverage(m: vec2<f32>, dims: vec2<f32>) -> f32 {
    if m.x < 0.0 || m.y < 0.0 || m.x >= dims.x || m.y >= dims.y {
        return 0.0;
    }
    return textureSampleLevel(mask_tex, frame_samp, m / dims, 0.0).r;
}

// The pointer shape (ADR 0010) over `base`, for the output pixel at `pos` (its centre). The mask
// sits with its hotspot pixel on the pointer, `size` output pixels per mask pixel. The outline
// is the mask dilated by `outline` output pixels (the maximum over a disc of samples), drawn
// under the fill. Pixels the mask plus its outline can't reach return early.
fn draw_pointer(base: vec4<f32>, pos: vec2<f32>) -> vec4<f32> {
    let dims  = vec2<f32>(f32(u.mask_w), f32(u.mask_h));
    let hot   = vec2<f32>(f32(u.hot_x), f32(u.hot_y));
    let cur   = vec2<f32>(f32(u.cursor_x), f32(u.cursor_y));
    let size  = max(u.size, 0.01);
    let width = clamp(u.outline, 0.0, f32(MAX_OUTLINE));

    // Output pixels from the pointer; the box the mask and its outline can cover.
    let rel = pos - cur;
    if any(rel < -hot * size - (width + 1.0)) || any(rel > (dims - hot) * size + (width + 1.0)) {
        return base;
    }

    let m    = rel / size + hot;
    let fill = mask_coverage(m, dims);

    var grown = fill;
    let r = i32(ceil(width));
    for (var dy = -r; dy <= r; dy = dy + 1) {
        for (var dx = -r; dx <= r; dx = dx + 1) {
            let off = vec2<f32>(f32(dx), f32(dy));
            if length(off) <= width {
                grown = max(grown, mask_coverage(m + off / size, dims));
            }
        }
    }

    var rgb = mix(base.rgb, u.outline_colour.rgb, grown * u.outline_colour.a);
    rgb = mix(rgb, u.fill.rgb, fill * u.fill.a);
    return vec4(rgb, base.a);
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
    // Remap panel UV into the crop sub-region of the frame texture.
    let uv = in.uv * vec2(u.src_w, u.src_h) + vec2(u.src_x, u.src_y);

    // Derivatives must be taken in uniform control flow, so before any branch.
    let dims  = vec2<f32>(textureDimensions(frame_tex, 0));
    let texel = uv * dims;
    let scale = max(1.0 / max(fwidth(texel), vec2(1e-6)), vec2(1.0));

    var col: vec4<f32>;
    if u.interp_mode == 1u {
        col = sample_bicubic(uv);
    } else if u.interp_mode == 2u {
        col = sample_sharp(texel, dims, scale);
    } else if u.interp_mode == 3u {
        col = sample_clean_edge(texel, dims);
    } else {
        col = textureSample(frame_tex, frame_samp, uv);
    }

    // Apply colour filter.
    var out_col: vec4<f32>;
    if u.color_mode == 1u {
        // Inverted
        out_col = vec4(1.0 - col.r, 1.0 - col.g, 1.0 - col.b, col.a);
    } else if u.color_mode == 2u {
        // Greyscale (standard luminance weights)
        let lum = dot(col.rgb, vec3(0.299, 0.587, 0.114));
        out_col = vec4(lum, lum, lum, col.a);
    } else if u.color_mode == 3u {
        // Greyscale + Inverted
        let lum = dot(col.rgb, vec3(0.299, 0.587, 0.114));
        out_col = vec4(1.0 - lum, 1.0 - lum, 1.0 - lum, col.a);
    } else {
        out_col = col;
    }

    // The pointer goes on after the colour filter, so the filter leaves it alone.
    if u.cursor_x == CURSOR_NONE {
        return out_col;
    }
    if u.pointer_kind == POINTER_SHAPE {
        return draw_pointer(out_col, in.pos.xy);
    }
    if u.pointer_kind == POINTER_CIRCLE {
        // Shape unknown: white filled circle (r=10px) with 2px black border.
        let cur = vec2<f32>(f32(u.cursor_x), f32(u.cursor_y));
        let dist = length(in.pos.xy - cur);
        if dist < 10.0 {
            return vec4(1.0, 1.0, 1.0, 1.0);
        } else if dist < 12.0 {
            return vec4(0.0, 0.0, 0.0, 1.0);
        }
    }

    return out_col;
}
