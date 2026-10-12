"""Linux WebKitGTK acceptance checks against the real Tauri executable.

Run after `npm run tauri build -- --debug --no-bundle`:
python3 scripts/test-native.py /absolute/path/to/melora
Requires Xvfb and WebKitWebDriver. Uses isolated disposable application data.
"""
import base64
import argparse
import json
import re
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import urllib.request

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('executable')
parser.add_argument('--screen', default='1920x1080')
parser.add_argument('--device-scale', type=int, choices=[1, 2], default=1)
parser.add_argument('--opening-only', action='store_true')
args = parser.parse_args()
if not re.fullmatch(r'[1-9][0-9]*x[1-9][0-9]*', args.screen):
    parser.error('Screen must be WIDTHxHEIGHT in physical pixels')
screen_width, screen_height = map(int, args.screen.split('x'))
exe = str(Path(args.executable).resolve())
output = Path('artifacts/runtime')
output.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(prefix='melora-runtime-') as data:
    driver = subprocess.Popen(['xvfb-run', '-a', '-s', f'-screen 0 {args.screen}x24', 'env', 'TAURI_WEBVIEW_AUTOMATION=true',
                               f'GDK_SCALE={args.device_scale}',
                               'WEBKIT_DISABLE_COMPOSITING_MODE=1', 'XDG_DATA_HOME=' + data,
                               'WebKitWebDriver', '--port=4445'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    base = 'http://127.0.0.1:4445'
    session = None
    try:
        def request(method, route, payload=None):
            body = json.dumps(payload).encode() if payload is not None else None
            req = urllib.request.Request(base + route, data=body, method=method, headers={'Content-Type': 'application/json'})
            return json.load(urllib.request.urlopen(req, timeout=20))['value']

        for attempt in range(40):
            try:
                request('GET', '/status')
                break
            except urllib.error.URLError:
                time.sleep(0.1)
        def launch():
            return request('POST', '/session', {'capabilities': {'alwaysMatch': {
                'webkitgtk:browserOptions': {'binary': exe}}}})['sessionId']

        session = launch()
        def call(method, route, payload=None):
            return request(method, '/session/' + session + route, payload)
        def execute(script, *args):
            return call('POST', '/execute/sync', {'script': script, 'args': list(args)})
        def wait_script(script):
            for _ in range(80):
                if execute(script):
                    return
                time.sleep(0.1)
            raise AssertionError('Native UI assertion timed out')
        wait_script("return document.body.innerText.includes('v0.1.0 / linux');")
        opening_size = execute('return {width: innerWidth, height: innerHeight};')
        if args.opening_only:
            assert opening_size == {
                'width': min(1360, screen_width // args.device_scale),
                'height': min(940, screen_height // args.device_scale),
            }, f'Opening size not bounded to simulated monitor: {opening_size}'
            rect = call('GET', '/window/rect')
            assert rect['x'] >= 0 and rect['y'] >= 0, 'Opening position outside monitor'
            assert rect['x'] + rect['width'] <= screen_width and rect['y'] + rect['height'] <= screen_height, 'Outer window outside monitor'
            evidence = {'screen': args.screen, 'device_scale': args.device_scale,
                        'opening_client': opening_size, 'outer_rect': rect,
                        'decorations': 'No window manager in Xvfb; subtraction unit-tested',
                        'actual_os_dpi': 'not tested'}
            (output / f'opening-{args.screen}-{args.device_scale}.json').write_text(json.dumps(evidence, indent=2) + '\n')
            print(f'Native opening bounds passed: {args.screen} physical pixels, simulated GTK scale {args.device_scale}.')
            sys.exit(0)
        assert opening_size == {'width': 1360, 'height': 940}, 'Preferred opening client size not applied'
        assert execute("return document.querySelector('.player').scrollHeight <= document.querySelector('.player').clientHeight + 1;"), 'Preferred player requires scrolling'
        (output / 'melora-linux-preferred.png').write_bytes(base64.b64decode(call('GET', '/screenshot')))
        assert execute("return document.querySelector('button[aria-label=Play]').disabled;")
        assert execute("return document.querySelector('.stream-quality').textContent === 'Stream quality: unavailable';")
        wait_script("return document.querySelector('.brand-symbol').complete && document.querySelector('.brand-symbol').naturalWidth > 0;")
        assert execute("return document.fonts.check('16px \"Share Tech Mono\"') && document.fonts.check('16px \"Chakra Petch\"') && document.fonts.check('16px \"Ubuntu Mono\"');")
        contrast = execute("""
            const css = getComputedStyle(document.documentElement);
            const names = ['--text', '--muted', '--cyan', '--orange', '--active-mode'];
            const surfaces = ['--background', '--surface', '--surface-raised', '--control', '--selection'];
            function luminance(hex) {
                const channels = hex.trim().slice(1).match(/../g).map(value => {
                    const v = parseInt(value, 16) / 255;
                    return v <= .04045 ? v / 12.92 : ((v + .055) / 1.055) ** 2.4;
                });
                return channels[0] * .2126 + channels[1] * .7152 + channels[2] * .0722;
            }
            return names.flatMap(name => surfaces.map(surface => {
                const a = luminance(css.getPropertyValue(name));
                const b = luminance(css.getPropertyValue(surface));
                return {text: name, background: surface, ratio: (Math.max(a,b)+.05)/(Math.min(a,b)+.05)};
            }));
        """)
        assert all(pair['ratio'] >= 4.5 for pair in contrast), 'Text contrast below 4.5:1'
        # Actual keyboard events, rather than a click simulation.
        def key(value):
            call('POST', '/actions', {'actions': [{'type': 'key', 'id': 'keyboard', 'actions': [
                {'type': 'keyDown', 'value': value}, {'type': 'keyUp', 'value': value}]}]})
        def visible_focus():
            assert execute("return getComputedStyle(document.activeElement).outlineStyle !== 'none';"), 'Keyboard focus outline missing'
            assert execute("""
                const box = document.activeElement.getBoundingClientRect();
                const player = document.querySelector('.player').getBoundingClientRect();
                return box.left - 3 >= player.left && box.right + 3 <= player.right &&
                    box.top - 3 >= document.querySelector('.masthead').getBoundingClientRect().bottom &&
                    box.bottom + 3 <= document.querySelector('.status-strip').getBoundingClientRect().top;
            """), 'Focused mode control obscured'
        execute("document.querySelector('.transport button').focus();")
        key(' ')
        wait_script("return document.querySelector('.transport button').getAttribute('aria-label') === 'Shuffle: On (preview)';")
        visible_focus()
        key('\ue004')
        wait_script("return document.activeElement === document.querySelector('.transport button:last-child');")
        for mode in ['All', 'Song', 'Off', 'All', 'Song']:
            key('\ue007')
            wait_script(f"return document.activeElement.getAttribute('aria-label') === 'Repeat: {mode} (preview)';")
        assert execute("return document.querySelector('.repeat-one').textContent === '1';")
        assert execute("return document.querySelector('.mode-status').textContent === 'Shuffle: On · Repeat: Song';")
        execute("document.querySelectorAll('.playlist-item')[2].focus();")
        key('\ue007')
        wait_script("return document.querySelector('h1').textContent === 'Soft machinery';")
        execute("document.querySelectorAll('.track-select')[1].focus();")
        key('\ue007')
        wait_script("return document.querySelector('.track-title').textContent === 'Amber lines';")
        assert execute("return getComputedStyle(document.activeElement).outlineStyle !== 'none';")
        execute("document.querySelectorAll('.nav-item')[2].click();")
        wait_script("return !!document.querySelector('select');")
        cases = [(1360, height, scale) for height in [940, 660, 520] for scale in [100, 125, 150, 175, 200]]
        cases += [(720, 850, 100), (720, 850, 200)]
        geometry = []
        for width, height, scale in cases:
            call('POST', '/window/rect', {'width': width, 'height': height})
            execute("const el=document.querySelector('select');el.value=String(arguments[0]);el.dispatchEvent(new Event('change',{bubbles:true}));", scale)
            wait_script(f"return document.documentElement.style.fontSize === '{scale}%';")
            execute("document.querySelectorAll('.nav-item')[0].click();")
            wait_script("return !!document.querySelector('.track-table');")
            assert execute("return document.querySelector('.masthead').getBoundingClientRect().height <= 37 * arguments[0] / 100;", scale), 'Header expanded beyond compact layout'
            assert execute("return document.querySelector('.track-select').getBoundingClientRect().width > 90;"), 'Track text space collapsed'
            assert execute("return document.querySelector('main').clientHeight > 180;"), f'Content pane collapsed at {width}/{height}/{scale}'
            shot = call('GET', '/screenshot')
            (output / f'melora-linux-{width}-{height}-{scale}-percent.png').write_bytes(base64.b64decode(shot))
            execute("const track = document.querySelector('.track-select'); track.focus(); track.scrollIntoView({block: 'nearest'});")
            assert execute("const box = document.activeElement.getBoundingClientRect(); return box.top >= document.querySelector('.masthead').getBoundingClientRect().bottom - 1 && box.bottom <= document.querySelector('.status-strip').getBoundingClientRect().top + 1;"), 'Focused track obscured by persistent header/footer'
            assert execute('return scrollY === 0 && document.documentElement.scrollHeight <= innerHeight + 1;'), 'Track focus moved the whole document'
            shot = call('GET', '/screenshot')
            (output / f'melora-linux-{width}-{height}-{scale}-percent-tracks.png').write_bytes(base64.b64decode(shot))
            assert execute('return document.documentElement.scrollWidth <= innerWidth;'), f'Horizontal overflow at {width}px/{scale}%'
            assert execute("return document.querySelector('.status-strip').getBoundingClientRect().bottom <= innerHeight + 1;"), 'Status strip outside viewport'
            assert execute("return document.querySelector('.mode-status').textContent === 'Shuffle: On · Repeat: Song';"), 'Modes lost during navigation'
            geometry.append(execute("""
                const player = document.querySelector('.player');
                const cover = document.querySelector('.cover');
                return {width: innerWidth, height: innerHeight, scale: arguments[0],
                    playerHeight: player.clientHeight, playerScrollHeight: player.scrollHeight,
                    artworkHeight: cover.clientHeight};
            """, scale))
            if width == 1360 and height == 940 and scale == 100:
                assert geometry[-1]['playerScrollHeight'] <= geometry[-1]['playerHeight'] + 1, 'Full player does not fit at preferred size'
            execute("document.querySelector('.transport button').focus();")
            execute("document.activeElement.scrollIntoView({block: 'nearest'});")
            visible_focus()
            key('\ue004')
            visible_focus()
            assert execute("""
                const buttons = [...document.querySelectorAll('.transport button')].map(button => button.getBoundingClientRect());
                return buttons.length === 5 && buttons.every(box => box.width >= 28 && box.height >= 28) &&
                    buttons.every((box, i) => !i || Math.abs(box.width - buttons[0].width) < 1 && box.left >= buttons[i-1].right);
            """), 'Five controls clipped, overlapping or uneven'
            (output / f'melora-linux-{width}-{height}-{scale}-percent-controls.png').write_bytes(base64.b64decode(call('GET', '/screenshot')))
            execute("document.querySelectorAll('.nav-item')[2].click();")
            wait_script("return !!document.querySelector('select');")
        assert execute("return document.querySelector('main').clientHeight > 180;"), 'Content pane collapsed at 200% scale'
        execute("document.querySelector('main').scrollIntoView();")
        assert execute('return scrollY === 0 && document.documentElement.scrollHeight <= innerHeight + 1;'), 'Shell overflow moved the persistent status strip'
        shot = call('GET', '/screenshot')
        (output / 'melora-linux-200-percent.png').write_bytes(base64.b64decode(shot))
        call('POST', '/window/rect', {'width': 1360, 'height': 850})
        execute("const el=document.querySelector('select');el.value='125';el.dispatchEvent(new Event('change',{bubbles:true}));")
        wait_script("return document.documentElement.style.fontSize === '125%';")
        preferences_path = Path(data) / 'app.melora.desktop/preferences.json'
        for _ in range(80):
            if preferences_path.exists() and json.loads(preferences_path.read_text())['scale'] == 125:
                break
            time.sleep(0.1)
        assert json.loads(preferences_path.read_text())['scale'] == 125
        call('DELETE', '')
        session = launch()
        wait_script("return document.documentElement.style.fontSize === '125%';")
        assert 'v0.1.0 / linux' in execute('return document.body.innerText;')
        assert execute("return document.querySelector('.transport button').getAttribute('aria-label') === 'Shuffle: Off (preview)' && document.querySelector('.transport button:last-child').getAttribute('aria-label') === 'Repeat: Off (preview)';"), 'Preview modes persisted after relaunch'
        shot = call('GET', '/screenshot')
        (output / 'melora-linux-restored.png').write_bytes(base64.b64decode(shot))
        (output / 'native-acceptance.json').write_text(json.dumps({
            'platform': 'linux', 'ipc': 'passed', 'keyboard_selection': 'passed',
            'local_fonts': 'passed', 'scaling': '100–200%, 720 and 1360px',
            'status_strip': 'visible', 'preferences_relaunch': 'passed',
            'brand_icon': 'loaded', 'compact_header': 'passed',
            'preferred_opening_size': opening_size, 'preferred_player_fit': 'no scrollbar',
            'preview_modes': 'keyboard toggle/cycle, navigation retention and relaunch reset passed',
            'player_geometry': geometry,
            'os_dpi_changes': 'not tested; scale/decorations arithmetic unit-tested',
            'text_contrast_minimum': min(pair['ratio'] for pair in contrast),
            'stream_quality': 'unavailable, no fabricated bitrate',
            'windows_runtime': 'not tested', 'macos_runtime': 'not tested',
        }, indent=2) + '\n')
        print('Native Linux acceptance passed: IPC, fonts, keyboard focus/selection, scaling, status strip and preferences after relaunch.')
    finally:
        if session:
            try:
                request('DELETE', '/session/' + session)
            except (OSError, urllib.error.URLError):
                pass
        driver.terminate()
        try:
            driver.wait(timeout=5)
        except subprocess.TimeoutExpired:
            driver.kill()
