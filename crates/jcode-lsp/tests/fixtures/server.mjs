// A deterministic LSP peer for cross-platform protocol and lifecycle tests.
import fs from 'node:fs';
import { spawn } from 'node:child_process';

let buffer = Buffer.alloc(0);
let options = {};
let rootUri;
const documents = new Map();
const callbacks = new Map();

function send(value) {
  const body = Buffer.from(JSON.stringify(value));
  process.stdout.write(`Content-Length: ${body.length}\r\n\r\n`);
  process.stdout.write(body);
}
function reply(id, result) { send({ jsonrpc: '2.0', id, result }); }
function log(event) {
  if (options.log_file) fs.appendFileSync(options.log_file, JSON.stringify(event) + '\n');
}
function publish(uri, doc) {
  if (options.publish === false) return;
  if (options.uri_alias) uri = uri.replace(/^file:\/\/\/([a-z]):/i, (_, drive) => `file:///${drive.toLowerCase()}%3A`);
  const diagnostics = doc.text.includes('invalid') ? [{
    range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
    severity: 1, message: 'Fixture type error', source: 'fixture'
  }] : [];
  setTimeout(() => send({ jsonrpc: '2.0', method: 'textDocument/publishDiagnostics', params: {
    uri, version: options.versionless ? undefined : doc.version, diagnostics
  } }), options.diagnostic_delay_ms ?? 20);
}

async function receive(message) {
  if (!message.method) {
    const callback = callbacks.get(message.id);
    callbacks.delete(message.id);
    callback?.(message.result);
    return;
  }
  if (message.method === 'initialize') {
    options = message.params.initializationOptions ?? {};
    rootUri = message.params.rootUri;
    log({ event: 'start', pid: process.pid, rootUri });
    if (options.grandchild) {
      const grandchild = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore', windowsHide: true });
      log({ event: 'grandchild', pid: grandchild.pid });
    }
    const configuration = new Promise(resolve => callbacks.set('configuration-1', resolve));
    send({ jsonrpc: '2.0', id: 'configuration-1', method: 'workspace/configuration', params: { items: [{ section: 'fixture' }] } });
    const settings = await configuration;
    log({ event: 'configuration', settings });
    if (options.apply_edit) {
      const edit = new Promise(resolve => callbacks.set('edit-1', resolve));
      send({ jsonrpc: '2.0', id: 'edit-1', method: 'workspace/applyEdit', params: { edit: { changes: {} } } });
      log({ event: 'edit_response', result: await edit });
    }
    reply(message.id, { capabilities: {
      positionEncoding: options.encoding ?? 'utf-16',
      textDocumentSync: { openClose: true, change: options.full_sync ? 1 : 2, save: { includeText: true } },
      definitionProvider: options.definition !== false, referencesProvider: true,
      documentSymbolProvider: true,
      diagnosticProvider: options.pull ? { interFileDependencies: false, workspaceDiagnostics: false } : undefined
    } });
    return;
  }
  const params = message.params;
  log({ event: message.method, id: message.id, params });
  if (message.method === 'textDocument/didOpen') {
    documents.set(params.textDocument.uri, { ...params.textDocument });
    publish(params.textDocument.uri, params.textDocument);
  } else if (message.method === 'textDocument/didChange') {
    const uri = params.textDocument.uri;
    const old = documents.get(uri);
    const change = params.contentChanges[0];
    // The fixture verifies the old whole-document range before accepting an
    // incremental replacement, independently of the client's implementation.
    if (change.range) {
      const lines = old.text.split('\n');
      const tail = lines.at(-1);
      const width = options.encoding === 'utf-8' ? Buffer.byteLength(tail)
        : options.encoding === 'utf-32' ? [...tail].length : tail.length;
      if (change.range.end.line !== lines.length - 1 || change.range.end.character !== width) {
        process.exit(8);
      }
    }
    const doc = { text: change.text, version: params.textDocument.version };
    documents.set(uri, doc);
    if (options.publish !== false) send({ jsonrpc: '2.0', method: 'textDocument/publishDiagnostics', params: {
      uri, version: old.version, diagnostics: [{ message: 'stale publication' }]
    } });
    publish(uri, doc);
  } else if (message.method === 'textDocument/didClose') {
    documents.delete(params.textDocument.uri);
  } else if (message.method === 'textDocument/documentSymbol') {
    const doc = documents.get(params.textDocument.uri);
    if (!doc) return send({ jsonrpc: '2.0', id: message.id, error: { code: -32000, message: 'Document is not open' } });
    if (doc.text.includes('hang')) return;
    if (doc.text.includes('crash')) return process.exit(7);
    reply(message.id, [{ name: doc.text.trim(), kind: 13, detail: rootUri,
      range: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } },
      selectionRange: { start: { line: 0, character: 0 }, end: { line: 0, character: 1 } }
    }]);
  } else if (message.method === 'textDocument/definition' || message.method === 'textDocument/references') {
    const uri = params.textDocument.uri;
    const position = params.position;
    reply(message.id, [{ uri, range: { start: position, end: position } }]);
  } else if (message.method === 'textDocument/diagnostic') {
    const doc = documents.get(params.textDocument.uri);
    reply(message.id, { kind: 'full', items: doc.text.includes('invalid') ? [{ message: 'Pull type error' }] : [] });
  } else if (message.method === 'shutdown') {
    reply(message.id, null);
  } else if (message.method === 'exit') {
    process.exit(0);
  } else if (message.id !== undefined) {
    send({ jsonrpc: '2.0', id: message.id, error: { code: -32601, message: 'Unknown fixture request' } });
  }
}

process.stdin.on('data', chunk => {
  buffer = Buffer.concat([buffer, chunk]);
  while (true) {
    const end = buffer.indexOf('\r\n\r\n');
    if (end < 0) break;
    const length = Number(/content-length:\s*(\d+)/i.exec(buffer.subarray(0, end).toString())[1]);
    if (buffer.length < end + 4 + length) break;
    const message = JSON.parse(buffer.subarray(end + 4, end + 4 + length));
    buffer = buffer.subarray(end + 4 + length);
    receive(message).catch(error => { process.stderr.write(String(error)); process.exit(9); });
  }
});
