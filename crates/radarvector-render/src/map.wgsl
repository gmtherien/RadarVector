// RadarVector map shaders. Positions are lon/lat degrees; the camera
// uniform maps them to screen points of the paint-callback rect, then NDC.

struct Camera {
    center: vec2<f32>,      // projected centre (x = lon, y = projected lat)
    scale: f32,             // points per projected degree
    mercator: f32,          // 1.0 = Mercator, 0.0 = equirectangular
    viewport: vec2<f32>,    // rect size in points
    world_offset: f32,      // longitude shift of this world copy
    _pad: f32,
};

@group(0) @binding(0) var<uniform> cam: Camera;

const PI: f32 = 3.14159265358979;

fn proj_y(lat: f32) -> f32 {
    if (cam.mercator > 0.5) {
        let l = clamp(lat, -85.0, 85.0) * PI / 180.0;
        return log(tan(PI / 4.0 + l / 2.0)) * 180.0 / PI;
    }
    return lat;
}

// lon/lat -> screen points (y down) relative to the rect.
fn to_screen(p: vec2<f32>) -> vec2<f32> {
    let x = (p.x + cam.world_offset - cam.center.x) * cam.scale;
    let y = (proj_y(p.y) - cam.center.y) * cam.scale;
    return vec2<f32>(cam.viewport.x * 0.5 + x, cam.viewport.y * 0.5 - y);
}

fn to_clip(s: vec2<f32>) -> vec4<f32> {
    let ndc = vec2<f32>(s.x / cam.viewport.x * 2.0 - 1.0, 1.0 - s.y / cam.viewport.y * 2.0);
    return vec4<f32>(ndc, 0.0, 1.0);
}

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) color: vec4<f32>,
};

// ---- Filled meshes (land, FIR fills) ----------------------------------

@vertex
fn vs_fill(@location(0) pos: vec2<f32>, @location(1) color: vec4<f32>) -> VsOut {
    var o: VsOut;
    o.pos = to_clip(to_screen(pos));
    o.color = color;
    return o;
}

// ---- Thick line segments, one instance per segment ---------------------

@vertex
fn vs_segment(
    @builtin(vertex_index) vi: u32,
    @location(0) a: vec2<f32>,
    @location(1) b: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) width: f32,
) -> VsOut {
    // Two triangles: corners (t, side) with t in {0,1}, side in {-1,1}.
    var ts = array<f32, 6>(0.0, 1.0, 0.0, 0.0, 1.0, 1.0);
    var ss = array<f32, 6>(-1.0, -1.0, 1.0, 1.0, -1.0, 1.0);
    let t = ts[vi];
    let side = ss[vi];
    let pa = to_screen(a);
    let pb = to_screen(b);
    var d = pb - pa;
    let len = length(d);
    if (len < 1e-4) {
        d = vec2<f32>(1.0, 0.0);
    } else {
        d = d / len;
    }
    let n = vec2<f32>(-d.y, d.x);
    let hw = width * 0.5;
    let p = mix(pa, pb, t) + n * side * hw + d * (t * 2.0 - 1.0) * hw * 0.5;
    var o: VsOut;
    o.pos = to_clip(p);
    o.color = color;
    return o;
}

// ---- Aircraft glyphs, one instance per aircraft -----------------------

@vertex
fn vs_aircraft(
    @builtin(vertex_index) vi: u32,
    @location(0) pos: vec2<f32>,
    @location(1) heading: f32,
    @location(2) size: f32,
    @location(3) color: vec4<f32>,
) -> VsOut {
    // Unit glyph pointing up (+y = forward), 4 triangles: fuselage (2), wings, tail.
    var g = array<vec2<f32>, 12>(
        vec2<f32>(-0.09, -0.85), vec2<f32>(0.09, -0.85), vec2<f32>(0.0, 1.0),
        vec2<f32>(-0.09, -0.85), vec2<f32>(0.09, -0.85), vec2<f32>(0.0, -0.95),
        vec2<f32>(-0.95, -0.05), vec2<f32>(0.95, -0.05), vec2<f32>(0.0, 0.45),
        vec2<f32>(-0.38, -0.95), vec2<f32>(0.38, -0.95), vec2<f32>(0.0, -0.55),
    );
    let local = g[vi] * size;
    let h = heading * PI / 180.0;
    // Rotate clockwise by heading; screen y points down.
    let r = vec2<f32>(local.x * cos(h) + local.y * sin(h), -(-local.x * sin(h) + local.y * cos(h)));
    var o: VsOut;
    o.pos = to_clip(to_screen(pos) + r);
    o.color = color;
    return o;
}

@fragment
fn fs_color(i: VsOut) -> @location(0) vec4<f32> {
    // Premultiplied alpha output (egui's blend state).
    return vec4<f32>(i.color.rgb * i.color.a, i.color.a);
}
