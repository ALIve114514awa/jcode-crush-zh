// The OAuth tests replace only the human browser action. The pinned adapter,
// metadata discovery, registration, state, PKCE, token exchange and refresh run.
import { readdir } from 'node:fs/promises';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const [proxy, ...args] = process.argv.slice(2);
for (const filename of await readdir(path.dirname(proxy))) {
  if (!filename.startsWith('chunk-') || !filename.endsWith('.js')) continue;
  const module = await import(pathToFileURL(path.join(path.dirname(proxy), filename)).href);
  if (!module.NodeOAuthClientProvider) continue;
  module.NodeOAuthClientProvider.prototype.redirectToAuthorization = async function (url) {
    if (!['127.0.0.1', 'localhost'].includes(url.hostname)) throw new Error('Fixture browser may access only loopback');
    // The callback's response can wait for the token exchange. A real browser
    // also runs independently from the client waiting for that callback.
    fetch(url).catch(error => process.stderr.write(`Fixture browser: ${error}\n`));
  };
}
process.argv = [process.execPath, proxy, ...args];
await import(pathToFileURL(proxy).href);
