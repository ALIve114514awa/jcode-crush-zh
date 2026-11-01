// Merge the integration settings without discarding provider or user settings.
import fs from 'node:fs';
import path from 'node:path';
import TOML from '@iarna/toml';

const [configPath, installRoot] = process.argv.slice(2);
if (!configPath || !installRoot) throw new Error('Usage: configure.mjs CONFIG_PATH INSTALL_ROOT');
const original = fs.existsSync(configPath) ? fs.readFileSync(configPath, 'utf8') : '';
const config = TOML.parse(original.replace(/^\uFEFF/, ''));
config.features ??= {};
// An upstream automatic update would replace this custom integration build.
config.features.check_updates = false;
config.lsp ??= {};
config.lsp.enabled = true;
config.lsp.servers ??= {};
config.lsp.servers.typescript ??= {
  command: path.join(installRoot, 'tools', 'node', 'node.exe'),
  args: [path.join(installRoot, 'tools', 'runtime', 'node_modules', 'typescript-language-server', 'lib', 'cli.mjs'), '--stdio'],
  file_types: ['ts', 'tsx', 'js', 'jsx', 'mjs', 'cjs'],
  root_markers: ['tsconfig.json', 'jsconfig.json', 'package.json', '.git'],
  timeout_secs: 30,
  idle_timeout_secs: 120,
  initialization_options: {
    disableAutomaticTypingAcquisition: true,
    maxTsServerMemory: 256,
    tsserver: { useSyntaxServer: 'never', path: path.join(installRoot, 'tools', 'runtime', 'node_modules', 'typescript', 'lib', 'tsserver.js') },
    preferences: { disableSuggestions: true },
  },
};
const result = TOML.stringify(config);
fs.mkdirSync(path.dirname(configPath), { recursive: true });
if (original) fs.copyFileSync(configPath, `${configPath}.before-fusion-${Date.now()}.bak`);
const staged = `${configPath}.${process.pid}.tmp`;
fs.writeFileSync(staged, result, { encoding: 'utf8', mode: 0o600 });
fs.renameSync(staged, configPath);
console.log(`Updated integration settings: ${configPath}`);
