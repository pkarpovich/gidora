import functools
import http.server
import json
import os
import shutil
import signal
import socket
import stat
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
BROWSER = ROOT / 'build/src/out/Default/Gidora.app/Contents/MacOS/Gidora'
CTL = ROOT / 'gidoractl/target/release/gidoractl'
SOCKET_NAME = 'control.sock'
STARTUP_SECONDS = 40
REQUEST_CAP = 1024 * 1024

failures = []


def check(name, passed, detail=''):
    print(('PASS ' if passed else 'FAIL ') + name)
    if not passed:
        failures.append(name)
        if detail:
            print('     ' + str(detail)[:600])


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, format, *args):
        pass


class Fixture:
    def __enter__(self):
        handler = functools.partial(QuietHandler, directory=str(Path(__file__).parent))
        self.server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), handler)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.url = f'http://127.0.0.1:{self.server.server_address[1]}/page.html'
        return self

    def __exit__(self, *exc):
        self.server.shutdown()


class Browser:
    def __init__(self, url, enabled=True, extra=()):
        self.url = url
        self.enabled = enabled
        self.extra = list(extra)

    def __enter__(self):
        self.profile = Path(tempfile.mkdtemp(prefix='gidora-e2e-'))
        (self.profile / 'Default').mkdir()
        preferences = {'helium': {'completed_onboarding': True,
                                  'services': {'schema_version': 1, 'user_consented': False}}}
        (self.profile / 'Default' / 'Preferences').write_text(json.dumps(preferences))
        if self.enabled:
            local_state = {'gidora': {'control_socket': {'enabled': True}}}
            (self.profile / 'Local State').write_text(json.dumps(local_state))
        self.socket = self.profile / SOCKET_NAME
        self.process = subprocess.Popen(
            [str(BROWSER), f'--user-data-dir={self.profile}', '--use-mock-keychain', '--no-first-run',
             '--no-default-browser-check', '--window-size=1200,900', *self.extra, self.url],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.wait_until_started()
        return self

    def __exit__(self, *exc):
        self.process.send_signal(signal.SIGTERM)
        try:
            self.process.wait(timeout=30)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
        shutil.rmtree(self.profile, ignore_errors=True)

    def wait_until_started(self):
        deadline = time.time() + STARTUP_SECONDS
        while time.time() < deadline:
            if self.process.poll() is not None:
                raise SystemExit(f'the browser exited with {self.process.returncode} during startup')
            if self.enabled and self.socket.exists():
                return
            if not self.enabled and (self.profile / 'Default' / 'History').exists():
                time.sleep(3)
                return
            time.sleep(0.2)
        raise SystemExit('the browser did not start in time')

    def ctl(self, *args, stdin=None):
        environment = dict(os.environ, GIDORA_SOCKET=str(self.socket))
        return subprocess.run([str(CTL), *args], input=stdin, capture_output=True, env=environment, timeout=90)

    def raw(self, payload, timeout=20):
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as connection:
            connection.settimeout(timeout)
            connection.connect(str(self.socket))
            if payload:
                connection.sendall(payload)
            chunks = []
            while True:
                chunk = connection.recv(65536)
                if not chunk:
                    break
                chunks.append(chunk)
        return b''.join(chunks)

    def reply(self, request):
        return json.loads(self.raw((json.dumps(request) + '\n').encode()))


def text(result):
    return result.stdout.decode(errors='replace')


def error(result):
    return result.stderr.decode(errors='replace')


def disabled_cases(fixture):
    with Browser(fixture.url, enabled=False) as browser:
        check('a browser without the pref has no socket', not browser.socket.exists())
        result = browser.ctl('tab', 'list')
        check('the client reports a missing socket with exit code 1',
              result.returncode == 1 and 'Agent control socket' in error(result), error(result))


def transport_cases(browser):
    mode = browser.socket.stat().st_mode
    check('the socket is a unix socket with mode 0600',
          stat.S_ISSOCK(mode) and stat.S_IMODE(mode) == 0o600, oct(mode))

    result = browser.ctl('version')
    lines = text(result).splitlines()
    check('version prints the client and the browser line',
          result.returncode == 0 and len(lines) == 2 and lines[0].startswith('gidoractl ')
          and lines[1].startswith('Gidora ') and lines[1].split()[1][0].isdigit(), text(result) + error(result))

    result = browser.ctl('--json', 'version')
    line = text(result)
    check('--json prints the response line unchanged',
          result.returncode == 0 and line.endswith('\n') and json.loads(line)['ok'] is True, line)

    reply = browser.reply({'cmd': 'nope'})
    check('an unknown command is named in the error',
          reply == {'ok': False, 'error': 'unknown command: nope'}, reply)

    reply = json.loads(browser.raw(b'this is not json\n'))
    check('malformed json is an invalid request',
          reply['ok'] is False and reply['error'].startswith('invalid request'), reply)

    reply = browser.reply({'args': {}})
    check('a request without cmd is an invalid request',
          reply['ok'] is False and reply['error'].startswith('invalid request'), reply)

    reply = browser.reply({'cmd': 'version', 'args': 5})
    check('args of the wrong type is an invalid request',
          reply['ok'] is False and reply['error'].startswith('invalid request'), reply)

    oversized = b'{"cmd":"version","pad":"' + b'x' * (REQUEST_CAP + 16) + b'"}\n'
    reply = json.loads(browser.raw(oversized))
    check('a request over the cap is refused', reply == {'ok': False, 'error': 'request too large'}, reply)

    started = time.time()
    silent = browser.raw(b'')
    waited = time.time() - started
    check('a silent connection is closed after about 5 s without a reply',
          silent == b'' and 4 <= waited <= 9, f'{waited:.1f}s {silent!r}')


def main():
    for path in (BROWSER, CTL):
        if not path.exists():
            raise SystemExit(f'{path} is missing, build it first')
    with Fixture() as fixture:
        disabled_cases(fixture)
        with Browser(fixture.url) as browser:
            transport_cases(browser)
    if failures:
        print(f'\n{len(failures)} failed: ' + '; '.join(failures))
        sys.exit(1)
    print('\nall passed')


main()
