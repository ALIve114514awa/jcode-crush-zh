"""Run the built host Registry against real TypeScript and local MCP peers.

Configuration, sessions, MCP credentials and IPC are isolated from user data.
No provider request or external MCP connection is made.
"""
import argparse
import ctypes
from ctypes import wintypes
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time

from verify_remote_mcp import Peer
from measure_lsp_windows import snapshot, windows_api


def main():
    sys.stdout.reconfigure(encoding='utf-8')
    parser = argparse.ArgumentParser()
    parser.add_argument('binary', type=Path)
    parser.add_argument('runtime', type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    binary, runtime = args.binary.resolve(), args.runtime.resolve()
    peer = Peer()
    threading.Thread(target=peer.serve_forever, daemon=True).start()
    kernel, psapi = windows_api()
    kernel.GetExitCodeProcess.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)]
    samples = []
    try:
        with tempfile.TemporaryDirectory(prefix='jcode-fusion-') as temp:
            root = Path(temp)
            home, project = root / 'home', root / '中文 项目😀'
            home.mkdir()
            project.mkdir()
            # Presence of the empty MCP config prevents one-time Codex imports.
            (home / 'mcp.json').write_text('{"mcpServers":{}}', encoding='utf-8')
            config = '\n'.join([
                '[features]', 'check_updates = false', '[lsp]', 'enabled = true',
                '[lsp.servers.typescript]', 'command = "node"',
                'args = ' + json.dumps([str(runtime / 'node_modules/typescript-language-server/lib/cli.mjs'), '--stdio']),
                'file_types = ["ts"]', 'root_markers = ["tsconfig.json"]',
                'timeout_secs = 45', 'idle_timeout_secs = 120',
                '[lsp.servers.typescript.initialization_options]',
                'disableAutomaticTypingAcquisition = true', 'maxTsServerMemory = 256',
                '[lsp.servers.typescript.initialization_options.tsserver]',
                'useSyntaxServer = "never"', 'path = ' + json.dumps(str(runtime / 'node_modules/typescript/lib/tsserver.js')),
            ])
            (home / 'config.toml').write_text(config, encoding='utf-8')
            (project / 'package.json').write_text('{"type":"module"}', encoding='utf-8')
            (project / 'tsconfig.json').write_text(json.dumps({'compilerOptions': {'strict': True, 'noEmit': True,
                'target': 'ES2022', 'module': 'NodeNext', 'moduleResolution': 'NodeNext', 'types': []}, 'include': ['*.ts']}), encoding='utf-8')
            (project / '.mcp.json').write_text(json.dumps({'mcpServers': {
                'fixture_http': {'type': 'http', 'url': peer.origin + '/mcp', 'headers': {'X-Fixture': 'fixture header value'}, 'timeout_secs': 10},
                'fixture_sse': {'type': 'sse', 'url': peer.origin + '/sse', 'headers': {'X-Fixture': 'fixture header value'}, 'shared': False, 'timeout_secs': 10},
            }}), encoding='utf-8')
            other_project = root / 'another account'
            other_project.mkdir()
            (other_project / '.mcp.json').write_text(json.dumps({'mcpServers': {
                'fixture_http': {'type': 'http', 'url': peer.origin + '/mcp',
                    'headers': {'X-Fixture': 'fixture header value', 'X-Tenant': 'other account'}, 'timeout_secs': 10},
            }}), encoding='utf-8')
            environment = dict(os.environ, JCODE_HOME=str(home), JCODE_DISABLE_CLAUDE_MCP='1',
                JCODE_NO_TELEMETRY='1',
                JCODE_MCP_REMOTE_BRIDGE=str(runtime / 'node_modules/mcp-remote/dist/proxy.js'),
                JCODE_TEST_STDIO_FIXTURE=str(Path(__file__).resolve().parents[1] / 'tests/fixtures/mcp-stdio.mjs'),
                JCODE_PRIVATE_API_KEY='fixture-filter-test',
                JCODE_TEST_OTHER_PROJECT=str(other_project),
                NO_PROXY='localhost,127.0.0.1')
            started = time.monotonic()
            process = subprocess.Popen([str(binary), '--cwd', str(project), '--fusion'], env=environment,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, encoding='utf-8', creationflags=0x08000000)
            stdout, stderr = [], []
            def read_stdout():
                for line in process.stdout:
                    stdout.append(line)
                    try:
                        event = json.loads(line)
                        if 'stage' in event:
                            print(json.dumps({'stage': event['stage']}, ensure_ascii=False), flush=True)
                    except json.JSONDecodeError:
                        pass
            threading.Thread(target=read_stdout, daemon=True).start()
            threading.Thread(target=lambda: stderr.append(process.stderr.read()), daemon=True).start()
            observed = set()
            while process.poll() is None:
                tree = snapshot(kernel, psapi, process.pid)
                observed.update(p['pid'] for p in tree if p['pid'] != process.pid)
                stage = 'starting'
                for line in reversed(stdout):
                    try:
                        event = json.loads(line)
                        if 'stage' in event:
                            stage = event['stage']
                            break
                    except json.JSONDecodeError:
                        pass
                samples.append({'elapsed_seconds': time.monotonic() - started, 'stage': stage, 'processes': tree})
                if time.monotonic() - started > 180:
                    process.kill()
                    process.wait(timeout=5)
                    raise RuntimeError('Full host verification exceeded 180 seconds')
                time.sleep(0.2)
            time.sleep(0.3)
            output = ''.join(stdout)
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.with_name(args.output.stem + '-trace.json').write_text(json.dumps({
                'stdout': output, 'stderr': ''.join(stderr), 'events': peer.events, 'samples': samples,
                'exit_code': process.returncode,
            }, ensure_ascii=False, indent=2), encoding='utf-8')
            if process.returncode != 0:
                args.output.parent.mkdir(parents=True, exist_ok=True)
                args.output.with_name(args.output.stem + '-failed.json').write_text(json.dumps({
                    'passed': False, 'stdout': output, 'stderr': ''.join(stderr), 'events': peer.events,
                    'samples': samples, 'exit_code': process.returncode,
                }, ensure_ascii=False, indent=2), encoding='utf-8')
                raise RuntimeError(f'Host exited {process.returncode}: {output[-4000:]}\n' + ''.join(stderr)[-4000:])
            assert 'fusion_passed' in output, output
            assert sum(event.get('fixture_text') == 'fixture-disconnect' for event in peer.events) == 1, 'The ambiguous disconnected call was replayed'
            delayed_calls = {(event['session'], event['id']) for event in peer.events if event.get('fixture_text') == 'fixture-delay'}
            assert any(event['method'] == 'notifications/cancelled' and (event['session'], event['cancelled_request']) in delayed_calls
                for event in peer.events), 'MCP cancellation was not forwarded for an active tool call'
            # Give stdio shutdown and the Windows LSP job enough time to finish.
            time.sleep(2)
            alive = []
            for pid in observed:
                handle = kernel.OpenProcess(0x1000, False, pid)
                if handle:
                    code = wintypes.DWORD()
                    active = kernel.GetExitCodeProcess(handle, ctypes.byref(code)) and code.value == 259
                    kernel.CloseHandle(handle)
                    if active:
                        alive.append(pid)
            assert not alive, f'Child processes survived the host: {alive}'
            report = {'passed': True, 'binary': str(binary), 'sha256': hashlib.file_digest(binary.open('rb'), 'sha256').hexdigest(),
                'elapsed_seconds': time.monotonic() - started, 'stdout': output, 'stderr': ''.join(stderr),
                'events': peer.events, 'samples': samples, 'surviving_children': alive,
                'peak_working_set_sum_bytes': max(sum(p['working_set_bytes'] for p in sample['processes']) for sample in samples),
                'peak_private_bytes_sum': max(sum(p['private_bytes'] for p in sample['processes']) for sample in samples)}
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
            print(json.dumps({key: report[key] for key in ('passed', 'binary', 'elapsed_seconds', 'peak_working_set_sum_bytes', 'peak_private_bytes_sum', 'surviving_children')}, ensure_ascii=False))
    finally:
        peer.shutdown()
        peer.server_close()


if __name__ == '__main__':
    main()
