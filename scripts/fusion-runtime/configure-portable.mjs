// Rebase only integration-owned paths; never replace provider settings or tokens.
import fs from 'node:fs';
import path from 'node:path';
import TOML from '@iarna/toml';
import { fileURLToPath } from 'node:url';

function publish(file, text) {
  if (fs.existsSync(file) && fs.readFileSync(file, 'utf8') === text) return;
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const staged = `${file}.${process.pid}.tmp`;
  fs.writeFileSync(staged, text, { mode: 0o600 });
  fs.renameSync(staged, file);
}

export function configurePortable(root, { migrate = false } = {}) {
  root = path.resolve(root);
  const home = path.join(root, 'data');
  const file = path.join(home, 'config.toml');
  const config = TOML.parse(fs.existsSync(file) ? fs.readFileSync(file, 'utf8').replace(/^\uFEFF/, '') : '');
  config.features ??= {};
  config.features.check_updates = false;
  config.lsp ??= {};
  config.lsp.enabled ??= true;
  config.lsp.servers ??= {};
  let server = config.lsp.servers.typescript;
  if (!server) {
    server = config.lsp.servers.typescript = {
      file_types: ['ts', 'tsx', 'js', 'jsx', 'mjs', 'cjs'],
      root_markers: ['tsconfig.json', 'jsconfig.json', 'package.json', '.git'],
      timeout_secs: 30, idle_timeout_secs: 120,
      initialization_options: { disableAutomaticTypingAcquisition: true, maxTsServerMemory: 256,
        tsserver: { useSyntaxServer: 'never' }, preferences: { disableSuggestions: true } },
    };
  }
  const managed = !server.command || (server.args ?? []).some(arg =>
    typeof arg === 'string' && /[\\/]tools[\\/]runtime[\\/]node_modules[\\/]typescript-language-server[\\/]lib[\\/]cli\.mjs$/i.test(arg));
  if (managed) {
    server.command = path.join(root, 'tools', 'node', 'node.exe');
    server.args = [path.join(root, 'tools', 'runtime', 'node_modules', 'typescript-language-server', 'lib', 'cli.mjs'), '--stdio'];
    server.initialization_options ??= {};
    server.initialization_options.tsserver ??= {};
    server.initialization_options.tsserver.path = path.join(root, 'tools', 'runtime', 'node_modules', 'typescript', 'lib', 'tsserver.js');
  }
  publish(file, TOML.stringify(config));

  const removed = [];
  if (migrate) {
    const mcpFile = path.join(home, 'mcp.json');
    const mcp = fs.existsSync(mcpFile) ? JSON.parse(fs.readFileSync(mcpFile, 'utf8').replace(/^\uFEFF/, '')) : { mcpServers: {} };
    const servers = mcp.mcpServers ?? mcp.servers ?? {};
    for (const [name, entry] of Object.entries(servers)) {
      if (typeof entry.command === 'string' && /[\\/]Codex[\\/]runtimes[\\/]/i.test(entry.command)) {
        delete servers[name];
        removed.push({ name, reason: '本机 Codex 专用运行时，便携版停用；原电脑配置保留。' });
      }
    }
    publish(mcpFile, JSON.stringify(mcp, null, 2) + '\n');
  }
  return { removed_mcp_servers: removed, bundled_typescript: managed };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [root, mode] = process.argv.slice(2);
  if (!root) throw new Error('Usage: configure-portable.mjs ROOT [--migrate]');
  const result = configurePortable(root, { migrate: mode === '--migrate' });
  // Windows PowerShell 5.1 can decode native stdout with a legacy code page.
  if (mode === '--migrate') console.log(JSON.stringify(result).replace(/[\u007f-\uffff]/g,
    char => `\\u${char.charCodeAt(0).toString(16).padStart(4, '0')}`));
}
