const fs = require('node:fs');
const { minify } = require('../demo/node_modules/terser');

(async () => {
  const path = process.argv[2];
  const result = await minify(fs.readFileSync(path, 'utf8'), { module: true, compress: true, mangle: true, format: { comments: false } });
  fs.writeFileSync(path, result.code + '\n');
})().catch(error => { console.error(error); process.exitCode = 1; });
