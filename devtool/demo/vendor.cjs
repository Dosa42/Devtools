const fs = require('node:fs/promises');
const path = require('node:path');
const { minify } = require('terser');

async function main() {
  const output = path.join(__dirname, 'vendor');
  await fs.mkdir(output, { recursive: true });
  for (const [packageName, filename] of [['@xterm/xterm', 'xterm'], ['@xterm/addon-fit', 'addon-fit']]) {
    const root = path.join(__dirname, 'node_modules', packageName);
    const source = await fs.readFile(path.join(root, 'lib', `${filename}.js`), 'utf8');
    const result = await minify(source, { format: { comments: false }, sourceMap: false });
    await fs.writeFile(path.join(output, `${filename}.js`), result.code);
    await fs.copyFile(path.join(root, 'LICENSE'), path.join(output, `${filename}.LICENSE`));
  }
  const stylesheet = await fs.readFile(path.join(__dirname, 'node_modules/@xterm/xterm/css/xterm.css'), 'utf8');
  await fs.writeFile(path.join(output, 'xterm.css'), stylesheet.replace(/\/\*[\s\S]*?\*\//g, ''));
}

main().catch(error => { process.stderr.write(`${error.stack}\n`); process.exitCode = 1; });
