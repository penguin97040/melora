"""Linux WebKitGTK acceptance checks against the real Tauri executable.

Run after `npm run tauri build -- --debug --no-bundle`:
python3 scripts/test-native.py /absolute/path/to/melora
Requires Xvfb, xdotool and WebKitWebDriver. Uses isolated disposable application data.
"""
import base64
import argparse
import json
import os
import re
import signal
import socket
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
parser.add_argument('--chrome-only', action='store_true', help='Exercise native window controls under Openbox/Xvfb')
args = parser.parse_args()
if not re.fullmatch(r'[1-9][0-9]*x[1-9][0-9]*', args.screen):
    parser.error('Screen must be WIDTHxHEIGHT in physical pixels')
screen_width, screen_height = map(int, args.screen.split('x'))
exe = str(Path(args.executable).resolve())
output = Path('artifacts/runtime')
output.mkdir(parents=True, exist_ok=True)
with tempfile.TemporaryDirectory(prefix='melora-runtime-') as data:
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        port = listener.getsockname()[1]
    driver = subprocess.Popen(['xvfb-run', '-a', '-s', f'-screen 0 {args.screen}x24', 'env', 'TAURI_WEBVIEW_AUTOMATION=true',
                               f'GDK_SCALE={args.device_scale}',
                               'WEBKIT_DISABLE_COMPOSITING_MODE=1', 'XDG_DATA_HOME=' + data,
                               'python3', '-c',
                               "import os,sys,json,subprocess;from pathlib import Path;Path(sys.argv[1]).write_text(json.dumps({key:os.environ[key] for key in ['DISPLAY','XAUTHORITY']}));subprocess.Popen(['openbox'],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL) if sys.argv[3]=='chrome' else None;os.execvp('WebKitWebDriver',['WebKitWebDriver','--port='+sys.argv[2]])",
                               str(Path(data) / 'display'), str(port), 'chrome' if args.chrome_only else 'plain'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    base = f'http://127.0.0.1:{port}'
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
            result = request(method, '/session/' + session + route, payload)
            if method == 'POST' and route == '/window/rect':
                # Unmanaged X11 loses activation on some WebDriver resizes.
                # Simulate the focused desktop window without altering CSS.
                subprocess.run(['xdotool', 'search', '--onlyvisible', '--name', '^Melora$', 'windowfocus'],
                               env={**os.environ, **json.loads((Path(data) / 'display').read_text())},
                               check=True, stdout=subprocess.DEVNULL)
            return result
        def execute(script, *args):
            return call('POST', '/execute/sync', {'script': script, 'args': list(args)})
        def wait_script(script):
            for _ in range(80):
                if execute(script):
                    return
                time.sleep(0.1)
            raise AssertionError('Native UI assertion timed out: ' + str(execute("return {errors:[...document.querySelectorAll('[role=alert]')].map(e=>e.textContent), clicks:window.__chromeClicks};")))
        wait_script("return document.body.innerText.includes('v0.1.0 / linux');")
        # Exercise the native boundary rather than only mocking frontend wrappers.
        execute("""
            window.__securityReview = null;
            const invoke = window.__TAURI_INTERNALS__.invoke;
            Promise.all([
                invoke('plugin:window|set_title', {label: 'main', title: 'Synthetic denied title'}),
                invoke('plugin:event|emit', {event: 'synthetic-review-event', payload: null}),
                invoke('save_preferences', {preferences: {scale: 101, comfortable: false, volume: 70}}),
                invoke('save_preferences', {preferences: {scale: 100, comfortable: false, volume: 70, extra: true}}),
            ].map(operation => operation.then(() => false, () => true)))
                .then(results => { window.__securityReview = results; });
        """)
        wait_script('return Array.isArray(window.__securityReview);')
        assert execute('return window.__securityReview.every(Boolean);'), 'Unapproved IPC or invalid settings accepted'
        original_url = execute('return location.href;')
        execute("location.assign('https://example.invalid/'); return true;")
        time.sleep(0.15)
        assert execute('return location.href;') == original_url, 'Remote main-window navigation accepted'
        execute("window.open('https://example.invalid/', '_blank'); return true;")
        time.sleep(0.15)
        assert len(call('GET', '/window/handles')) == 1, 'Unexpected popup window'
        opening_size = execute('return {width: innerWidth, height: innerHeight};')
        opening_rect = call('GET', '/window/rect')
        assert abs(opening_rect['x'] - (screen_width / args.device_scale - opening_rect['width']) / 2) <= 1 and abs(opening_rect['y'] - (screen_height / args.device_scale - opening_rect['height']) / 2) <= 1, f'Opening window not centred: {opening_rect}, client {opening_size}'
        if args.opening_only:
            assert opening_size == {
                'width': min(1440, screen_width // args.device_scale),
                'height': min(940, screen_height // args.device_scale),
            }, f'Opening size not bounded to simulated monitor: {opening_size}'
            rect = call('GET', '/window/rect')
            assert rect['x'] >= 0 and rect['y'] >= 0, 'Opening position outside monitor'
            assert rect['x'] + rect['width'] <= screen_width / args.device_scale and rect['y'] + rect['height'] <= screen_height / args.device_scale, 'Outer window outside monitor'
            evidence = {'screen': args.screen, 'device_scale': args.device_scale,
                        'opening_client': opening_size, 'outer_rect': rect,
                        'security': 'native IPC denials, invalid preferences, remote navigation and popup rejection passed',
                        'rect_units': 'GTK/WebDriver logical pixels',
                        'decorations': 'No window manager in Xvfb; subtraction unit-tested',
                        'actual_os_dpi': 'not tested'}
            (output / f'opening-{args.screen}-{args.device_scale}.json').write_text(json.dumps(evidence, indent=2) + '\n')
            print(f'Native opening bounds passed: {args.screen} physical pixels, simulated GTK scale {args.device_scale}.')
            sys.exit(0)
        assert opening_size == {'width': 1440, 'height': 940}, 'Preferred opening client size not applied'
        assert execute("return document.querySelector('.player').scrollHeight <= document.querySelector('.player').clientHeight + 1;"), 'Preferred player requires scrolling'
        def check_device_footer():
            result = execute("""
                const footer = document.querySelector('.status-strip');
                const device = footer.querySelector('.playback-device');
                const spacer = document.querySelector('.player-device-spacer');
                const bounds = footer.getBoundingClientRect();
                const range = document.createRange();
                const textVisible = [...footer.children].every(item => {
                    range.selectNodeContents(item);
                    return [...range.getClientRects()].every(box =>
                        box.left >= bounds.left && box.right <= bounds.right &&
                        box.top >= bounds.top && box.bottom <= bounds.bottom);
                });
                const css = getComputedStyle(spacer);
                const scale = parseFloat(getComputedStyle(document.documentElement).fontSize) / 16;
                return {textVisible, footerHeight: bounds.height,
                    ordered: footer.children[0].textContent.includes('Synthetic preview') &&
                        footer.children[1] === device && footer.children[2].className === 'stream-quality',
                    status: device.textContent.trim().replace(/\\s+/g, ' '),
                    spacerHeight: spacer.getBoundingClientRect().height,
                    minimumSpace: 40 * scale + parseFloat(css.paddingTop),
                    blank: spacer.textContent === '' && spacer.children.length === 0 &&
                        spacer.getAttribute('aria-hidden') === 'true' && spacer.tabIndex === -1 &&
                        css.borderTopWidth === '0px',
                    playerDeviceAbsent: !document.querySelector('.player').textContent.includes('No device connected')};
            """)
            assert result['ordered'] and result['status'] == 'Playback device: No device connected', 'Device status misplaced or changed'
            assert result['textVisible'], f'Footer text clipped: {result}'
            assert result['blank'] and result['playerDeviceAbsent'], 'Former device area is not blank/non-interactive'
            assert result['spacerHeight'] >= result['minimumSpace'] - 1, f'Former device space collapsed: {result}'
            return result
        device_footer = check_device_footer()
        (output / 'melora-linux-preferred.png').write_bytes(base64.b64decode(call('GET', '/screenshot')))
        def artwork_geometry():
            return execute("""
                const player = document.querySelector('.player');
                const cover = document.querySelector('.cover');
                const box = cover.getBoundingClientRect();
                const pane = player.getBoundingClientRect();
                const label = player.querySelector('.panel-label').getBoundingClientRect();
                const area = player.querySelector('.player-artwork').getBoundingClientRect();
                const details = player.querySelector('.player-body').getBoundingClientRect();
                const stacked = getComputedStyle(player).containerType === 'normal';
                const minimum = parseFloat(getComputedStyle(document.documentElement).fontSize) * 6;
                const room = player.clientHeight - label.height - details.height;
                const decorationInside = ['.cover-mark', '.cover-caption'].every(selector => {
                    const b = cover.querySelector(selector).getBoundingClientRect();
                    return b.left >= box.left && b.right <= box.right &&
                        b.top >= box.top && b.bottom <= box.bottom;
                });
                return {width: box.width, height: box.height, playerWidth: player.clientWidth,
                    playerHeight: player.clientHeight, playerScrollHeight: player.scrollHeight,
                    centred: Math.abs(box.left + box.width / 2 -
                        (pane.left + player.clientLeft + player.clientWidth / 2)) <= 1,
                    gap: box.top - label.bottom, border: getComputedStyle(cover).borderTopWidth,
                    areaHeight: area.height, minimum, room, stacked, decorationInside,
                    expectedSize: stacked ? player.clientWidth :
                        Math.min(player.clientWidth, Math.max(minimum, room))};
            """)
        def check_artwork():
            artwork = artwork_geometry()
            assert abs(artwork['width'] - artwork['height']) <= 1, f'Artwork not square: {artwork}'
            assert artwork['centred'] and artwork['width'] <= artwork['playerWidth'] + 1, f'Artwork not centred/bounded: {artwork}'
            assert abs(artwork['height'] - artwork['expectedSize']) <= 2, f'Artwork not using available height: {artwork}'
            assert artwork['gap'] == 0 and artwork['border'] == '0px', f'Artwork has gutter or border: {artwork}'
            assert artwork['decorationInside'], f'Synthetic decoration outside artwork: {artwork}'
            if not artwork['stacked']:
                assert artwork['height'] >= artwork['minimum'] - 1, f'Artwork below minimum: {artwork}'
                if artwork['room'] >= artwork['minimum']:
                    assert artwork['playerScrollHeight'] <= artwork['playerHeight'] + 1, f'Player scrolls while content fits: {artwork}'
                else:
                    assert artwork['playerScrollHeight'] > artwork['playerHeight'], f'Fallback scrolling missing: {artwork}'
            return artwork
        artwork = check_artwork()
        assert abs(artwork['height'] - 330.1875) <= 1, 'Device move changed preferred artwork sizing'
        assert execute("return document.querySelector('.masthead').getBoundingClientRect().height === 36;"), 'Header not 36 logical pixels'
        assert execute("return document.querySelector('.app').classList.contains('chrome-custom') && document.querySelectorAll('.window-controls button:not(:disabled)').length === 3 && document.querySelectorAll('.resize-target').length === 8;"), 'Native chrome not ready'
        assert execute("return document.querySelector('button[aria-label=Play]').disabled;")
        assert execute("return document.querySelector('.stream-quality').textContent === 'Stream quality: unavailable';")
        wait_script("return document.querySelector('.brand-symbol').complete && document.querySelector('.brand-symbol').naturalWidth > 0;")
        assert execute("return document.fonts.check('16px \"Share Tech Mono\"') && document.fonts.check('16px \"Chakra Petch\"') && document.fonts.check('16px \"Ubuntu Mono\"');")
        assert execute("return getComputedStyle(document.querySelector('.content')).backgroundColor === 'rgb(14, 33, 24)' && getComputedStyle(document.querySelector('.playlist-item.selected')).backgroundColor === 'rgb(18, 42, 30)';"), 'Centre/side panel backgrounds changed incorrectly'
        contrast = execute("""
            const css = getComputedStyle(document.documentElement);
            const names = ['--text', '--muted', '--cyan', '--orange', '--active-mode'];
            const surfaces = ['--background', '--surface-content', '--surface', '--surface-raised', '--control', '--selection'];
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
        if args.chrome_only:
            display_env = {**os.environ, **json.loads((Path(data) / 'display').read_text())}
            def desktop(*parts):
                return subprocess.check_output(list(parts), env=display_env, text=True).strip()
            wid = desktop('xdotool', 'search', '--onlyvisible', '--name', '^Melora$').splitlines()[0]
            def click(selector):
                box = execute("""const b=document.querySelector(arguments[0]).getBoundingClientRect();return {x:b.x+b.width/2,y:b.y+b.height/2};""", selector)
                rect = call('GET', '/window/rect')
                desktop('xdotool', 'mousemove', str(round(rect['x']+box['x'])), str(round(rect['y']+box['y'])), 'click', '1')
            click('[aria-label="Maximise window"]')
            wait_script("""return !!document.querySelector('[aria-label="Restore window"]') && !document.querySelector('.resize-target');""")
            assert call('GET', '/window/rect')['width'] == screen_width
            click('[aria-label="Restore window"]')
            wait_script("""return !!document.querySelector('[aria-label="Maximise window"]') && document.querySelectorAll('.resize-target').length === 8;""")
            execute("window.__chromeClicks=[];document.addEventListener('mousedown',e=>window.__chromeClicks.push({detail:e.detail,target:e.target.tagName,x:e.clientX,y:e.clientY}),true);")
            time.sleep(.6)  # Let the native click count reset before dragging.
            rect = call('GET', '/window/rect')
            desktop('xdotool', 'mousemove', str(rect['x']+400), str(rect['y']+18), 'mousedown', '1', 'sleep', '.08', 'mouseup', '1', 'sleep', '.08', 'mousedown', '1', 'sleep', '.08', 'mouseup', '1')
            wait_script("""return !!document.querySelector('[aria-label="Restore window"]');""")
            time.sleep(.6)  # Start a fresh double-click, rather than a quadruple-click.
            desktop('xdotool', 'mousemove', '400', '18', 'mousedown', '1', 'sleep', '.08', 'mouseup', '1', 'sleep', '.08', 'mousedown', '1', 'sleep', '.08', 'mouseup', '1')
            wait_script("""return !!document.querySelector('[aria-label="Maximise window"]');""")
            time.sleep(.6)  # Let the native click count reset before dragging.
            rect = call('GET', '/window/rect')
            desktop('xdotool', 'mousemove', str(rect['x']+400), str(rect['y']+18), 'mousedown', '1', 'sleep', '.2', 'mousemove', str(rect['x']+430), str(rect['y']+48), 'sleep', '.2', 'mouseup', '1')
            time.sleep(.2)
            moved = call('GET', '/window/rect')
            assert abs(moved['x']-rect['x']-30) <= 1 and abs(moved['y']-rect['y']-30) <= 1, f'Header dragging failed: {rect} -> {moved}'
            # Resize every thin edge/corner with real native mouse events.
            resize_results = []
            for edge in ['North','South','East','West','NorthEast','NorthWest','SouthEast','SouthWest']:
                call('POST', '/window/rect', {'x':200,'y':100,'width':1000,'height':700})
                before = call('GET', '/window/rect')
                box = execute("""const b=document.querySelector('[data-resize-edge="'+arguments[0]+'"]').getBoundingClientRect();return {x:b.x+b.width/2,y:b.y+b.height/2};""", edge)
                px,py = round(before['x']+box['x']), round(before['y']+box['y'])
                dx = 20 if 'East' in edge else -20 if 'West' in edge else 0
                dy = 20 if 'South' in edge else -20 if 'North' in edge else 0
                desktop('xdotool','mousemove',str(px),str(py),'mousedown','1','sleep','.2','mousemove',str(px+dx),str(py+dy),'sleep','.2','mouseup','1')
                time.sleep(.2)
                after = call('GET','/window/rect')
                assert after['width'] == before['width']+(20 if dx else 0) and after['height'] == before['height']+(20 if dy else 0), f'Resize {edge} failed: {before} -> {after}'
                resize_results.append({'edge':edge,'before':before,'after':after})
            # Keyboard access must invoke minimise, without a header drag.
            execute("""document.querySelector('[aria-label="Minimise window"]').focus();""")
            call('POST','/actions',{'actions':[{'type':'key','id':'keyboard','actions':[
                {'type':'keyDown','value':'\ue004'},{'type':'keyUp','value':'\ue004'},
                {'type':'keyDown','value':'\ue008'},
                {'type':'keyDown','value':'\ue004'},{'type':'keyUp','value':'\ue004'},
                {'type':'keyUp','value':'\ue008'}]}]})
            assert execute("""const b=document.activeElement.getBoundingClientRect(); const h=document.querySelector('.masthead').getBoundingClientRect(); return document.activeElement.getAttribute('aria-label') === 'Minimise window' && getComputedStyle(document.activeElement).outlineStyle !== 'none' && b.top >= h.top && b.bottom <= h.bottom;"""), 'Window keyboard focus missing or clipped'
            (output / 'melora-linux-header-focus.png').write_bytes(base64.b64decode(call('GET', '/screenshot')))
            call('POST','/actions',{'actions':[{'type':'key','id':'keyboard','actions':[{'type':'keyDown','value':'\ue007'},{'type':'keyUp','value':'\ue007'}]}]})
            for _ in range(40):
                if 'Iconic' in desktop('xprop','-id',wid,'WM_STATE'): break
                time.sleep(.1)
            assert 'Iconic' in desktop('xprop','-id',wid,'WM_STATE'), 'Minimise did not iconify native window'
            desktop('xdotool','windowmap',wid,'windowactivate','--sync',wid)
            wait_script("""return document.hasFocus();""")
            click('[aria-label="Close window"]')
            for _ in range(40):
                result = subprocess.run(['xdotool','getwindowname',wid],env=display_env,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
                if result.returncode: break
                time.sleep(.1)
            assert result.returncode, 'Close did not destroy native window'
            session = None
            (output / 'chrome-acceptance.json').write_text(json.dumps({'environment':'Linux X11 Openbox under Xvfb; no real desktop or Wayland', 'controls':'native minimise/maximise/restore/close passed', 'header':'native drag and double click passed', 'resize_edges':resize_results, 'windows_macos':'unavailable'},indent=2)+'\n')
            print('Native Linux/Openbox chrome passed: controls, keyboard minimise, drag/double-click, all resize edges/corners.')
            sys.exit(0)
        # Resizing must shrink the square before overflow, then restore its width cap.
        height_sequence = []
        def fixed_player_sizes():
            return execute("""
                return ['.track-title', '.artist-name', '.player-footnote', '.transport button'].map(selector => {
                    const e = document.querySelector(selector), css = getComputedStyle(e);
                    return {font: css.fontSize, height: selector.includes('button') ? e.clientHeight : null};
                });
            """)
        fixed_sizes = fixed_player_sizes()
        for height in [940, 700, 600, 520, 600, 700, 940]:
            call('POST', '/window/rect', {'width': 1440, 'height': height})
            height_sequence.append({'windowHeight': height, **check_artwork()})
            assert fixed_player_sizes() == fixed_sizes, 'Text or controls scaled with artwork'
            (output / f'melora-linux-artwork-height-{height}.png').write_bytes(base64.b64decode(call('GET', '/screenshot')))
        assert height_sequence[1]['height'] < height_sequence[0]['height'], 'Shorter pane did not shrink artwork'
        assert height_sequence[1]['playerScrollHeight'] <= height_sequence[1]['playerHeight'] + 1, 'Artwork did not shrink before scrolling'
        assert height_sequence[3]['height'] == height_sequence[3]['minimum'], 'Fallback did not retain 6rem artwork'
        assert height_sequence[3]['playerScrollHeight'] > height_sequence[3]['playerHeight'], 'Small-window fallback not exercised'
        assert height_sequence[-1]['height'] == height_sequence[0]['height'], 'Artwork did not recover after increasing height'
        # Synthetic text stress checks intrinsic wrapping without changing production preview data.
        call('POST', '/window/rect', {'width': 1440, 'height': 700})
        original_title = execute("return document.querySelector('.track-title').textContent;")
        execute("document.querySelector('.track-title').textContent = 'Synthetic wrapping title for a quiet evening with exceptionally long words';")
        wrapping = check_artwork()
        assert wrapping['height'] < height_sequence[1]['height'], 'Wrapped text did not receive artwork space'
        assert fixed_player_sizes() == fixed_sizes, 'Wrapping changed text/control sizes'
        (output / 'melora-linux-artwork-wrapping.png').write_bytes(base64.b64decode(call('GET', '/screenshot')))
        execute("document.querySelector('.track-title').textContent = arguments[0];", original_title)
        call('POST', '/window/rect', {'width': 1440, 'height': 940})
        # Actual keyboard events, rather than a click simulation.
        def key(value):
            call('POST', '/actions', {'actions': [{'type': 'key', 'id': 'keyboard', 'actions': [
                {'type': 'keyDown', 'value': value}, {'type': 'keyUp', 'value': value}]}]})
        def visible_focus():
            assert execute("return getComputedStyle(document.activeElement).outlineStyle !== 'none';"), 'Keyboard focus outline missing: ' + str(execute("return {width:innerWidth,height:innerHeight,scale:document.documentElement.style.fontSize,focus:document.hasFocus(),active:document.activeElement.outerHTML,match:document.activeElement.matches(':focus-visible')};"))
            assert execute("""
                const box = document.activeElement.getBoundingClientRect();
                const player = document.querySelector('.player').getBoundingClientRect();
                return box.left - 3 >= player.left && box.right + 3 <= player.right &&
                    box.top - 3 >= Math.max(player.top, document.querySelector('.masthead').getBoundingClientRect().bottom) &&
                    box.bottom + 3 <= Math.min(player.bottom, document.querySelector('.status-strip').getBoundingClientRect().top);
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
        cases = [(width, height, scale) for width, heights in [(1440, [940, 820, 660, 520]), (1360, [940, 660, 520])] for height in heights for scale in [100, 125, 150, 175, 200]]
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
            artwork = check_artwork()
            check_device_footer()
            geometry.append(execute("""
                const player = document.querySelector('.player');
                const cover = document.querySelector('.cover');
                return {width: innerWidth, height: innerHeight, scale: arguments[0],
                    playerHeight: player.clientHeight, playerScrollHeight: player.scrollHeight,
                    artworkHeight: cover.clientHeight};
            """, scale))
            if width == 1440 and height == 940 and scale == 100:
                assert geometry[-1]['playerScrollHeight'] <= geometry[-1]['playerHeight'] + 1, 'Full player does not fit at preferred size'
            execute("document.querySelector('.transport button').focus();")
            # Establish keyboard modality after native resize/reflow.
            # Use a real keyboard Tab before checking the keyboard outline.
            key('\ue004')
            execute("document.activeElement.scrollIntoView({block: 'nearest'});")
            visible_focus()
            call('POST', '/actions', {'actions': [{'type': 'key', 'id': 'keyboard', 'actions': [
                {'type': 'keyDown', 'value': '\ue008'},
                {'type': 'keyDown', 'value': '\ue004'}, {'type': 'keyUp', 'value': '\ue004'},
                {'type': 'keyUp', 'value': '\ue008'}]}]})
            execute("document.activeElement.scrollIntoView({block: 'nearest'});")
            visible_focus()
            assert execute("""
                const buttons = [...document.querySelectorAll('.transport button')].map(button => button.getBoundingClientRect());
                return buttons.length === 5 && buttons.every(box => box.width >= 28 && box.height >= 28) &&
                    buttons.every((box, i) => !i || Math.abs(box.width - buttons[0].width) < 1 && box.left >= buttons[i-1].right);
            """), 'Five controls clipped, overlapping or uneven'
            (output / f'melora-linux-{width}-{height}-{scale}-percent-controls.png').write_bytes(base64.b64decode(call('GET', '/screenshot')))
            execute("document.querySelector('.player-footnote').scrollIntoView({block: 'end'});")
            assert execute("""
                const note = document.querySelector('.player-footnote').getBoundingClientRect();
                return note.top >= document.querySelector('.masthead').getBoundingClientRect().bottom &&
                    note.bottom <= document.querySelector('.status-strip').getBoundingClientRect().top;
            """), 'Fallback scrolling cannot expose full player details'
            assert execute('return scrollY === 0;'), 'Player scrolling moved the whole document'
            execute("document.querySelectorAll('.nav-item')[2].click();")
            wait_script("return !!document.querySelector('select');")
        assert execute("return document.querySelector('main').clientHeight > 180;"), 'Content pane collapsed at 200% scale'
        execute("document.querySelector('main').scrollIntoView();")
        assert execute('return scrollY === 0 && document.documentElement.scrollHeight <= innerHeight + 1;'), 'Shell overflow moved the persistent status strip'
        shot = call('GET', '/screenshot')
        (output / 'melora-linux-200-percent.png').write_bytes(base64.b64decode(shot))
        call('POST', '/window/rect', {'width': 1360, 'height': 850, 'x': 30, 'y': 40})
        user_rect = call('GET', '/window/rect')
        execute("window.__revealTestDone = false; window.__TAURI_INTERNALS__.invoke('show_main_window').then(() => window.__TAURI_INTERNALS__.invoke('show_main_window')).then(() => { window.__revealTestDone = true; });")
        wait_script('return window.__revealTestDone;')
        assert call('GET', '/window/rect') == user_rect, 'Duplicate readiness changed user geometry'
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
        # Spotify account: the real ora plugin over IPC, with a synthetic Client ID only.
        # Sign-in itself needs a browser and Spotify, so it is never started here.
        assert execute("return !!document.querySelector('.connection-note .link-button');"), 'Set up Spotify action missing'
        execute("document.querySelectorAll('.nav-item')[2].click();")
        wait_script("return !!document.querySelector('#spotify-client-id');")
        execute("document.querySelector('.account-settings').scrollIntoView({block: 'start'});")
        (output / 'melora-linux-settings-spotify-setup.png').write_bytes(base64.b64decode(call('GET', '/screenshot')))
        execute("""
            window.__oraReview = null;
            const invoke = window.__TAURI_INTERNALS__.invoke;
            const rejected = (operation) => operation.then(() => null, (error) => error);
            Promise.all([
                invoke('plugin:ora|status'),
                rejected(invoke('plugin:ora|set_client_id', {clientId: 'not a client id'})),
                rejected(invoke('plugin:ora|open_in_spotify', {url: 'https://example.invalid/'})),
                rejected(invoke('plugin:ora|open_in_spotify', {url: 'file:///etc/passwd'})),
            ]).then(results => { window.__oraReview = results; });
        """)
        wait_script('return Array.isArray(window.__oraReview);')
        ora_review = execute('return window.__oraReview;')
        assert ora_review[0]['auth'] == {'kind': 'notConfigured'}, f'Unexpected initial account status: {ora_review[0]}'
        # The Dashboard needs this exact URI; the app must send and show the same one.
        assert ora_review[0]['redirectUri'] == 'http://127.0.0.1:43821/callback', 'Unexpected redirect URI'
        assert execute("return document.querySelector('.redirect-uri code').textContent;") == 'http://127.0.0.1:43821/callback', 'Settings shows a different redirect URI'
        assert ora_review[1] == {'kind': 'invalidClientId'}, 'Invalid Client ID accepted'
        assert ora_review[2] == {'kind': 'unsupported'} and ora_review[3] == {'kind': 'unsupported'}, 'Non-Spotify URL accepted'
        synthetic_id = '0123456789abcdef' * 2
        execute("""
            const field = document.querySelector('#spotify-client-id');
            Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(field, arguments[0]);
            field.dispatchEvent(new Event('input', {bubbles: true}));
        """, synthetic_id)
        wait_script("return !document.querySelector('.client-id-form button').disabled;")
        execute("document.querySelector('.client-id-form button').click();")
        wait_script("return [...document.querySelectorAll('.account-actions button')].some(b => b.textContent === 'Sign in with Spotify');")
        spotify_path = Path(data) / 'app.melora.desktop/spotify.json'
        assert json.loads(spotify_path.read_text()) == {'clientId': synthetic_id}, 'Client ID not saved beside preferences'
        assert execute("return document.querySelector('.connection-note').textContent.includes('Spotify not connected');"), 'Sidebar status not updated'
        assert execute("return document.querySelector('#spotify-client-id').placeholder === 'Saved: 0123…cdef' && !document.body.innerText.includes(arguments[0]);", synthetic_id), 'Full Client ID shown in the UI'
        execute("document.querySelector('.account-settings').scrollIntoView({block: 'start'});")
        (output / 'melora-linux-settings-spotify.png').write_bytes(base64.b64decode(call('GET', '/screenshot')))
        (output / 'native-acceptance.json').write_text(json.dumps({
            'spotify_account': 'plugin status, invalid Client ID and URL rejection, masked save and Settings screenshots passed; sign-in not run',
            'platform': 'linux', 'ipc': 'passed', 'keyboard_selection': 'passed',
            'local_fonts': 'passed', 'scaling': '100–200%, 720, 1360 and 1440px',
            'status_strip': 'visible', 'preferences_relaunch': 'passed',
            'device_footer': device_footer,
            'brand_icon': 'loaded', 'artwork_geometry': artwork, 'duplicate_readiness_user_geometry': 'passed', 'compact_header': 'passed',
            'preferred_opening_size': opening_size, 'preferred_player_fit': 'no scrollbar',
            'preview_modes': 'keyboard toggle/cycle, navigation retention and relaunch reset passed',
            'player_geometry': geometry, 'artwork_height_sequence': height_sequence,
            'artwork_wrapping': wrapping,
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
        # Stop the whole Xvfb/driver process group, including wrapper children.
        try:
            os.killpg(driver.pid, signal.SIGTERM)
            driver.wait(timeout=5)
        except (ProcessLookupError, subprocess.TimeoutExpired):
            pass
        finally:
            try:
                os.killpg(driver.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            driver.wait(timeout=5)
