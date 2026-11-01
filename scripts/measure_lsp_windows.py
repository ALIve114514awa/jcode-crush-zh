"""Run the standalone TypeScript smoke test and sample its entire process tree.

Uses Windows process APIs, not a model provider, daemon or third party library.
"""
import argparse
import atexit
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import threading
import time


class ProcessEntry(ctypes.Structure):
    _fields_ = [
        ('dwSize', wintypes.DWORD), ('cntUsage', wintypes.DWORD),
        ('th32ProcessID', wintypes.DWORD), ('th32DefaultHeapID', ctypes.c_size_t),
        ('th32ModuleID', wintypes.DWORD), ('cntThreads', wintypes.DWORD),
        ('th32ParentProcessID', wintypes.DWORD), ('pcPriClassBase', wintypes.LONG),
        ('dwFlags', wintypes.DWORD), ('szExeFile', wintypes.WCHAR * 260),
    ]


class MemoryCounters(ctypes.Structure):
    _fields_ = [('cb', wintypes.DWORD), ('PageFaultCount', wintypes.DWORD)] + [
        (name, ctypes.c_size_t) for name in (
            'PeakWorkingSetSize', 'WorkingSetSize', 'QuotaPeakPagedPoolUsage',
            'QuotaPagedPoolUsage', 'QuotaPeakNonPagedPoolUsage',
            'QuotaNonPagedPoolUsage', 'PagefileUsage', 'PeakPagefileUsage',
            'PrivateUsage',
        )
    ]


def windows_api():
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    psapi = ctypes.WinDLL('psapi', use_last_error=True)
    kernel.CreateToolhelp32Snapshot.argtypes = [wintypes.DWORD, wintypes.DWORD]
    kernel.CreateToolhelp32Snapshot.restype = wintypes.HANDLE
    for name in ('Process32FirstW', 'Process32NextW'):
        method = getattr(kernel, name)
        method.argtypes = [wintypes.HANDLE, ctypes.POINTER(ProcessEntry)]
        method.restype = wintypes.BOOL
    kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
    kernel.OpenProcess.restype = wintypes.HANDLE
    kernel.CloseHandle.argtypes = [wintypes.HANDLE]
    kernel.GetProcessTimes.argtypes = [wintypes.HANDLE] + [ctypes.POINTER(wintypes.FILETIME)] * 4
    psapi.GetProcessMemoryInfo.argtypes = [wintypes.HANDLE, ctypes.POINTER(MemoryCounters), wintypes.DWORD]
    return kernel, psapi


def snapshot(kernel, psapi, root_pid):
    handle = kernel.CreateToolhelp32Snapshot(2, 0)  # TH32CS_SNAPPROCESS
    if handle == ctypes.c_void_p(-1).value:
        raise ctypes.WinError(ctypes.get_last_error())
    entry = ProcessEntry()
    entry.dwSize = ctypes.sizeof(entry)
    processes = {}
    try:
        valid = kernel.Process32FirstW(handle, ctypes.byref(entry))
        while valid:
            processes[entry.th32ProcessID] = (entry.th32ParentProcessID, entry.szExeFile)
            valid = kernel.Process32NextW(handle, ctypes.byref(entry))
    finally:
        kernel.CloseHandle(handle)
    tree = {root_pid}
    while True:
        children = {pid for pid, (parent, _) in processes.items() if parent in tree}
        if children.issubset(tree):
            break
        tree.update(children)
    result = []
    for pid in sorted(tree & processes.keys()):
        handle = kernel.OpenProcess(0x0400 | 0x0010, False, pid)
        if not handle:
            continue
        try:
            memory = MemoryCounters()
            memory.cb = ctypes.sizeof(memory)
            if not psapi.GetProcessMemoryInfo(handle, ctypes.byref(memory), memory.cb):
                continue
            times = [wintypes.FILETIME() for _ in range(4)]
            cpu = None
            if kernel.GetProcessTimes(handle, *(ctypes.byref(t) for t in times)):
                cpu = sum((t.dwHighDateTime << 32) | t.dwLowDateTime for t in times[2:]) / 10_000_000
            result.append({'pid': pid, 'parent_pid': processes[pid][0], 'name': processes[pid][1],
                           'working_set_bytes': memory.WorkingSetSize, 'private_bytes': memory.PrivateUsage,
                           'cpu_seconds': cpu})
        finally:
            kernel.CloseHandle(handle)
    return result


def main():
    if os.name != 'nt':
        raise RuntimeError('This process-tree sampler requires Windows')
    sys.stdout.reconfigure(encoding='utf-8')
    parser = argparse.ArgumentParser()
    parser.add_argument('binary', type=Path)
    parser.add_argument('language_server', type=Path)
    parser.add_argument('tsserver', type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    kernel, psapi = windows_api()
    environment = dict(os.environ, JCODE_LSP_MEASURE='1')
    process = subprocess.Popen([str(args.binary.resolve()), str(args.language_server.resolve()), str(args.tsserver.resolve())],
                               stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               text=True, encoding='utf-8', env=environment, creationflags=0x08000000)

    def cleanup():
        if process.poll() is None:
            process.stdin.close()
            try:
                process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=2)

    atexit.register(cleanup)
    messages = queue.Queue()
    errors = []

    def reader():
        for line in process.stdout:
            messages.put(json.loads(line))

    def read_errors():
        errors.append(process.stderr.read())

    threading.Thread(target=reader, daemon=True).start()
    threading.Thread(target=read_errors, daemon=True).start()
    started = time.monotonic()
    gate = None
    phase = 'starting'
    events, samples, observed_pids = [], [], set()
    while process.poll() is None:
        now = time.monotonic()
        if now - started > 90:
            process.stdin.close()  # EOF releases a waiting measurement gate.
            process.terminate()
            raise RuntimeError('Smoke test exceeded 90 seconds')
        while not messages.empty():
            event = messages.get_nowait()
            event['elapsed_seconds'] = round(now - started, 3)
            events.append(event)
            print(json.dumps(event, ensure_ascii=False), flush=True)
            phase = event['stage']
            if phase in ('baseline', 'active'):
                gate = now + 1.0
        processes = snapshot(kernel, psapi, process.pid)
        observed_pids.update(p['pid'] for p in processes)
        samples.append({'elapsed_seconds': round(now - started, 3), 'stage': phase, 'processes': processes,
                        'working_set_bytes': sum(p['working_set_bytes'] for p in processes),
                        'private_bytes': sum(p['private_bytes'] for p in processes)})
        if gate is not None and now >= gate:
            process.stdin.write('continue\n')
            process.stdin.flush()
            gate = None
            if phase == 'baseline':
                phase = 'queries'
        time.sleep(0.2)
    process.wait()
    while not messages.empty():
        events.append(messages.get_nowait())
    remaining = []
    for _ in range(10):
        remaining = snapshot(kernel, psapi, process.pid)
        if not remaining:
            break
        time.sleep(0.1)
    # Dead parents disappear from Toolhelp snapshots, so also check observed
    # descendants individually rather than relying on ancestry after shutdown.
    surviving_descendants = []
    for pid in observed_pids - {process.pid}:
        handle = kernel.OpenProcess(0x1000, False, pid)
        if handle:
            code = wintypes.DWORD()
            kernel.GetExitCodeProcess.argtypes = [wintypes.HANDLE, ctypes.POINTER(wintypes.DWORD)]
            if kernel.GetExitCodeProcess(handle, ctypes.byref(code)) and code.value == 259:
                surviving_descendants.append(pid)
            kernel.CloseHandle(handle)
    last_cpu = {}
    for sample in samples:
        for child in sample['processes']:
            if child['cpu_seconds'] is not None:
                last_cpu[child['pid']] = max(last_cpu.get(child['pid'], 0), child['cpu_seconds'])
    report = {'binary': str(args.binary.resolve()), 'exit_code': process.returncode,
              'elapsed_seconds': round(time.monotonic() - started, 3), 'sample_interval_seconds': 0.2,
              'logical_cpu_count': os.cpu_count(), 'observed_pids': sorted(observed_pids),
              'peak_working_set_bytes': max((s['working_set_bytes'] for s in samples), default=0),
              'peak_private_bytes': max((s['private_bytes'] for s in samples), default=0),
              'maximum_process_count': max((len(s['processes']) for s in samples), default=0),
              'sampled_cpu_seconds': sum(last_cpu.values()),
              'surviving_descendants': surviving_descendants, 'events': events, 'samples': samples,
              'stderr': ''.join(errors)}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps({k: v for k, v in report.items() if k not in ('events', 'samples')}, ensure_ascii=False))
    if process.returncode or surviving_descendants:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
