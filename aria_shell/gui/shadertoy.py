from pathlib import  Path
from array import array

from gi.repository import Gdk, Gtk

from aria_shell.utils import CleanupHelper
from aria_shell.utils.logger import get_loggers

DBG, INF, WRN, ERR, CRI = get_loggers(__name__)


try:  # OpenGL is an optional dependency
    from OpenGL import GL
    from OpenGL.GL.shaders import compileShader, compileProgram
except ImportError:
    GL = compileShader = compileProgram = None


VERTEX_SHADER = """
#version 330 core

uniform vec3 iResolution;

layout(location = 0) in vec2 position;
out vec2 fragCoord;

void main()
{
    gl_Position = vec4(position, 0.0, 1.0);
    fragCoord = (position + vec2(1.0)) * 0.5 * iResolution.xy;
}
"""

FRAG_HEADER = """
#version 330 core

// SHADERTOY UNIFORMS
uniform vec3  iResolution;
uniform float iTime;
uniform float iTimeDelta;
uniform int   iFrame;
uniform vec4  iMouse;

// SHADER INPUT
in vec2 fragCoord;
out vec4 outColor;

// SHADERTOY HELPERS
vec2 getUV()
{
    return fragCoord / iResolution.xy;
}
vec2 getCenteredUV()
{
    vec2 uv = fragCoord / iResolution.xy;
    return uv * 2.0 - 1.0;
}
vec2 getRayUV()
{
    vec2 uv = (fragCoord - 0.5 * iResolution.xy) / iResolution.y;
    return uv;
}
float getDeltaTime()
{
    return max(iTimeDelta, 0.0001);
}
vec4 getMouse()
{
    return iMouse;
}
"""

FRAG_FOOTER = """
void main()
{
    vec4 color = vec4(0.0);
    mainImage(color, fragCoord);
    outColor = color;
}
"""


TEST_SHADER_UNIFORMS = """
void mainImage(out vec4 fragColor, in vec2 fragCoord)
{
    vec2 uv = fragCoord / iResolution.xy;

    vec3 col = 0.5 + 0.5*cos(iTime + uv.xyx + vec3(0,2,4));

    float d = length(uv - iMouse.xy / iResolution.xy);
    col += 0.3 * exp(-10.0*d);

    fragColor = vec4(col, 1.0);
}
"""


# =========================================================
# SHADERTOY WIDGET
# =========================================================
class ShaderToy(CleanupHelper, Gtk.GLArea):
    """
    A gtk widget to show Shadertoy.com dialect shaders.

    Just save the shader code in a file with .shadertoy extension.
    ...and cross your finger :)
    """
    __supported_extensions__ = {'shadertoy'}

    def __init__(self, shader: Path | str, **args):
        super().__init__()

        if GL is None:
            WRN('OpenGL not available, install python-opengl for shaders support')
            return

        if isinstance(shader, Path):
            self.shader_code = shader.read_text()
        else:
            self.shader_code = shader

        # TO DEBUG UNIFORMS:
        # self.shader_code = TEST_SHADER_UNIFORMS

        # some shadertoy.com shaders require Desktop GL  :/
        self.set_allowed_apis(Gdk.GLAPI.GL)

        self.time = 0.0
        self.last_time = 0.0
        self.frame = 0

        self.mouse = [0.0, 0.0, 0.0, 0.0]
        self.resolution = [1.0, 1.0, 1.0]

        self.program: ShaderProgram | None = None
        self.quad: FullscreenQuad | None = None

        self.ticker = self.add_tick_callback(self.on_tick)

        drag = Gtk.GestureDrag()
        self.safe_connect(drag, 'drag-begin', self.on_drag_begin)
        self.safe_connect(drag, 'drag-update', self.on_drag_update)
        self.safe_connect(drag, 'drag-end', self.on_drag_end)
        self.add_controller(drag)

    # -----------------------------
    # GL init / cleanup
    # -----------------------------
    def do_realize(self):
        Gtk.GLArea.do_realize(self)

        self.make_current()
        try:
            self.quad = FullscreenQuad()
            self.program = ShaderProgram(
                VERTEX_SHADER,
                FRAG_HEADER + self.shader_code + FRAG_FOOTER
            )
        except Exception as e:
            ERR('Cannot compile shader. Error: %s', e)
            self.quad = None
            self.program = None

    def do_resize(self, w: int, h: int):
        self.resolution = [w, h, 1.0]
        GL.glViewport(0, 0, w, h)

    def do_unrealize(self):
        self.make_current()
        if self.ticker:
            self.remove_tick_callback(self.ticker)
            self.ticker = 0
        if self.program:
            self.program.destroy()
            self.program = None
        if self.quad:
            self.quad.destroy()
            self.quad = None
        Gtk.GLArea.do_unrealize(self)
        CleanupHelper.shutdown(self)

    # -----------------------------
    # time ticker
    # -----------------------------
    def on_tick(self, _area, clock: Gdk.FrameClock):
        now = clock.get_frame_time() / 1_000_000.0

        self.frame += 1
        self.last_time = self.time
        self.time = now

        self.queue_render()
        return True

    # -----------------------------
    # render
    # -----------------------------
    def do_render(self, _ctx):
        if self.program and self.quad:
            self.make_current()
            self.program.use()

            GL.glUniform3f(self.program.u_resolution, *self.resolution)
            GL.glUniform4f(self.program.u_mouse,      *self.mouse)
            GL.glUniform1f(self.program.u_time,       self.time)
            GL.glUniform1f(self.program.u_delta,      self.time - self.last_time)
            GL.glUniform1i(self.program.u_frame,      self.frame)
            # TODO support  iChannel0, then test: https://www.shadertoy.com/view/MsGSRd

            self.quad.draw()

    # -----------------------------
    # mouse
    # -----------------------------
    def on_drag_begin(self, _gesture: Gtk.GestureDrag, x: float, y: float):
        h = self.get_height()
        self.mouse = [x, h - y, x, h - y]

    def on_drag_update(self, gesture: Gtk.GestureDrag, dx: float, dy: float):
        ok, sx, sy = gesture.get_start_point()
        if ok:
            h = self.get_height()
            self.mouse[0] = sx + dx
            self.mouse[1] = h - (sy + dy)

    def on_drag_end(self, _gesture: Gtk.GestureDrag, _dx: float, _dy: float):
        # Shadertoy negate z/w components on mouse release
        self.mouse[2] *= -1
        self.mouse[3] *= -1


# =========================================================
# SHADER PROGRAM (compile + uniforms)
# =========================================================
class ShaderProgram:
    def __init__(self, vertex_src: str, fragment_src: str):
        # compile the shaders and the program
        self.program = compileProgram(
            compileShader(vertex_src, GL.GL_VERTEX_SHADER),
            compileShader(fragment_src, GL.GL_FRAGMENT_SHADER),
        )

        # cache uniform locations
        self.u_resolution = GL.glGetUniformLocation(self.program, 'iResolution')
        self.u_time       = GL.glGetUniformLocation(self.program, 'iTime')
        self.u_delta      = GL.glGetUniformLocation(self.program, 'iTimeDelta')
        self.u_frame      = GL.glGetUniformLocation(self.program, 'iFrame')
        self.u_mouse      = GL.glGetUniformLocation(self.program, 'iMouse')

        # use
        GL.glUseProgram(self.program)

    def use(self):
        if self.program:
            GL.glUseProgram(self.program)

    def destroy(self):
        if self.program:
            GL.glDeleteProgram(self.program)
            self.program = None


# =========================================================
# FULLSCREEN QUAD
# =========================================================
class FullscreenQuad:
    def __init__(self):
        self.vao = GL.glGenVertexArrays(1)
        self.vbo = GL.glGenBuffers(1)

        vertices = array('f', [
            -1.0, -1.0,
             1.0, -1.0,
            -1.0,  1.0,
             1.0,  1.0,
        ])

        GL.glBindVertexArray(self.vao)
        GL.glBindBuffer(GL.GL_ARRAY_BUFFER, self.vbo)

        GL.glBufferData(
            GL.GL_ARRAY_BUFFER,
            vertices.itemsize * len(vertices),
            vertices.tobytes(),
            GL.GL_STATIC_DRAW
        )

        GL.glVertexAttribPointer(0, 2, GL.GL_FLOAT, GL.GL_FALSE, 0, None)
        GL.glEnableVertexAttribArray(0)

        GL.glBindVertexArray(0)

    def draw(self):
        GL.glBindVertexArray(self.vao)
        GL.glDrawArrays(GL.GL_TRIANGLE_STRIP, 0, 4)

    def destroy(self):
        GL.glDeleteVertexArrays(1, [self.vao])
        GL.glDeleteBuffers(1, [self.vbo])
        self.vao = self.vbo = None
