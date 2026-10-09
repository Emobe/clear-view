// cleanEdge pixel art scaling, ported to WGSL for clear-view (roadmap 1.9, ADR 0006).
//
// Source: libretro slang-shaders include/cleanEdge.inc at commit 94a7736 (ported to slang by
// hunterk), based on torcado's shadertoy version:
// https://gist.githubusercontent.com/torcado194/e2794f5a4b22049ac0a41f972d14c329/raw/3221e8d66e885d6221db2bd4d4d4fd7546254cd6/cleanEdge-shadertoy.glsl
//
// Appended to shader.wgsl by gfx.rs; uses its `u` uniforms and `frame_tex`.
// Changes from the source:
// - SLOPE and CLEANUP are always on; rotation, zoom, offset and the debug toggle are removed.
// - Line width is fixed at the libretro default 1.0; the similarity threshold is `u.edge_threshold`.
// - Texels are read with textureLoad at floor(texel) + offset, clamped to the frame. The source
//   samples at ceil(px)/size with a nearest sampler, which is the same image shifted one texel;
//   floor keeps it aligned with the other interpolation modes.
// - The quadrant is step(0.5, local), not round(local): WGSL rounds 0.5 to even (0), GLSL leaves it
//   to the implementation.
// - Alpha is forced to 1 on load; the desktop capture's alpha channel carries no meaning.
// - `u` (the up neighbour) is named `up`, because `u` is the uniform block; `this` is reserved in WGSL.
// - similar5 is dropped (unused).

/*** MIT LICENSE
Copyright (c) 2022 torcado

Permission is hereby granted, free of charge, to any person
obtaining a copy of this software and associated documentation
files (the "Software"), to deal in the Software without
restriction, including without limitation the rights to use,
copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the
Software is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice shall be
included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES
OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT
HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY,
WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING
FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR
OTHER DEALINGS IN THE SOFTWARE.
***/

// The colour with the highest priority. Other colours are ranked by distance to it
// to decide which one wins where slices overlap.
const CE_HIGHEST: vec3<f32> = vec3(1.0, 1.0, 1.0);
// Line width 1.0; with SLOPE the source clamps it to [0.44, 1.142], so it stays 1.0.
const CE_LINE_WIDTH: f32 = 1.0;

fn ce_similar(col1: vec4<f32>, col2: vec4<f32>) -> bool {
    return (col1.a == 0.0 && col2.a == 0.0) || distance(col1, col2) <= u.edge_threshold;
}

// The inner check should ideally compare all permutations, but this is good enough and faster.
fn ce_similar3(col1: vec4<f32>, col2: vec4<f32>, col3: vec4<f32>) -> bool {
    return ce_similar(col1, col2) && ce_similar(col2, col3);
}

fn ce_similar4(col1: vec4<f32>, col2: vec4<f32>, col3: vec4<f32>, col4: vec4<f32>) -> bool {
    return ce_similar(col1, col2) && ce_similar(col2, col3) && ce_similar(col3, col4);
}

fn ce_higher(this_col: vec4<f32>, other_col: vec4<f32>) -> bool {
    if ce_similar(this_col, other_col) { return false; }
    if this_col.a == other_col.a {
        return distance(this_col.rgb, CE_HIGHEST) < distance(other_col.rgb, CE_HIGHEST);
    }
    return this_col.a > other_col.a;
}

// Colour distance.
fn ce_cd(col1: vec4<f32>, col2: vec4<f32>) -> f32 {
    return distance(col1, col2);
}

fn ce_dist_to_line(test_pt: vec2<f32>, pt1: vec2<f32>, pt2: vec2<f32>, dir: vec2<f32>) -> f32 {
    let line_dir   = pt2 - pt1;
    let perp_dir   = vec2(line_dir.y, -line_dir.x);
    let dir_to_pt1 = pt1 - test_pt;
    return select(-1.0, 1.0, dot(perp_dir, dir) > 0.0) * dot(normalize(perp_dir), dir_to_pt1);
}

// The source's `dist <= 0.0 ? ((cd(c,a) <= cd(c,b)) ? a : b) : vec4(-1.0)`.
fn ce_pick(dist: f32, c: vec4<f32>, a: vec4<f32>, b: vec4<f32>) -> vec4<f32> {
    if dist > 0.0 { return vec4(-1.0); }
    return select(b, a, ce_cd(c, a) <= ce_cd(c, b));
}

// Based on the down-forward direction. Returns the slice colour, or -1 when there is no slice.
fn ce_slice_dist(
    point_in: vec2<f32>, main_dir: vec2<f32>, point_dir: vec2<f32>,
    up: vec4<f32>, uf: vec4<f32>, uff: vec4<f32>,
    b: vec4<f32>, c: vec4<f32>, f: vec4<f32>, ff: vec4<f32>,
    db: vec4<f32>, d: vec4<f32>, df: vec4<f32>, dff: vec4<f32>,
    ddb: vec4<f32>, dd: vec4<f32>, ddf: vec4<f32>,
) -> vec4<f32> {
    let lw = CE_LINE_WIDTH;
    let point = main_dir * (point_in - 0.5) + 0.5; // flip point

    // Edge detection
    let dist_against = 4.0 * ce_cd(f, d) + ce_cd(uf, c) + ce_cd(c, db) + ce_cd(ff, df) + ce_cd(df, dd);
    let dist_towards = 4.0 * ce_cd(c, df) + ce_cd(up, f) + ce_cd(f, dff) + ce_cd(b, d) + ce_cd(d, ddf);
    var should_slice = (dist_against < dist_towards)
        || ((dist_against < dist_towards + 0.001) && !ce_higher(c, f)); // equivalent edges edge case
    if ce_similar4(f, d, b, up) && ce_similar3(uf, df, db) && !ce_similar(c, f) { // checkerboard edge case
        should_slice = false;
    }
    if !should_slice { return vec4(-1.0); }

    var dist = 1.0;
    var flip = false;
    let center = vec2(0.5, 0.5);

    // Lower shallow 2:1 slant
    if ce_similar3(f, d, db) && !ce_similar3(f, d, b) && !ce_similar(uf, db) {
        if !(ce_similar(c, df) && ce_higher(c, f)) { // otherwise single pixel wide diagonal, don't flip
            // priority edge cases
            if ce_higher(c, f) { flip = true; }
            if ce_similar(up, f) && !ce_similar(c, df) && !ce_higher(c, up) { flip = true; }
        }

        // midpoints of neighbour two-pixel groupings
        if flip {
            dist = lw - ce_dist_to_line(point, center + vec2(1.5, -1.0) * point_dir, center + vec2(-0.5, 0.0) * point_dir, -point_dir);
        } else {
            dist = ce_dist_to_line(point, center + vec2(1.5, 0.0) * point_dir, center + vec2(-0.5, 1.0) * point_dir, point_dir);
        }

        // cleanup slant transitions (shallow)
        if !flip && ce_similar(c, uf) && !(ce_similar3(c, uf, uff) && !ce_similar3(c, uf, ff) && !ce_similar(d, uff)) {
            let dist2 = ce_dist_to_line(point, center + vec2(2.0, -1.0) * point_dir, center + vec2(0.0, 1.0) * point_dir, point_dir);
            dist = min(dist, dist2);
        }

        dist -= lw / 2.0;
        return ce_pick(dist, c, f, d);
    }

    // Forward steep 2:1 slant
    if ce_similar3(uf, f, d) && !ce_similar3(up, f, d) && !ce_similar(uf, db) {
        if !(ce_similar(c, df) && ce_higher(c, d)) { // otherwise single pixel wide diagonal, don't flip
            // priority edge cases
            if ce_higher(c, d) { flip = true; }
            if ce_similar(b, d) && !ce_similar(c, df) && !ce_higher(c, d) { flip = true; }
        }

        // midpoints of neighbour two-pixel groupings
        if flip {
            dist = lw - ce_dist_to_line(point, center + vec2(0.0, -0.5) * point_dir, center + vec2(-1.0, 1.5) * point_dir, -point_dir);
        } else {
            dist = ce_dist_to_line(point, center + vec2(1.0, -0.5) * point_dir, center + vec2(0.0, 1.5) * point_dir, point_dir);
        }

        // cleanup slant transitions (steep)
        if !flip && ce_similar(c, db) && !(ce_similar3(c, db, ddb) && !ce_similar3(c, db, dd) && !ce_similar(f, ddb)) {
            let dist2 = ce_dist_to_line(point, center + vec2(1.0, 0.0) * point_dir, center + vec2(-1.0, 2.0) * point_dir, point_dir);
            dist = min(dist, dist2);
        }

        dist -= lw / 2.0;
        return ce_pick(dist, c, f, d);
    }

    // 45 degree diagonal
    if ce_similar(f, d) {
        if ce_similar(c, df) && ce_higher(c, f) { // single pixel diagonal along neighbours, don't flip
            if !ce_similar(c, dd) && !ce_similar(c, ff) { // line against triple colour stripe edge case
                flip = true;
            }
        } else {
            // priority edge cases
            if ce_higher(c, f) { flip = true; }
            if !ce_similar(c, b) && ce_similar4(b, f, d, up) { flip = true; }
        }
        // single pixel 2:1 slope, don't flip
        if ((ce_similar(f, db) && ce_similar3(up, f, df)) || (ce_similar(uf, d) && ce_similar3(b, d, df))) && !ce_similar(c, df) {
            flip = true;
        }

        if flip {
            // midpoints of own diagonal pixels
            dist = lw - ce_dist_to_line(point, center + vec2(1.0, -1.0) * point_dir, center + vec2(-1.0, 1.0) * point_dir, -point_dir);
        } else {
            // midpoints of corner neighbour pixels
            dist = ce_dist_to_line(point, center + vec2(1.0, 0.0) * point_dir, center + vec2(0.0, 1.0) * point_dir, point_dir);
        }

        // cleanup slant transitions
        if !flip && ce_similar3(c, uf, uff) && !ce_similar3(c, uf, ff) && !ce_similar(d, uff) { // shallow
            let dist2 = ce_dist_to_line(point, center + vec2(1.5, 0.0) * point_dir, center + vec2(-0.5, 1.0) * point_dir, point_dir);
            dist = max(dist, dist2);
        }
        if !flip && ce_similar3(ddb, db, c) && !ce_similar3(dd, db, c) && !ce_similar(ddb, f) { // steep
            let dist2 = ce_dist_to_line(point, center + vec2(1.0, -0.5) * point_dir, center + vec2(0.0, 1.5) * point_dir, point_dir);
            dist = max(dist, dist2);
        }

        dist -= lw / 2.0;
        return ce_pick(dist, c, f, d);
    }

    // Far corner of shallow slant
    if ce_similar3(ff, df, d) && !ce_similar3(ff, df, c) && !ce_similar(uff, d) {
        if !(ce_similar(f, dff) && ce_higher(f, ff)) { // otherwise single pixel wide diagonal, don't flip
            // priority edge cases
            if ce_higher(f, ff) { flip = true; }
            if ce_similar(uf, ff) && !ce_similar(f, dff) && !ce_higher(f, uf) { flip = true; }
        }

        // midpoints of neighbour two-pixel groupings
        if flip {
            dist = lw - ce_dist_to_line(point, center + vec2(1.5 + 1.0, -1.0) * point_dir, center + vec2(-0.5 + 1.0, 0.0) * point_dir, -point_dir);
        } else {
            dist = ce_dist_to_line(point, center + vec2(1.5 + 1.0, 0.0) * point_dir, center + vec2(-0.5 + 1.0, 1.0) * point_dir, point_dir);
        }

        dist -= lw / 2.0;
        return ce_pick(dist, f, ff, df);
    }

    // Far corner of steep slant
    if ce_similar3(f, df, dd) && !ce_similar3(c, df, dd) && !ce_similar(f, ddb) {
        if !(ce_similar(d, ddf) && ce_higher(d, dd)) { // otherwise single pixel wide diagonal, don't flip
            // priority edge cases
            if ce_higher(d, dd) { flip = true; }
            if ce_similar(db, dd) && !ce_similar(d, ddf) && !ce_higher(d, dd) { flip = true; }
        }

        // midpoints of neighbour two-pixel groupings
        if flip {
            dist = lw - ce_dist_to_line(point, center + vec2(0.0, -0.5 + 1.0) * point_dir, center + vec2(-1.0, 1.5 + 1.0) * point_dir, -point_dir);
        } else {
            dist = ce_dist_to_line(point, center + vec2(1.0, -0.5 + 1.0) * point_dir, center + vec2(0.0, 1.5 + 1.0) * point_dir, point_dir);
        }

        dist -= lw / 2.0;
        return ce_pick(dist, d, df, dd);
    }

    return vec4(-1.0);
}

// Texel at `base + off * point_dir`, clamped to the frame, alpha forced to 1.
fn ce_load(base: vec2<i32>, off: vec2<f32>, point_dir: vec2<f32>, imax: vec2<i32>) -> vec4<f32> {
    let coord = clamp(base + vec2<i32>(off * point_dir), vec2(0), imax);
    return vec4(textureLoad(frame_tex, coord, 0).rgb, 1.0);
}

// cleanEdge sample at `texel` (position in texel units; texel i covers [i, i + 1)).
fn sample_clean_edge(texel: vec2<f32>, dims: vec2<f32>) -> vec4<f32> {
    let local = fract(texel);
    let base  = vec2<i32>(floor(texel));
    let imax  = vec2<i32>(dims) - vec2(1);

    let pd = step(vec2(0.5), local) * 2.0 - 1.0;

    // Neighbour texels: Up, Down, Forward and Back, relative to the quadrant of the
    // current position within the texel.
    let uub = ce_load(base, vec2(-1.0, -2.0), pd, imax);
    let uu  = ce_load(base, vec2( 0.0, -2.0), pd, imax);
    let uuf = ce_load(base, vec2( 1.0, -2.0), pd, imax);

    let ubb = ce_load(base, vec2(-2.0, -2.0), pd, imax);
    let ub  = ce_load(base, vec2(-1.0, -1.0), pd, imax);
    let up  = ce_load(base, vec2( 0.0, -1.0), pd, imax);
    let uf  = ce_load(base, vec2( 1.0, -1.0), pd, imax);
    let uff = ce_load(base, vec2( 2.0, -1.0), pd, imax);

    let bb  = ce_load(base, vec2(-2.0,  0.0), pd, imax);
    let b   = ce_load(base, vec2(-1.0,  0.0), pd, imax);
    let c   = ce_load(base, vec2( 0.0,  0.0), pd, imax);
    let f   = ce_load(base, vec2( 1.0,  0.0), pd, imax);
    let ff  = ce_load(base, vec2( 2.0,  0.0), pd, imax);

    let dbb = ce_load(base, vec2(-2.0,  1.0), pd, imax);
    let db  = ce_load(base, vec2(-1.0,  1.0), pd, imax);
    let d   = ce_load(base, vec2( 0.0,  1.0), pd, imax);
    let df  = ce_load(base, vec2( 1.0,  1.0), pd, imax);
    let dff = ce_load(base, vec2( 2.0,  1.0), pd, imax);

    let ddb = ce_load(base, vec2(-1.0,  2.0), pd, imax);
    let dd  = ce_load(base, vec2( 0.0,  2.0), pd, imax);
    let ddf = ce_load(base, vec2( 1.0,  2.0), pd, imax);

    var col = c;

    // c_orner, b_ack and u_p slices
    // (slices from neighbour texels only ever reach these 3 quadrants)
    let c_col = ce_slice_dist(local, vec2( 1.0,  1.0), pd, up, uf, uff, b, c, f, ff, db, d, df, dff, ddb, dd, ddf);
    let b_col = ce_slice_dist(local, vec2(-1.0,  1.0), pd, up, ub, ubb, f, c, b, bb, df, d, db, dbb, ddf, dd, ddb);
    let u_col = ce_slice_dist(local, vec2( 1.0, -1.0), pd, d, df, dff, b, c, f, ff, ub, up, uf, uff, uub, uu, uuf);

    if c_col.r >= 0.0 { col = c_col; }
    if b_col.r >= 0.0 { col = b_col; }
    if u_col.r >= 0.0 { col = u_col; }

    return col;
}
