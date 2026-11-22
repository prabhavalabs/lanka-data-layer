// Isolated compositing inside MapLibre's own frame: real blend modes and real group opacity.
//
// MapLibre has no blend state per layer, and a fixed-function blend cannot express a mode that reads
// the backdrop (overlay). So a node that needs compositing is bracketed by two custom layers:
//
//   begin  copy the frame so far to a texture (the backdrop), then clear the colour buffer
//   …      MapLibre draws the node's own layers into the empty buffer, exactly as it always does
//   end    copy that buffer (the node, premultiplied), then write blend(backdrop, node) × opacity
//
// The node is therefore composited as ONE image: a group at 50 % does not show its children through
// each other, and every separable blend mode is a few lines of shader. Cost: two framebuffer copies
// and one full-screen pass per isolated node per frame; nodes that need neither stay on the plain path.
import type { CustomLayerInterface, Map as MapLibreMap } from 'maplibre-gl'
import type { BlendMode } from '@/lib/map-spec'

type GL = WebGLRenderingContext | WebGL2RenderingContext
const MODE: Record<BlendMode, number> = { normal: 0, multiply: 1, screen: 2, overlay: 3, darken: 4, lighten: 5, add: 6 }

const VERT = `attribute vec2 a_pos; varying vec2 v_uv; void main() { v_uv = a_pos * 0.5 + 0.5; gl_Position = vec4(a_pos, 0.0, 1.0); }`
const FRAG = `precision mediump float;
uniform sampler2D u_backdrop; uniform sampler2D u_layer; uniform float u_opacity; uniform int u_mode; varying vec2 v_uv;
vec3 blend(vec3 d, vec3 s) {
  if (u_mode == 1) return d * s;
  if (u_mode == 2) return d + s - d * s;
  if (u_mode == 3) return mix(2.0 * d * s, 1.0 - 2.0 * (1.0 - d) * (1.0 - s), step(0.5, d));
  if (u_mode == 4) return min(d, s);
  if (u_mode == 5) return max(d, s);
  if (u_mode == 6) return min(d + s, 1.0);
  return s;
}
void main() {
  vec4 D = texture2D(u_backdrop, v_uv);
  vec4 L = texture2D(u_layer, v_uv);
  float a = L.a * u_opacity;
  if (a <= 0.0005) { gl_FragColor = D; return; }
  vec3 s = L.rgb / max(L.a, 0.0001);
  vec3 d = D.a > 0.0 ? D.rgb / D.a : vec3(0.0);
  // W3C compositing: where the backdrop is transparent the source shows unblended
  vec3 c = mix(s, blend(d, s), D.a);
  gl_FragColor = vec4(a * c + (1.0 - a) * D.rgb, a + D.a * (1.0 - a));
}`

export type IsoParams = { blend: BlendMode; opacity: number }
export const isoId = (nodeId: string, end: boolean) => `iso:${nodeId}:${end ? 'z' : 'a'}`

export class Compositor {
  /** read at draw time, so a slider only has to change a number and ask for a repaint */
  readonly params = new Map<string, IsoParams>()
  private gl: GL | null = null
  private program: WebGLProgram | null = null
  private quad: WebGLBuffer | null = null
  private uniforms: Record<string, WebGLUniformLocation | null> = {}
  private pool: WebGLTexture[] = []
  private free: WebGLTexture[] = []
  private stack: WebGLTexture[] = []
  private w = 0
  private h = 0

  private setup(gl: GL) {
    if (this.gl === gl && this.program) return
    this.gl = gl
    this.pool = []
    this.free = []
    this.stack = []
    const shader = (type: number, src: string) => {
      const s = gl.createShader(type)!
      gl.shaderSource(s, src)
      gl.compileShader(s)
      return s
    }
    const p = gl.createProgram()!
    gl.attachShader(p, shader(gl.VERTEX_SHADER, VERT))
    gl.attachShader(p, shader(gl.FRAGMENT_SHADER, FRAG))
    gl.bindAttribLocation(p, 0, 'a_pos')
    gl.linkProgram(p)
    this.program = p
    for (const u of ['u_backdrop', 'u_layer', 'u_opacity', 'u_mode']) this.uniforms[u] = gl.getUniformLocation(p, u)
    this.quad = gl.createBuffer()
    gl.bindBuffer(gl.ARRAY_BUFFER, this.quad)
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW)
  }
  /** A texture the size of the drawing buffer, holding a copy of what is in it now. */
  private grab(gl: GL): WebGLTexture {
    const w = gl.drawingBufferWidth, h = gl.drawingBufferHeight
    if (w !== this.w || h !== this.h) {
      for (const t of this.pool) gl.deleteTexture(t)
      this.pool = []
      this.free = []
      this.stack = []
      this.w = w
      this.h = h
    }
    let tex = this.free.pop()
    gl.activeTexture(gl.TEXTURE0)
    if (!tex) {
      tex = gl.createTexture()!
      gl.bindTexture(gl.TEXTURE_2D, tex)
      for (const [k, v] of [[gl.TEXTURE_MIN_FILTER, gl.NEAREST], [gl.TEXTURE_MAG_FILTER, gl.NEAREST], [gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE], [gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE]]) gl.texParameteri(gl.TEXTURE_2D, k, v)
      gl.copyTexImage2D(gl.TEXTURE_2D, 0, gl.RGBA, 0, 0, w, h, 0)
      this.pool.push(tex)
    } else {
      gl.bindTexture(gl.TEXTURE_2D, tex)
      gl.copyTexSubImage2D(gl.TEXTURE_2D, 0, 0, 0, 0, 0, w, h)
    }
    return tex
  }
  private plain(gl: GL) {
    gl.disable(gl.DEPTH_TEST)
    gl.disable(gl.STENCIL_TEST)
    gl.disable(gl.SCISSOR_TEST)
    gl.disable(gl.CULL_FACE)
    gl.colorMask(true, true, true, true)
    gl.viewport(0, 0, gl.drawingBufferWidth, gl.drawingBufferHeight)
  }

  begin(nodeId: string): CustomLayerInterface {
    return {
      id: isoId(nodeId, false),
      type: 'custom',
      renderingMode: '2d',
      render: (gl: GL) => {
        this.setup(gl)
        this.stack.push(this.grab(gl))
        this.plain(gl)
        gl.clearColor(0, 0, 0, 0)
        gl.clear(gl.COLOR_BUFFER_BIT)
      },
    }
  }
  end(nodeId: string): CustomLayerInterface {
    return {
      id: isoId(nodeId, true),
      type: 'custom',
      renderingMode: '2d',
      render: (gl: GL) => {
        this.setup(gl)
        const backdrop = this.stack.pop()
        if (!backdrop) return
        const layer = this.grab(gl)
        const p = this.params.get(nodeId) ?? { blend: 'normal', opacity: 1 }
        this.plain(gl)
        gl.disable(gl.BLEND)
        gl.useProgram(this.program)
        gl.activeTexture(gl.TEXTURE0)
        gl.bindTexture(gl.TEXTURE_2D, backdrop)
        gl.activeTexture(gl.TEXTURE1)
        gl.bindTexture(gl.TEXTURE_2D, layer)
        gl.uniform1i(this.uniforms.u_backdrop, 0)
        gl.uniform1i(this.uniforms.u_layer, 1)
        gl.uniform1f(this.uniforms.u_opacity, p.opacity)
        gl.uniform1i(this.uniforms.u_mode, MODE[p.blend])
        gl.bindBuffer(gl.ARRAY_BUFFER, this.quad)
        gl.enableVertexAttribArray(0)
        gl.vertexAttribPointer(0, 2, gl.FLOAT, false, 0, 0)
        gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4)
        gl.activeTexture(gl.TEXTURE0)
        this.free.push(backdrop, layer)
      },
    }
  }
  /** A context loss or a new style throws the GL objects away. */
  reset() {
    this.gl = null
    this.program = null
    this.pool = []
    this.free = []
    this.stack = []
  }
  attach(map: MapLibreMap) {
    map.on('webglcontextlost', () => this.reset())
    // a begin without its end (a layer removed mid-frame) must not leak a backdrop into the next frame
    map.on('render', () => {
      while (this.stack.length) this.free.push(this.stack.pop()!)
    })
  }
}
