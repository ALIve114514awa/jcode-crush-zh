import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import TOML from '@iarna/toml';
import { configurePortable } from './configure-portable.mjs';

test('migration preserves user data and rebases bundled paths after relocation', t => {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'jcode-portable-path-'));
  t.after(() => fs.rmSync(temp, { recursive: true, force: true }));
  const root = path.join(temp, '原始 中文 folder');
  const moved = path.join(temp, '移动后的 folder');
  fs.mkdirSync(path.join(root, 'data'), { recursive: true });
  fs.writeFileSync(path.join(root, 'data', 'config.toml'), TOML.stringify({
    features: { check_updates: true }, provider: { default_model: 'user-model' },
    lsp: { enabled: false, servers: { typescript: {
      command: 'C:\\old\\tools\\node\\node.exe',
      args: ['C:\\old\\tools\\runtime\\node_modules\\typescript-language-server\\lib\\cli.mjs', '--stdio'],
      timeout_secs: 17,
    } } },
  }));
  const remote = { transport: 'http', url: 'https://example.invalid/mcp', headers: { Authorization: 'fixture-only' } };
  fs.writeFileSync(path.join(root, 'data', 'mcp.json'), JSON.stringify({ mcpServers: {
    node_repl: { command: 'C:\\Users\\fixture\\OpenAI\\Codex\\runtimes\\cua_node\\bin\\node_repl.exe' },
    remote, custom: { command: 'custom-tool' },
  } }));
  const result = configurePortable(root, { migrate: true });
  assert.equal(result.removed_mcp_servers[0].name, 'node_repl');
  const mcp = JSON.parse(fs.readFileSync(path.join(root, 'data', 'mcp.json'), 'utf8'));
  assert.deepEqual(mcp.mcpServers.remote, remote);
  assert.equal(mcp.mcpServers.custom.command, 'custom-tool');
  fs.cpSync(root, moved, { recursive: true });
  configurePortable(moved);
  const file = path.join(moved, 'data', 'config.toml');
  const text = fs.readFileSync(file, 'utf8');
  const config = TOML.parse(text);
  assert.equal(config.provider.default_model, 'user-model');
  assert.equal(config.features.check_updates, false);
  assert.equal(config.lsp.enabled, false);
  assert.equal(config.lsp.servers.typescript.timeout_secs, 17);
  assert.equal(config.lsp.servers.typescript.command, path.join(moved, 'tools', 'node', 'node.exe'));
  assert.ok(config.lsp.servers.typescript.args[0].startsWith(moved));
  assert.ok(config.lsp.servers.typescript.initialization_options.tsserver.path.startsWith(moved));
  assert.ok(!text.includes(root.replaceAll('\\', '\\\\')));
  configurePortable(moved);
  assert.equal(fs.readFileSync(file, 'utf8'), text);
});

test('custom language server remains user managed', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'jcode-portable-custom-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.mkdirSync(path.join(root, 'data'));
  const server = { command: 'custom-ts-server', args: ['--custom'] };
  fs.writeFileSync(path.join(root, 'data', 'config.toml'), TOML.stringify({ lsp: { servers: { typescript: server } } }));
  configurePortable(root);
  const config = TOML.parse(fs.readFileSync(path.join(root, 'data', 'config.toml'), 'utf8'));
  assert.deepEqual(config.lsp.servers.typescript, server);
});

test('malformed configuration is preserved for recovery', t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'jcode-portable-malformed-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.mkdirSync(path.join(root, 'data'));
  const file = path.join(root, 'data', 'config.toml');
  fs.writeFileSync(file, 'invalid = [');
  assert.throws(() => configurePortable(root));
  assert.equal(fs.readFileSync(file, 'utf8'), 'invalid = [');
});
