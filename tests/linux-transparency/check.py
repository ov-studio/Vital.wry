#!/usr/bin/env python3
"""Compositor-output regression; always creates its own headless Weston display.

Requires Weston 15 with Xwayland, weston-screenshooter, ImageMagick and Godot.
Outputs are kept outside the source tree. No active desktop sockets are inherited.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time

HERE = Path(__file__).resolve().parent

def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()

def state(out):
    try:
        return json.loads((out / 'state.json').read_text())
    except (ValueError, OSError):
        return {}

def wait_for(game, predicate, timeout=25):
    deadline = time.monotonic() + timeout
    while game.poll() is None and time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(.05)
    raise RuntimeError('Timed out waiting for fixture, or Godot exited')

def capture(out, name):
    with tempfile.TemporaryDirectory(dir=out) as tmp:
        env = dict(os.environ, XDG_PICTURES_DIR=tmp)
        subprocess.run(['weston-screenshooter'], cwd=tmp, env=env,
                       check=True, capture_output=True, timeout=10)
        files = list(Path(tmp).glob('*.png'))
        if len(files) != 1:
            raise RuntimeError('Expected one fresh compositor screenshot')
        target = out / (name + '.png')
        shutil.copy2(files[0], target)
    data = subprocess.check_output(['magick', str(target), '-depth', '8', 'rgb:-'])
    if len(data) != 960 * 640 * 3:
        raise RuntimeError('Unexpected compositor output dimensions')
    return data

def pixel(data, x, y):
    i = (y * 960 + x) * 3
    return list(data[i:i+3])

def background(x):
    return [24, 130, 184] if x // 120 % 2 == 0 else [229, 172, 32]

def inside(args):
    out = args.output
    if (os.environ.get('XDG_RUNTIME_DIR') != str(out / 'runtime')
            or os.environ.get('WAYLAND_DISPLAY') != 'vital-test'
            or not (out / 'runtime/vital-test').is_socket()):
        raise RuntimeError('Not on the owned compositor')
    result = {'status': 'FAIL', 'checks': []}
    game = None
    def check(name, passed, **details):
        result['checks'].append(dict(name=name, passed=bool(passed), **details))
    try:
        env = dict(os.environ, WRY_PROBE_OUTPUT=str(out), GDK_BACKEND='x11')
        with (out / 'godot.log').open('w') as log:
            game = subprocess.Popen([args.godot, '--path', str(out / 'project'),
                '--display-driver', 'x11', '--rendering-method', args.renderer,
                '--audio-driver', 'Dummy', '--max-fps', '60', '--position', '0,0'],
                env=env, stdout=log, stderr=subprocess.STDOUT)
        wait_for(game, lambda: state(out).get('ready_count', 0) > 0)
        check('html-ready', True)
        commands = ['initial', 'resize', 'hidden', 'shown', 'parent-hidden',
                    'parent-shown', 'changed-html', 'load-html'] + ['recreate'] * 10 + ['full-window', 'removed']
        old_ready = 0
        for index, phase in enumerate(commands):
            if index:
                old_ready = state(out).get('ready_count', 0)
                temp = out / 'command.tmp'
                temp.write_text(json.dumps({'id': index, 'phase': phase}))
                temp.replace(out / 'command.json')
                wait_for(game, lambda: state(out).get('command_id') == index)
                if phase in ('load-html', 'recreate'):
                    wait_for(game, lambda: state(out).get('ready_count', 0) > old_ready)
            if phase == 'recreate':
                origin = (160, 120)
            elif phase == 'initial':
                origin = (160, 120)
            elif phase == 'resize':
                origin = (80, 80)
            elif phase == 'full-window':
                origin = (0, 0)
            hidden = phase in ('hidden', 'parent-hidden', 'removed')
            x, y = origin
            points = {'clear': (x + 40, y + 200), 'opaque': (x + 290, y + 130),
                      'half': (x + 490, y + 130)}
            expected = {key: background(p[0]) for key, p in points.items()}
            if not hidden:
                expected['opaque'] = [32, 255, 32] if phase == 'changed-html' else [255, 32, 32]
                expected['half'] = [round((c + 255) / 2) for c in background(x + 490)]
            # DOM-ready isn't paint. Require three consecutive compositor frames.
            consecutive = 0
            deadline = time.monotonic() + 15
            samples = []
            motion = []
            start_frame = state(out).get('frame', 0)
            while time.monotonic() < deadline and game.poll() is None:
                data = capture(out, f'{index:02}-{phase}')
                observed = {key: pixel(data, *p) for key, p in points.items()}
                samples.append(observed)
                motion.append(hashlib.sha256(data[600*960*3:630*960*3]).hexdigest())
                ok = all(all(abs(a-b) <= 1 for a, b in zip(observed[k], v))
                         for k, v in expected.items())
                consecutive = consecutive + 1 if ok else 0
                if consecutive >= 3:
                    break
                time.sleep(.15)
            check(f'{index:02}-{phase}-pixels', consecutive >= 3,
                  expected=expected, samples=samples[-3:])
            check(f'{index:02}-{phase}-continuous-render',
                  len(set(motion)) > 1 and state(out).get('frame', 0) > start_frame)
        (out / 'command.json').write_text(json.dumps({'id': 999, 'phase': 'quit'}))
        game.wait(timeout=10)
        check('clean-exit', game.returncode == 0, exit_code=game.returncode)
        log = (out / 'godot.log').read_text(errors='replace')
        errors = [line for line in log.splitlines() if any(s in line for s in
                  ('ERROR:', 'panicked', 'already bound', 'Gdk-ERROR', 'WebView snapshot:'))]
        check('no-runtime-errors', not errors, errors=errors)
        result['renderer_log'] = [line for line in log.splitlines() if 'Using Device' in line]
        result['status'] = 'PASS' if all(c['passed'] for c in result['checks']) else 'FAIL'
    except Exception as error:
        result['error'] = str(error)
    finally:
        if game and game.poll() is None:
            game.terminate()
            try:
                game.wait(timeout=5)
            except subprocess.TimeoutExpired:
                game.kill()
                game.wait()
        (out / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    return 0 if result['status'] == 'PASS' else 1

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--library', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--godot', default='godot')
    parser.add_argument('--renderer', choices=['gl_compatibility', 'forward_plus'], required=True)
    parser.add_argument('--inside', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args()
    args.output = args.output.resolve()
    args.library = args.library.resolve()
    if args.inside:
        return inside(args)
    args.output.mkdir(parents=True, exist_ok=False)
    out = args.output
    (out / 'runtime').mkdir(mode=0o700)
    shutil.copytree(HERE, out / 'project', ignore=shutil.ignore_patterns('.godot', '*.so', '__pycache__'))
    shutil.copy2(args.library, out / 'project/wry.so')
    (out / 'project/.godot').mkdir()
    (out / 'project/.godot/extension_list.cfg').write_text('res://wry.gdextension\n')
    source = {str(p.relative_to(HERE.parent.parent)): digest(p)
              for directory in (HERE.parent.parent / 'src', HERE)
              for p in directory.rglob('*') if p.is_file() and '.godot' not in p.parts and '__pycache__' not in p.parts}
    receipt = {'library_sha256': digest(args.library), 'checker_sha256': digest(__file__),
               'source': source, 'renderer': args.renderer, 'argv': sys.argv,
               'head': subprocess.check_output(['git', '-C', str(HERE), 'rev-parse', 'HEAD'], text=True).strip()}
    (out / 'identity.json').write_text(json.dumps(receipt, indent=2) + '\n')
    env = dict(os.environ)
    for key in ('DISPLAY', 'WAYLAND_DISPLAY', 'WAYLAND_SOCKET', 'XAUTHORITY',
                'DBUS_SESSION_BUS_ADDRESS', 'LIBGL_ALWAYS_SOFTWARE', 'VK_DRIVER_FILES',
                'VK_ICD_FILENAMES', 'WEBKIT_DISABLE_DMABUF_RENDERER'):
        env.pop(key, None)
    env.update(XDG_RUNTIME_DIR=str(out / 'runtime'), GDK_BACKEND='x11')
    cmd = ['dbus-run-session', 'weston', '--backend=headless', '--renderer=gl',
           '--shell=kiosk', '--xwayland', '--no-xwm-decorations', '--no-config',
           '--debug', '--idle-time=0', '--socket=vital-test', '--width=960',
           '--height=640', '--fake-seat', '--', sys.executable, str(Path(__file__).resolve()),
           '--inside', '--output', str(out), '--library', str(args.library),
           '--renderer', args.renderer, '--godot', args.godot]
    with (out / 'weston.log').open('w') as log:
        compositor = subprocess.Popen(cmd, env=env, stdout=log, stderr=subprocess.STDOUT,
                                      start_new_session=True)
        try:
            deadline = time.monotonic() + 480
            while not (out / 'result.json').exists() and compositor.poll() is None and time.monotonic() < deadline:
                time.sleep(.2)
        finally:
            os.killpg(compositor.pid, signal.SIGTERM)
            try:
                compositor.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(compositor.pid, signal.SIGKILL)
                compositor.wait()
    if not (out / 'result.json').exists():
        raise RuntimeError('Compositor/runner failed; inspect weston.log')
    result = json.loads((out / 'result.json').read_text())
    print(result['status'], out)
    return 0 if result['status'] == 'PASS' else 1

if __name__ == '__main__':
    raise SystemExit(main())
