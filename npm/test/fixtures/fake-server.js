#!/usr/bin/env node
// A tiny MCP server for tests: newline-delimited JSON-RPC over stdio.
// Every call is appended to $FAKE_LOG so tests can see what really reached the tool.
import fs from 'node:fs';
import readline from 'node:readline';

const TOOLS = [
  { name: 'read_note', description: 'Read a note', inputSchema: { type: 'object' }, annotations: { readOnlyHint: true } },
  { name: 'get_customer', description: 'Look up a customer', inputSchema: { type: 'object' }, annotations: { readOnlyHint: true } },
  { name: 'send_email', description: 'Send an email', inputSchema: { type: 'object' }, annotations: { readOnlyHint: false, openWorldHint: true } },
  { name: 'pay', description: 'Pay an invoice', inputSchema: { type: 'object' }, annotations: { readOnlyHint: false, openWorldHint: true } },
  { name: 'wipe', description: 'Delete everything', inputSchema: { type: 'object' }, annotations: { readOnlyHint: false, destructiveHint: true } },
  { name: 'mystery', description: 'No annotations at all', inputSchema: { type: 'object' } },
];

const out = (msg) => process.stdout.write(JSON.stringify(msg) + '\n');
const rl = readline.createInterface({ input: process.stdin });
rl.on('line', (line) => {
  if (!line.trim()) return;
  const m = JSON.parse(line);
  if (m.id === undefined) return;
  if (m.method === 'initialize') return out({ jsonrpc: '2.0', id: m.id, result: { protocolVersion: m.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: 'fake', version: '1' } } });
  if (m.method === 'tools/list') return out({ jsonrpc: '2.0', id: m.id, result: { tools: TOOLS } });
  if (m.method === 'tools/call') {
    if (process.env.FAKE_LOG) fs.appendFileSync(process.env.FAKE_LOG, JSON.stringify({ tool: m.params.name, args: m.params.arguments, key: process.env.FAKE_KEY || null }) + '\n');
    const text = m.params.name === 'get_customer'
      ? 'Ana Lima, ana.lima@example.com, +44 20 7946 0958, card 4242 4242 4242 4242, IBAN GB82WEST12345698765432, key sk_live_abcdefghijklmnop1234'
      : `${m.params.name} ok`;
    return out({ jsonrpc: '2.0', id: m.id, result: { content: [{ type: 'text', text }] } });
  }
  out({ jsonrpc: '2.0', id: m.id, error: { code: -32601, message: 'no such method' } });
});
