"""Linux WebKitGTK acceptance checks against the real Tauri executable.

Run after `npm run tauri build -- --debug --no-bundle`:
python3 scripts/test-native.py /absolute/path/to/melora
Requires Xvfb and WebKitWebDriver. Uses isolated disposable application data.
"""
import base64
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import urllib.request

exe = str(Path(sys.argv[1]).resolve())
output = Path('artifacts/runtime')
output.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(prefix='melora-runtime-') as data:
    driver = subprocess.Popen(['xvfb-run', '-a', 'env', 'TAURI_WEBVIEW_AUTOMATION=true',
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
        assert execute("return document.querySelector('button[aria-label=Play]').disabled;")
        assert execute("return document.querySelector('.stream-quality').textContent === 'Stream quality: unavailable';")
        wait_script("return document.querySelector('.brand-symbol').complete && document.querySelector('.brand-symbol').naturalWidth > 0;")
        assert execute("return document.fonts.check('16px \"Share Tech Mono\"') && document.fonts.check('16px \"Chakra Petch\"') && document.fonts.check('16px \"Ubuntu Mono\"');")
        contrast = execute("""
            const css = getComputedStyle(document.documentElement);
            const names = ['--text', '--muted', '--cyan', '--orange'];
            const surfaces = ['--background', '--surface', '--surface-raised', '--selection'];
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
        execute("document.querySelectorAll('.playlist-item')[2].focus();")
        call('POST', '/actions', {'actions': [{'type': 'key', 'id': 'keyboard', 'actions': [
            {'type': 'keyDown', 'value': '\ue007'}, {'type': 'keyUp', 'value': '\ue007'}]}]})
        wait_script("return document.querySelector('h1').textContent === 'Soft machinery';")
        execute("document.querySelectorAll('.track-select')[1].focus();")
        call('POST', '/actions', {'actions': [{'type': 'key', 'id': 'keyboard', 'actions': [
            {'type': 'keyDown', 'value': '\ue007'}, {'type': 'keyUp', 'value': '\ue007'}]}]})
        wait_script("return document.querySelector('.track-title').textContent === 'Amber lines';")
        assert execute("return getComputedStyle(document.activeElement).outlineStyle !== 'none';")
        execute("document.querySelectorAll('.nav-item')[2].click();")
        wait_script("return !!document.querySelector('select');")
        for width, scale in [(1360, 100), (1360, 125), (1360, 150), (1360, 175), (1360, 200), (720, 100), (720, 200)]:
            call('POST', '/window/rect', {'width': width, 'height': 850})
            execute("const el=document.querySelector('select');el.value=String(arguments[0]);el.dispatchEvent(new Event('change',{bubbles:true}));", scale)
            wait_script(f"return document.documentElement.style.fontSize === '{scale}%';")
            execute("document.querySelectorAll('.nav-item')[0].click();")
            wait_script("return !!document.querySelector('.track-table');")
            assert execute("return document.querySelector('.masthead').getBoundingClientRect().height <= 37 * arguments[0] / 100;", scale), 'Header expanded beyond compact layout'
            assert execute("return document.querySelector('.track-select').getBoundingClientRect().width > 90;"), 'Track text space collapsed'
            assert execute("return document.querySelector('main').clientHeight > 180;"), 'Content pane collapsed'
            shot = call('GET', '/screenshot')
            (output / f'melora-linux-{width}-{scale}-percent.png').write_bytes(base64.b64decode(shot))
            execute("const track = document.querySelector('.track-select'); track.focus(); track.scrollIntoView({block: 'nearest'});")
            assert execute("const box = document.activeElement.getBoundingClientRect(); return box.top >= document.querySelector('.masthead').getBoundingClientRect().bottom - 1 && box.bottom <= document.querySelector('.status-strip').getBoundingClientRect().top + 1;"), 'Focused track obscured by persistent header/footer'
            assert execute('return scrollY === 0 && document.documentElement.scrollHeight <= innerHeight + 1;'), 'Track focus moved the whole document'
            shot = call('GET', '/screenshot')
            (output / f'melora-linux-{width}-{scale}-percent-tracks.png').write_bytes(base64.b64decode(shot))
            assert execute('return document.documentElement.scrollWidth <= innerWidth;'), f'Horizontal overflow at {width}px/{scale}%'
            assert execute("return document.querySelector('.status-strip').getBoundingClientRect().bottom <= innerHeight + 1;"), 'Status strip outside viewport'
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
        shot = call('GET', '/screenshot')
        (output / 'melora-linux-restored.png').write_bytes(base64.b64decode(shot))
        (output / 'native-acceptance.json').write_text(json.dumps({
            'platform': 'linux', 'ipc': 'passed', 'keyboard_selection': 'passed',
            'local_fonts': 'passed', 'scaling': '100–200%, 720 and 1360px',
            'status_strip': 'visible', 'preferences_relaunch': 'passed',
            'brand_icon': 'loaded', 'compact_header': 'passed',
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
