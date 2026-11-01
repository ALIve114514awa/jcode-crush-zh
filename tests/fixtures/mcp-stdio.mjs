// Portable stdio regression peer; never calls a provider or external service.
import readline from 'node:readline';
const input = readline.createInterface({ input: process.stdin });
input.on('line', line => {
  const request = JSON.parse(line);
  if (request.id === undefined) return;
  let result;
  if (request.method === 'initialize') {
    result = { protocolVersion: request.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: 'stdio-fixture', version: '1' } };
  } else if (request.method === 'tools/list') {
    result = { tools: [{ name: 'echo', description: 'Fixture', inputSchema: { type: 'object', properties: { text: { type: 'string' } } } }] };
  } else if (request.method === 'tools/call') {
    const text = request.params.arguments.text;
    if (text === 'fixture-exit') process.exit(0);
    result = { content: [{ type: 'text', text: JSON.stringify({ text, sensitive_inherited: Boolean(process.env.JCODE_PRIVATE_API_KEY) }) }], isError: false };
  } else {
    result = {};
  }
  process.stdout.write(JSON.stringify({ jsonrpc: '2.0', id: request.id, result }) + '\n');
});
input.on('close', () => process.exit(0));
