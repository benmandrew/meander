// WebGL2 renderer: floodplain age map, oxbow strips, and the channel strip.

export const VERTEX_FLOATS = 5;

const NOISE = `
float hash(vec2 p) {
  p = fract(p * vec2(123.34, 456.21));
  p += dot(p, p + 45.32);
  return fract(p.x * p.y);
}
float noise(vec2 p) {
  vec2 i = floor(p), f = fract(p);
  vec2 s = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1, 0)), s.x),
             mix(hash(i + vec2(0, 1)), hash(i + vec2(1, 1)), s.x), s.y);
}
// Three octaves, rotated between octaves so the lattice does not show.
float fbm(vec2 p) {
  mat2 r = mat2(0.8, 0.6, -0.6, 0.8);
  float v = 0.5 * noise(p);
  p = r * p * 2.03;
  v += 0.25 * noise(p + 17.0);
  p = r * p * 2.03;
  v += 0.125 * noise(p + 41.0);
  return v / 0.875;
}`;

const FLOODPLAIN_VS = `#version 300 es
in vec2 a_pos;
uniform vec4 u_view;
uniform vec2 u_bounds;
out vec2 v_uv;
out vec2 v_world;
void main() {
  v_uv = a_pos / u_bounds;
  v_world = a_pos;
  gl_Position = vec4(a_pos * u_view.xy + u_view.zw, 0, 1);
}`;

const FLOODPLAIN_FS = `#version 300 es
precision highp float;
in vec2 v_uv;
in vec2 v_world;
uniform sampler2D u_age;
uniform sampler2D u_erod;
uniform float u_width;
uniform float u_show_age;
uniform float u_show_erod;
out vec4 o;
${NOISE}
void main() {
  bool inside = all(greaterThanEqual(v_uv, vec2(0))) && all(lessThanEqual(v_uv, vec2(1)));
  // 0..254/255 is time since the channel last passed; 1 is never.
  float a = inside ? texture(u_age, v_uv).r : 1.0;
  float t = a * 255.0 / 254.0;
  vec3 terrace = vec3(0.80, 0.77, 0.62);
  vec3 sand = vec3(0.87, 0.81, 0.63);
  vec3 young = vec3(0.68, 0.73, 0.50);
  vec3 old = vec3(0.47, 0.57, 0.38);
  vec3 c = mix(sand, young, smoothstep(0.0, 0.2, t));
  c = mix(c, old, smoothstep(0.2, 1.0, t));
  // Faint banding by age suggests scroll bars on the point bars.
  c *= 1.0 + 0.035 * sin(t * 80.0);
  c = mix(c, terrace, smoothstep(0.985, 0.998, a));
  c = mix(terrace, c, u_show_age);
  // Erodibility: 0.5 is the base rate. Resistant ground darkens and
  // reddens; easily eroded ground lightens.
  float e = inside ? texture(u_erod, v_uv).r : 0.5;
  vec3 resistant = vec3(0.52, 0.40, 0.30);
  c = mix(c, resistant, u_show_erod * 0.7 * smoothstep(0.5, 0.05, e));
  c = mix(c, vec3(0.95, 0.92, 0.82), u_show_erod * 0.5 * smoothstep(0.5, 1.0, e));
  c *= 0.93 + 0.14 * fbm(v_world / (u_width * 2.5));
  o = vec4(c, 1);
}`;

const CHANNEL_VS = `#version 300 es
in vec2 a_pos;
in float a_u;
in float a_v;
in float a_c;
uniform vec4 u_view;
out float v_u;
out float v_v;
out float v_c;
void main() {
  v_u = a_u;
  v_v = a_v;
  v_c = a_c;
  gl_Position = vec4(a_pos * u_view.xy + u_view.zw, 0, 1);
}`;

const CHANNEL_FS = `#version 300 es
precision highp float;
in float v_u;
in float v_v;
in float v_c;
uniform float u_clock;
uniform float u_flow;
uniform float u_depth_k;
uniform float u_age;
out vec4 o;
${NOISE}
void main() {
  float v = v_v;
  // v = +1 is the right bank, the outer bank of a left turn (c > 0).
  // Depth is deepest toward the outer bank and shoals over the point bar.
  float asym = max(0.0, 1.0 + u_depth_k * v_c * v);
  float depth = (1.0 - v * v) * asym;
  vec3 shallow = vec3(0.50, 0.68, 0.66);
  vec3 deep = vec3(0.13, 0.33, 0.49);
  vec3 sand = vec3(0.87, 0.81, 0.63);
  vec3 c = mix(shallow, deep, smoothstep(0.0, 1.3, depth));
  float inner = smoothstep(0.1, 0.6, -v_c * v * u_depth_k);
  c = mix(c, sand, inner * (1.0 - smoothstep(0.05, 0.35, depth)));
  // Streaks advected downstream along the arc length.
  float s = noise(vec2(v_u * 1.2 - u_clock * u_flow, v * 3.0));
  c += step(0.001, u_flow) * 0.1 * (s - 0.5) * (1.0 - v * v);
  // Oxbows silt up and fade.
  vec3 silt = vec3(0.55, 0.62, 0.46);
  c = mix(c, silt, smoothstep(0.0, 0.8, u_age));
  float alpha = 1.0 - 0.9 * u_age;
  o = vec4(c * alpha, alpha);
}`;

function compile(gl, vs, fs) {
  const prog = gl.createProgram();
  for (const [type, src] of [[gl.VERTEX_SHADER, vs], [gl.FRAGMENT_SHADER, fs]]) {
    const sh = gl.createShader(type);
    gl.shaderSource(sh, src);
    gl.compileShader(sh);
    if (!gl.getShaderParameter(sh, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(sh));
    gl.attachShader(prog, sh);
  }
  gl.linkProgram(prog);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(prog));
  const uniforms = {};
  const n = gl.getProgramParameter(prog, gl.ACTIVE_UNIFORMS);
  for (let i = 0; i < n; i++) {
    const name = gl.getActiveUniform(prog, i).name;
    uniforms[name] = gl.getUniformLocation(prog, name);
  }
  return { prog, uniforms };
}

export class Renderer {
  constructor(canvas) {
    this.canvas = canvas;
    const gl = canvas.getContext('webgl2', { antialias: true, premultipliedAlpha: true });
    if (!gl) throw new Error('WebGL2 is not available');
    this.gl = gl;
    this.floodplain = compile(gl, FLOODPLAIN_VS, FLOODPLAIN_FS);
    this.channel = compile(gl, CHANNEL_VS, CHANNEL_FS);

    this.quadBuf = gl.createBuffer();
    this.quadVao = gl.createVertexArray();
    gl.bindVertexArray(this.quadVao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.quadBuf);
    const loc = gl.getAttribLocation(this.floodplain.prog, 'a_pos');
    gl.enableVertexAttribArray(loc);
    gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);

    this.channelStrip = this.strip();
    this.oxbowStrip = this.strip();

    this.ageTex = this.texture();
    this.erodTex = this.texture();
    gl.bindVertexArray(null);
  }

  texture() {
    const gl = this.gl;
    const tex = gl.createTexture();
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
    gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
    tex.size = [0, 0];
    return tex;
  }

  // Upload a single-channel byte grid, reallocating when its size changes.
  upload(tex, unit, data, [cols, rows]) {
    const gl = this.gl;
    gl.activeTexture(gl.TEXTURE0 + unit);
    gl.bindTexture(gl.TEXTURE_2D, tex);
    gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
    if (tex.size[0] !== cols || tex.size[1] !== rows) {
      gl.texImage2D(gl.TEXTURE_2D, 0, gl.R8, cols, rows, 0, gl.RED, gl.UNSIGNED_BYTE, data);
      tex.size = [cols, rows];
    } else {
      gl.texSubImage2D(gl.TEXTURE_2D, 0, 0, 0, cols, rows, gl.RED, gl.UNSIGNED_BYTE, data);
    }
  }

  strip() {
    const gl = this.gl;
    const buf = gl.createBuffer();
    const vao = gl.createVertexArray();
    gl.bindVertexArray(vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, buf);
    const stride = VERTEX_FLOATS * 4;
    const attrs = [['a_pos', 2, 0], ['a_u', 1, 8], ['a_v', 1, 12], ['a_c', 1, 16]];
    for (const [name, size, offset] of attrs) {
      const loc = gl.getAttribLocation(this.channel.prog, name);
      gl.enableVertexAttribArray(loc);
      gl.vertexAttribPointer(loc, size, gl.FLOAT, false, stride, offset);
    }
    gl.bindVertexArray(null);
    return { buf, vao };
  }

  resize() {
    const dpr = window.devicePixelRatio || 1;
    this.canvas.width = Math.round(this.canvas.clientWidth * dpr);
    this.canvas.height = Math.round(this.canvas.clientHeight * dpr);
  }

  // `views` holds typed-array views into WASM memory, built after `sync`.
  draw(views, opts) {
    const gl = this.gl;
    const { bounds: [ww, wh], width } = opts;
    const [cw, ch] = [this.canvas.width, this.canvas.height];
    gl.viewport(0, 0, cw, ch);
    gl.clearColor(0.80, 0.77, 0.62, 1);
    gl.clear(gl.COLOR_BUFFER_BIT);

    // World to clip space, fitted and centred; world y runs down the screen.
    const scale = Math.min(cw / ww, ch / wh);
    const ox = (cw - ww * scale) / 2;
    const oy = (ch - wh * scale) / 2;
    const view = [2 * scale / cw, -2 * scale / ch, 2 * ox / cw - 1, 1 - 2 * oy / ch];

    // Floodplain, over the whole visible area; outside the domain it is
    // plain terrace.
    this.upload(this.ageTex, 0, views.ages, views.ageSize);
    this.upload(this.erodTex, 1, views.erod, views.erodSize);
    const fp = this.floodplain;
    gl.useProgram(fp.prog);
    gl.uniform4fv(fp.uniforms.u_view, view);
    gl.uniform2f(fp.uniforms.u_bounds, ww, wh);
    gl.uniform1f(fp.uniforms.u_width, width);
    gl.uniform1f(fp.uniforms.u_show_age, opts.showAge ? 1 : 0);
    gl.uniform1f(fp.uniforms.u_show_erod, opts.showErod ? 1 : 0);
    gl.uniform1i(fp.uniforms.u_age, 0);
    gl.uniform1i(fp.uniforms.u_erod, 1);
    gl.bindVertexArray(this.quadVao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.quadBuf);
    const [x0, y0, x1, y1] = [-ox / scale, -oy / scale, (cw - ox) / scale, (ch - oy) / scale];
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([x0, y0, x1, y0, x0, y1, x1, y1]), gl.DYNAMIC_DRAW);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);

    // Oxbows, then the channel, blended with premultiplied alpha.
    gl.enable(gl.BLEND);
    gl.blendFunc(gl.ONE, gl.ONE_MINUS_SRC_ALPHA);
    const cp = this.channel;
    gl.useProgram(cp.prog);
    gl.uniform4fv(cp.uniforms.u_view, view);
    gl.uniform1f(cp.uniforms.u_clock, opts.clock);
    gl.uniform1f(cp.uniforms.u_depth_k, opts.depthK);

    gl.bindVertexArray(this.oxbowStrip.vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.oxbowStrip.buf);
    gl.bufferData(gl.ARRAY_BUFFER, views.oxbows, gl.DYNAMIC_DRAW);
    gl.uniform1f(cp.uniforms.u_flow, 0);
    for (let k = 0; k < views.oxbowAges.length; k++) {
      gl.uniform1f(cp.uniforms.u_age, views.oxbowAges[k]);
      const start = views.oxbowOffsets[k];
      gl.drawArrays(gl.TRIANGLE_STRIP, start, views.oxbowOffsets[k + 1] - start);
    }

    gl.bindVertexArray(this.channelStrip.vao);
    gl.bindBuffer(gl.ARRAY_BUFFER, this.channelStrip.buf);
    gl.bufferData(gl.ARRAY_BUFFER, views.channel, gl.DYNAMIC_DRAW);
    gl.uniform1f(cp.uniforms.u_flow, opts.flow);
    gl.uniform1f(cp.uniforms.u_age, 0);
    gl.drawArrays(gl.TRIANGLE_STRIP, 0, views.channel.length / VERTEX_FLOATS);

    gl.disable(gl.BLEND);
    gl.bindVertexArray(null);
  }
}
