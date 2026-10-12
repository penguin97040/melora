"""Sample repeated Linux/X11 launches for a single centred reveal and first-paint timing.

Run with the freshly built release executable. Requires Xvfb and system libX11;
no window manager/compositor is simulated, and this does not verify Windows.
"""
import ctypes as c
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


class Attributes(c.Structure):
    _fields_ = [
        ('x', c.c_int), ('y', c.c_int), ('width', c.c_int), ('height', c.c_int),
        ('border_width', c.c_int), ('depth', c.c_int), ('visual', c.c_void_p),
        ('root', c.c_ulong), ('class_', c.c_int), ('bit_gravity', c.c_int),
        ('win_gravity', c.c_int), ('backing_store', c.c_int),
        ('backing_planes', c.c_ulong), ('backing_pixel', c.c_ulong),
        ('save_under', c.c_int), ('colormap', c.c_ulong),
        ('map_installed', c.c_int), ('map_state', c.c_int),
        ('all_event_masks', c.c_long), ('your_event_mask', c.c_long),
        ('do_not_propagate_mask', c.c_long), ('override_redirect', c.c_int),
        ('screen', c.c_void_p),
    ]


if len(sys.argv) == 2:
    subprocess.run(['xvfb-run', '-a', '-s', '-screen 0 1920x1080x24',
                    sys.executable, __file__, sys.argv[1], '--on-display'], check=True)
    sys.exit(0)
if len(sys.argv) != 3 or sys.argv[2] != '--on-display':
    raise SystemExit('Usage: python3 scripts/test-startup.py /path/to/release/melora')

exe = Path(sys.argv[1]).resolve()
x = c.CDLL('libX11.so.6')
signatures = {
    'XOpenDisplay': ([c.c_char_p], c.c_void_p),
    'XDefaultRootWindow': ([c.c_void_p], c.c_ulong),
    'XQueryTree': ([c.c_void_p, c.c_ulong, c.POINTER(c.c_ulong),
                    c.POINTER(c.c_ulong), c.POINTER(c.POINTER(c.c_ulong)),
                    c.POINTER(c.c_uint)], c.c_int),
    'XFetchName': ([c.c_void_p, c.c_ulong, c.POINTER(c.c_void_p)], c.c_int),
    'XGetWindowAttributes': ([c.c_void_p, c.c_ulong, c.POINTER(Attributes)], c.c_int),
    'XGetImage': ([c.c_void_p, c.c_ulong, c.c_int, c.c_int, c.c_uint,
                   c.c_uint, c.c_ulong, c.c_int], c.c_void_p),
    'XGetPixel': ([c.c_void_p, c.c_int, c.c_int], c.c_ulong),
    'XDestroyImage': ([c.c_void_p], c.c_int),
    'XFree': ([c.c_void_p], c.c_int),
    'XCloseDisplay': ([c.c_void_p], c.c_int),
}
for name, (argtypes, restype) in signatures.items():
    function = getattr(x, name)
    function.argtypes, function.restype = argtypes, restype
display = x.XOpenDisplay(None)
assert display, 'Cannot open test X11 display'
root = x.XDefaultRootWindow(display)


def windows():
    parent, root_return = c.c_ulong(), c.c_ulong()
    children, count = c.POINTER(c.c_ulong)(), c.c_uint()
    assert x.XQueryTree(display, root, c.byref(root_return), c.byref(parent),
                        c.byref(children), c.byref(count))
    matches = []
    try:
        for index in range(count.value):
            window = children[index]
            name = c.c_void_p()
            if x.XFetchName(display, window, c.byref(name)) and name.value:
                try:
                    if c.string_at(name) == b'Melora':
                        attributes = Attributes()
                        if x.XGetWindowAttributes(display, window, c.byref(attributes)):
                            matches.append((window, attributes))
                finally:
                    x.XFree(name)
    finally:
        if children:
            x.XFree(children)
    return matches


results = []
try:
    for run in range(3):
        with tempfile.TemporaryDirectory(prefix='melora-startup-') as data:
            # Exercise restored scale and corrupt-preference error on real launches.
            if run:
                settings = Path(data) / 'app.melora.desktop/preferences.json'
                settings.parent.mkdir()
                settings.write_text('{"scale":125,"comfortable":true,"volume":40}'
                                    if run == 1 else 'invalid synthetic settings')
            env = {**os.environ, 'XDG_DATA_HOME': data, 'GDK_SCALE': '1',
                   'WEBKIT_DISABLE_COMPOSITING_MODE': '1'}
            start = time.perf_counter()
            process = subprocess.Popen([str(exe)], env=env, stdout=subprocess.DEVNULL,
                                       stderr=subprocess.DEVNULL)
            frames = []
            hidden_seen = False
            first_visible = None
            first_shell = None
            try:
                while time.perf_counter() - start < 10:
                    assert process.poll() is None, 'Application exited before startup check finished'
                    matches = windows()
                    assert len(matches) <= 1, 'Multiple main windows created'
                    for window, attrs in matches:
                        if attrs.map_state != 2:
                            hidden_seen = True
                            assert first_visible is None, 'Window hidden after reveal'
                            continue
                        elapsed = time.perf_counter() - start
                        if first_visible is None:
                            first_visible = elapsed
                        rect = [attrs.x, attrs.y, attrs.width, attrs.height]
                        assert rect == [240, 70, 1440, 940], f'Visible window moved/resized: {rect}'
                        # Sample the centre panel at several positions. A blank native
                        # background or black frame cannot have its distinct colour.
                        image = x.XGetImage(display, window, 0, 0, attrs.width,
                                            attrs.height, c.c_ulong(-1).value, 2)
                        assert image, 'Cannot sample mapped window'
                        try:
                            colours = [x.XGetPixel(image, px, py) & 0xffffff
                                       for px in [500, 700, 900]
                                       for py in [500, 650, 750]]
                        finally:
                            x.XDestroyImage(image)
                        rendered = 0x0e2118 in colours
                        if rendered and first_shell is None:
                            first_shell = elapsed
                        frames.append({'ms': round(elapsed * 1000, 2),
                                       'rect': rect, 'centre_rendered': rendered, 'colours': [hex(v) for v in colours]})
                    if first_visible is not None and elapsed - first_visible >= 0.6:
                        break
                    time.sleep(0.005)
                assert first_visible is not None and first_shell is not None, 'Rendered reveal timed out'
                assert hidden_seen, 'Hidden startup window was not observed'
                # An uncomposited X11 map can be sampled before GTK's first paint.
                # Record that interval honestly, then reject any later blank frame.
                assert all(frame['centre_rendered'] for frame in frames if frame['ms'] >= round(first_shell * 1000, 2)), 'Blank frame after the first rendered shell'
                results.append({'launch': run + 1, 'preferences': ['default', '125% restored', 'corrupt'][run],
                                'hidden_observed': hidden_seen,
                                'first_visible_ms': round(first_visible * 1000, 2),
                                'first_rendered_ms': round(first_shell * 1000, 2),
                                'mapped_to_first_paint_ms': round((first_shell - first_visible) * 1000, 2),
                                'blank_samples_before_first_paint': sum(not frame['centre_rendered'] for frame in frames),
                                'frames': frames})
            finally:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)
    output = Path('artifacts/runtime/startup-acceptance.json')
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps({'executable_sha256': hashlib.sha256(exe.read_bytes()).hexdigest(),
                                 'environment': 'Linux X11/Xvfb, no window manager or compositor',
                                 'sampling': '5ms requested interval plus X11 calls; sub-sample flashes not ruled out',
                                 'windows_macos_runtime': 'not tested', 'launches': results}, indent=2) + '\n')
    print('Repeated Linux launch geometry passed: single centred reveal; map timings ' +
          ', '.join(str(item['first_visible_ms']) + 'ms' for item in results))
finally:
    x.XCloseDisplay(display)
