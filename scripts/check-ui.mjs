import { readFileSync } from 'node:fs';
import { Script } from 'node:vm';

// Syntax checking only: this does not invoke the Tauri bridge or send requests.
const html = readFileSync(new URL('../src/index.html', import.meta.url), 'utf8');
const scripts = [...html.matchAll(/<script\b[^>]*>([\s\S]*?)<\/script>/gi)];
if (!scripts.length) throw new Error('Expected an inline frontend script');
for (const [index, match] of scripts.entries()) {
  new Script(match[1], { filename: `src/index.html:inline-script-${index + 1}` });
}
console.log(`Checked ${scripts.length} frontend script(s)`);
