"""Exercise the pinned MCP adapter against local HTTP, SSE and OAuth peers.

No external service, user token or real browser is used. Full-host Registry
validation is performed separately by the Rust fusion harness.
"""
import argparse
import base64
from concurrent.futures import ThreadPoolExecutor
import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import queue
import socket
import subprocess
import sys
import tempfile
import threading
import time
import urllib.parse
import uuid


class Peer(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self):
        super().__init__(('127.0.0.1', 0), Handler)
        self.origin = f'http://127.0.0.1:{self.server_port}'
        self.streams = {}
        self.sessions = set()
        self.events = []
        self.pending_cancellations = {}
        self.codes = {}
        self.oauth = {'registration': 0, 'authorization': 0, 'code_exchange': 0, 'refresh': 0}
        self.mutex = threading.Lock()


class Handler(BaseHTTPRequestHandler):
    protocol_version = 'HTTP/1.1'

    def log_message(self, *_):
        pass

    def handle(self):
        try:
            super().handle()
        except (ConnectionResetError, ConnectionAbortedError, BrokenPipeError):
            pass

    def output(self, status, payload=None, headers=None):
        body = b'' if payload is None else json.dumps(payload, ensure_ascii=False).encode()
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(body)))
        for name, value in (headers or {}).items():
            self.send_header(name, value)
        self.end_headers()
        self.wfile.write(body)

    def body(self):
        return self.rfile.read(int(self.headers.get('Content-Length', '0')))

    def unauthorized(self):
        self.output(401, headers={'WWW-Authenticate': f'Bearer resource_metadata="{self.server.origin}/metadata", scope="tools:read"'})

    def do_GET(self):
        parsed = urllib.parse.urlsplit(self.path)
        if parsed.path == '/metadata' or parsed.path.startswith('/.well-known/oauth-protected-resource/oauth'):
            return self.output(200, {'resource': self.server.origin + '/oauth', 'authorization_servers': [self.server.origin], 'scopes_supported': ['tools:read']})
        if parsed.path in ('/.well-known/oauth-authorization-server', '/.well-known/openid-configuration'):
            return self.output(200, {'issuer': self.server.origin, 'authorization_endpoint': self.server.origin + '/authorize',
                'token_endpoint': self.server.origin + '/token', 'registration_endpoint': self.server.origin + '/register',
                'response_types_supported': ['code'], 'grant_types_supported': ['authorization_code', 'refresh_token'],
                'code_challenge_methods_supported': ['S256'], 'token_endpoint_auth_methods_supported': ['none']})
        if parsed.path == '/authorize':
            params = {k: v[0] for k, v in urllib.parse.parse_qs(parsed.query).items()}
            assert params['code_challenge_method'] == 'S256'
            assert params['state'] and params['client_id'] == 'fixture-client'
            code = uuid.uuid4().hex
            self.server.codes[code] = params
            self.server.oauth['authorization'] += 1
            callback = params['redirect_uri'] + '?' + urllib.parse.urlencode({'code': code, 'state': params['state'], 'iss': self.server.origin})
            return self.output(302, headers={'Location': callback})
        if parsed.path == '/oauth' and not self.headers.get('Authorization', '').startswith('Bearer fixture-'):
            return self.unauthorized()
        if parsed.path in ('/mcp', '/mcp-stream', '/oauth'):
            return self.output(405)
        if parsed.path == '/sse':
            if self.headers.get('X-Fixture') != 'fixture header value':
                return self.output(403, {'error': 'missing configured SSE header'})
            identifier = uuid.uuid4().hex
            messages = queue.Queue()
            self.server.streams[identifier] = messages
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.send_header('Cache-Control', 'no-cache')
            self.end_headers()
            try:
                self.wfile.write(f'event: endpoint\ndata: /messages?session={identifier}\n\n'.encode())
                self.wfile.flush()
                while True:
                    try:
                        message = messages.get(timeout=0.5)
                        self.wfile.write(('event: message\ndata: ' + json.dumps(message, ensure_ascii=False) + '\n\n').encode())
                    except queue.Empty:
                        self.wfile.write(b': keep-alive\n\n')
                    self.wfile.flush()
            except (OSError, BrokenPipeError):
                pass
            finally:
                self.server.streams.pop(identifier, None)
            return
        self.output(404)

    def do_DELETE(self):
        self.server.sessions.discard(self.headers.get('Mcp-Session-Id'))
        self.output(204)

    def do_POST(self):
        parsed = urllib.parse.urlsplit(self.path)
        if parsed.path == '/register':
            info = json.loads(self.body())
            self.server.oauth['registration'] += 1
            return self.output(201, {**info, 'client_id': 'fixture-client', 'token_endpoint_auth_method': 'none'})
        if parsed.path == '/token':
            params = {k: v[0] for k, v in urllib.parse.parse_qs(self.body().decode()).items()}
            if params['grant_type'] == 'authorization_code':
                original = self.server.codes.pop(params['code'])
                challenge = base64.urlsafe_b64encode(hashlib.sha256(params['code_verifier'].encode()).digest()).decode().rstrip('=')
                assert challenge == original['code_challenge'], 'PKCE verifier mismatch'
                assert params['redirect_uri'] == original['redirect_uri']
                self.server.oauth['code_exchange'] += 1
            elif params['grant_type'] == 'refresh_token':
                assert params['refresh_token'] == 'fixture-refresh'
                self.server.oauth['refresh'] += 1
            else:
                return self.output(400, {'error': 'unsupported_grant_type'})
            return self.output(200, {'access_token': 'fixture-' + uuid.uuid4().hex, 'token_type': 'Bearer',
                'refresh_token': 'fixture-refresh', 'expires_in': 2, 'scope': 'tools:read'})
        request = json.loads(self.body())
        if parsed.path == '/oauth' and not self.headers.get('Authorization', '').startswith('Bearer fixture-'):
            return self.unauthorized()
        if parsed.path in ('/mcp', '/mcp-stream', '/sse', '/messages') and self.headers.get('X-Fixture') != 'fixture header value':
            return self.output(403, {'error': 'missing configured header'})
        if parsed.path not in ('/mcp', '/mcp-stream', '/oauth', '/messages'):
            return self.output(404)
        session = self.headers.get('Mcp-Session-Id')
        if parsed.path == '/messages':
            session = urllib.parse.parse_qs(parsed.query)['session'][0]
        if request['method'] == 'initialize':
            session = session or uuid.uuid4().hex
            self.server.sessions.add(session)
        elif parsed.path != '/messages' and session not in self.server.sessions:
            return self.output(404)
        self.server.events.append({'method': request['method'], 'session': session, 'id': request.get('id'),
            'cancelled_request': request.get('params', {}).get('requestId'),
            'fixture_text': request.get('params', {}).get('arguments', {}).get('text')})
        if request['method'] == 'notifications/cancelled':
            pending = self.server.pending_cancellations.get(session)
            if pending and pending[0] == request['params']['requestId']:
                pending[1].set()
        if 'id' not in request:
            return self.output(202)
        method = request['method']
        if method == 'initialize':
            result = {'protocolVersion': request['params']['protocolVersion'], 'capabilities': {'tools': {'listChanged': False}},
                      'serverInfo': {'name': 'jcode-fixture', 'version': '1.0.0'}}
        elif method == 'tools/list':
            result = {'tools': [{'name': 'echo', 'description': 'Echo fixture', 'inputSchema': {'type': 'object',
                'properties': {'text': {'type': 'string'}}, 'required': ['text']}}]}
        elif method == 'tools/call':
            text = request['params']['arguments']['text']
            if text == 'fixture-delay':
                cancellation = threading.Event()
                self.server.pending_cancellations[session] = (request['id'], cancellation)
                cancellation.wait(3)
            if text == 'after cancellation':
                pending = self.server.pending_cancellations.get(session)
                if not pending or not pending[1].wait(3):
                    return self.output(200, {'jsonrpc': '2.0', 'id': request['id'], 'result': {
                        'content': [{'type': 'text', 'text': 'missing cancellation notification'}], 'isError': True}}, {'Mcp-Session-Id': session})
            if text == 'fixture-disconnect':
                self.close_connection = True
                try:
                    self.connection.shutdown(socket.SHUT_RDWR)
                except OSError:
                    pass
                return
            result = {'content': [{'type': 'text', 'text': session if text == 'fixture-session' else text}], 'isError': False}
        elif method == 'ping':
            result = {}
        else:
            return self.output(400, {'error': 'unknown fixture method'})
        response = {'jsonrpc': '2.0', 'id': request['id'], 'result': result}
        if parsed.path == '/messages':
            self.server.streams[session].put(response)
            return self.output(202)
        if parsed.path == '/mcp-stream':
            payload = ('event: message\ndata: ' + json.dumps(response, ensure_ascii=False) + '\n\n').encode()
            self.send_response(200)
            self.send_header('Content-Type', 'text/event-stream')
            self.send_header('Mcp-Session-Id', session)
            self.send_header('Content-Length', str(len(payload)))
            self.end_headers()
            self.wfile.write(payload)
            return
        self.output(200, response, {'Mcp-Session-Id': session})


def run_adapter(peer, proxy, mode, directory):
    oauth = mode == 'oauth'
    transport = 'sse-only' if mode == 'sse' else 'http-only'
    endpoint = peer.origin + '/' + mode
    env = dict(os.environ, MCP_REMOTE_CONFIG_DIR=str(directory / 'auth'), NO_PROXY='localhost,127.0.0.1',
               JCODE_TEST_HEADER='fixture header value')
    args = ['node', str(proxy), endpoint, '--allow-http', '--transport', transport, '--auth-timeout', '20',
            '--header', 'X-Fixture:${JCODE_TEST_HEADER}']
    if oauth:
        args = ['node', str(Path(__file__).resolve().parents[1] / 'tests/fixtures/mcp-oauth-browser.mjs'), *args[1:]]
    process = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        text=True, encoding='utf-8', env=env, creationflags=0x08000000 if os.name == 'nt' else 0)
    responses, stderr = queue.Queue(), []
    threading.Thread(target=lambda: [responses.put(json.loads(line)) for line in process.stdout if line.strip()], daemon=True).start()
    threading.Thread(target=lambda: stderr.append(process.stderr.read()), daemon=True).start()
    identifier = 0

    def request(method, params=None):
        nonlocal identifier
        identifier += 1
        msg = {'jsonrpc': '2.0', 'id': identifier, 'method': method}
        if params is not None:
            msg['params'] = params
        process.stdin.write(json.dumps(msg, ensure_ascii=False) + '\n')
        process.stdin.flush()
        deadline = time.monotonic() + 35
        while time.monotonic() < deadline:
            try:
                response = responses.get(timeout=0.2)
            except queue.Empty:
                if process.poll() is not None:
                    raise RuntimeError('Adapter exited: ' + ''.join(stderr)[-2000:])
                continue
            if response.get('id') == identifier:
                assert 'error' not in response, response
                return response['result']
        raise RuntimeError('Adapter request timed out: ' + ''.join(stderr)[-2000:])

    try:
        info = request('initialize', {'protocolVersion': '2025-11-25', 'capabilities': {},
            'clientInfo': {'name': 'jcode-fixture', 'version': '1.0.0'}})
        assert info['serverInfo']['name'] == 'jcode-fixture'
        process.stdin.write('{"jsonrpc":"2.0","method":"notifications/initialized"}\n')
        process.stdin.flush()
        tools = request('tools/list')['tools']
        assert tools[0]['name'] == 'echo'
        echo = request('tools/call', {'name': 'echo', 'arguments': {'text': '中文 😀 cross transport'}})
        assert echo['content'][0]['text'] == '中文 😀 cross transport'
        if oauth:
            time.sleep(3)
            request('tools/call', {'name': 'echo', 'arguments': {'text': 'after refresh'}})
            assert peer.oauth['code_exchange'] > 0
            assert peer.oauth['refresh'] > 0
        return {'mode': mode, 'pid': process.pid, 'tools': len(tools), 'passed': True}
    finally:
        process.stdin.close()
        try:
            process.wait(timeout=2)
        except subprocess.TimeoutExpired:
            process.terminate()
            process.wait(timeout=3)


def main():
    sys.stdout.reconfigure(encoding='utf-8')
    parser = argparse.ArgumentParser()
    parser.add_argument('proxy', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    peer = Peer()
    threading.Thread(target=peer.serve_forever, daemon=True).start()
    results = []
    try:
        with tempfile.TemporaryDirectory(prefix='jcode-mcp-') as temp:
            for mode in ('mcp', 'mcp-stream', 'sse', 'oauth'):
                result = run_adapter(peer, args.proxy.resolve(), mode, Path(temp) / mode)
                results.append(result)
                print(json.dumps(result), flush=True)
        report = {'results': results, 'oauth': peer.oauth, 'events': peer.events}
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2), encoding='utf-8')
        print(json.dumps({'passed': len(results), 'oauth': peer.oauth}), flush=True)
    finally:
        peer.shutdown()
        peer.server_close()


if __name__ == '__main__':
    main()
