"""Stage a byte-identical dependency closure for focused LSP validation.

The upstream workspace resolves unrelated Git dependencies even for `-p
jcode-lsp`. This fallback stages only local dependencies and verifies SHA256
for every copied source. Run from any directory; build artifacts belong in
the main workspace's target/lsp-validation, outside this disposable copy.
"""
from pathlib import Path
import hashlib
import json
import shutil
import tomllib

source = Path(__file__).resolve().parents[1]
destination = source / '.tmp' / 'lsp-validation'
resolved = destination.resolve()
if not resolved.is_relative_to(source.resolve() / '.tmp') or resolved == source.resolve():
    raise RuntimeError('Validation path escapes the intended scratch directory')
lock_path = destination / 'Cargo.lock'
previous_lock = lock_path.read_bytes() if lock_path.is_file() else None
if destination.exists():
    shutil.rmtree(destination)
destination.mkdir(parents=True)

pending = [source / 'crates' / 'jcode-lsp', source / 'crates' / 'jcode-mcp-bridge']
packages = set()
while pending:
    package = pending.pop().resolve()
    if package in packages:
        continue
    if not package.is_relative_to(source.resolve() / 'crates'):
        raise RuntimeError('Unexpected dependency outside the source crates directory')
    packages.add(package)
    manifest = tomllib.loads((package / 'Cargo.toml').read_text(encoding='utf-8'))
    tables = [manifest, *manifest.get('target', {}).values()]
    for table in tables:
        for section in ('dependencies', 'dev-dependencies', 'build-dependencies'):
            for dependency in table.get(section, {}).values():
                if isinstance(dependency, dict) and 'path' in dependency:
                    pending.append(package / dependency['path'])

hashes = {}
for package in sorted(packages):
    relative = package.relative_to(source.resolve())
    copied = destination / relative
    shutil.copytree(package, copied, ignore=shutil.ignore_patterns('target', '.git', '__pycache__'))
    for file in copied.rglob('*'):
        if file.is_file():
            relative_file = file.relative_to(destination)
            original = source / relative_file
            digest = hashlib.sha256(file.read_bytes()).hexdigest()
            if digest != hashlib.sha256(original.read_bytes()).hexdigest():
                raise RuntimeError('Source copy verification failed')
            hashes[str(relative_file).replace('\\', '/')] = digest
members = [str(p.relative_to(source.resolve())).replace('\\', '/') for p in sorted(packages)]
(destination / 'Cargo.toml').write_text(
    '[workspace]\nresolver = "3"\nmembers = ' + json.dumps(members)
    + '\n\n[profile.dev]\ndebug = 0\n\n[profile.test]\ndebug = 0\n', encoding='utf-8')
(destination / 'source-hashes.json').write_text(json.dumps(hashes, indent=2), encoding='utf-8')
if previous_lock is not None:
    lock_path.write_bytes(previous_lock)
print(f'Verified {len(hashes)} identical source files across {len(packages)} crates')
print(destination)
