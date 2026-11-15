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


def tabs(browser):
    result = browser.ctl('tab', 'list')
    rows = []
    for line in text(result).splitlines():
        identifier, window, active, url, title = line.split('\t')
        rows.append({'id': identifier, 'window': window, 'active': active == '*', 'url': url, 'title': title})
    return rows


def frontmost():
    return subprocess.run(['lsappinfo', 'front'], capture_output=True, text=True).stdout.strip()


def tab_cases(browser, fixture):
    listed = tabs(browser)
    check('tab list shows the fixture tab as the active one',
          len(listed) == 1 and listed[0]['active'] and listed[0]['url'] == fixture.url
          and listed[0]['title'] == 'Gidora fixture', listed)
    first = listed[0]['id']

    front = frontmost()
    result = browser.ctl('tab', 'new', fixture.url + '?n=1')
    opened = text(result).strip()
    check('tab new prints the id of the new tab', result.returncode == 0 and opened.isdigit(),
          text(result) + error(result))
    listed = tabs(browser)
    by_id = {row['id']: row for row in listed}
    check('the new tab is listed with its url', opened in by_id and by_id[opened]['url'] == fixture.url + '?n=1',
          listed)
    check('the new tab did not take the active state',
          by_id.get(first, {}).get('active') is True and by_id.get(opened, {}).get('active') is False, listed)
    check('tab new did not change the frontmost application', frontmost() == front, f'{front} -> {frontmost()}')

    reply = browser.reply({'cmd': 'tab.new', 'args': {'url': fixture.url + '?n=json'}})
    check('tab.new replies after the load with id, url and loading',
          reply['ok'] and reply['result']['loading'] is False
          and reply['result']['url'] == fixture.url + '?n=json' and isinstance(reply['result']['id'], int), reply)
    second = str(reply['result']['id'])

    result = browser.ctl('tab', 'go', '--tab', opened, fixture.url + '?n=2')
    check('tab go prints the committed url',
          result.returncode == 0 and text(result) == fixture.url + '?n=2\n', text(result) + error(result))

    for identifier in (opened, second):
        result = browser.ctl('tab', 'close', '--tab', identifier)
        check(f'tab close succeeds quietly for tab {identifier}',
              result.returncode == 0 and text(result) == '', text(result) + error(result))
    listed = tabs(browser)
    check('closed tabs leave the list and the first tab stays active',
          [row['id'] for row in listed] == [first] and listed[0]['active'], listed)

    for arguments in (['tab', 'go', '--tab', '999999', fixture.url], ['tab', 'close', '--tab', '999999']):
        result = browser.ctl(*arguments)
        check(f'{arguments[1]} on an unknown tab id fails',
              result.returncode == 1 and 'unknown tab: 999999' in error(result), error(result))

    for url in ('chrome://settings', 'file:///etc/hosts', 'javascript:alert(1)', 'not a url'):
        result = browser.ctl('tab', 'new', url)
        check(f'tab new refuses {url}',
              result.returncode == 1 and 'unsupported URL: only http and https are allowed' in error(result),
              error(result))
    result = browser.ctl('tab', 'go', 'chrome://version')
    check('tab go refuses an internal url',
          result.returncode == 1 and 'unsupported URL' in error(result), error(result))
    check('refused urls opened no tab', len(tabs(browser)) == 1)


def incognito_cases(fixture):
    with Browser(fixture.url, extra=['--incognito']) as browser:
        result = browser.ctl('tab', 'list')
        check('an incognito window is not listed', result.returncode == 0 and text(result) == '', text(result))
        result = browser.ctl('tab', 'close')
        check('an incognito window is not a target',
              result.returncode == 1 and 'no browser window' in error(result), error(result))
        result = browser.ctl('tab', 'new', fixture.url)
        check('tab new does not open into an incognito window',
              result.returncode == 1 and 'no browser window' in error(result), error(result))


def main():
    for path in (BROWSER, CTL):
        if not path.exists():
            raise SystemExit(f'{path} is missing, build it first')
    with Fixture() as fixture:
        disabled_cases(fixture)
        incognito_cases(fixture)
        with Browser(fixture.url) as browser:
            transport_cases(browser)
            tab_cases(browser, fixture)
    if failures:
        print(f'\n{len(failures)} failed: ' + '; '.join(failures))
        sys.exit(1)
    print('\nall passed')


main()
