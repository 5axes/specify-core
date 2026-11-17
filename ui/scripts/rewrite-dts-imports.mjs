// tsc keeps `.ts` in the relative imports of the declarations it emits (the sources use them so
// that node can run the unit tests directly); consumers resolve extensionless specifiers.
import { readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const walk = dir => readdirSync(dir, { withFileTypes: true }).flatMap(e => e.isDirectory() ? walk(join(dir, e.name)) : [join(dir, e.name)]);
for (const file of walk('dist').filter(f => f.endsWith('.d.ts'))) {
  const text = readFileSync(file, 'utf8');
  const out = text.replace(/(from\s+|import\s*\(\s*)(['"])(\.{1,2}\/[^'"]*)\.ts\2/g, '$1$2$3$2');
  if (out !== text) writeFileSync(file, out);
}
