import { spawn } from 'node:child_process';
// A small stdio MCP client for the tests: `specify-core mcp` started as an agent starts it, spoken
// to in the legacy 2025-11-25 revision (initialize, then tools/call), one JSON-RPC message a line.

/** Starts ``cli mcp`` in ``cwd`` with ``env``; ``call(tool, args)`` gives a tool's structuredContent
 * (throwing on isError unless ``{ allowError: true }``). */
export async function mcp(cli, { cwd, env = {}, args = [] } = {}) {
  const child = spawn(cli, ['mcp', ...args], { cwd, env: { ...process.env, ...env }, stdio: ['pipe', 'pipe', 'pipe'] });
  let buffer = '';
  let stderr = '';
  let id = 0;
  const waiting = new Map();
  child.stdout.setEncoding('utf8');
  child.stdout.on('data', chunk => {
    buffer += chunk;
    for (let at = buffer.indexOf('\n'); at >= 0; at = buffer.indexOf('\n')) {
      const line = buffer.slice(0, at).trim();
      buffer = buffer.slice(at + 1);
      if (!line) continue;
      const message = JSON.parse(line);
      waiting.get(message.id)?.(message);
      waiting.delete(message.id);
    }
  });
  child.stderr.on('data', chunk => { stderr += chunk; });
  const request = (method, params) => new Promise((resolve, reject) => {
    const mine = ++id;
    const timer = setTimeout(() => reject(new Error(`${method} timed out; stderr: ${stderr}`)), 120000);
    waiting.set(mine, message => { clearTimeout(timer); message.error ? reject(new Error(`${method}: ${JSON.stringify(message.error)}`)) : resolve(message.result); });
    child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id: mine, method, params })}\n`);
  });
  await request('initialize', { protocolVersion: '2025-11-25', capabilities: {}, clientInfo: { name: 'specify-ui-test', version: '0' } });
  child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' })}\n`);
  return {
    async call(name, args, { allowError = false } = {}) {
      const result = await request('tools/call', { name, arguments: args });
      if (result.isError && !allowError) throw new Error(`${name}: ${JSON.stringify(result.structuredContent ?? result.content)}`);
      return result.structuredContent;
    },
    stderr: () => stderr,
    stop: () => child.kill(),
  };
}
