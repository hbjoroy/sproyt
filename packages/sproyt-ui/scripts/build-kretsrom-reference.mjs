import { readFile, mkdir, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const pkg = new URL('../', import.meta.url);
const archive = new URL('../../docs/prototypes/kretsrom/', pkg);
const frontend = new URL('../../frontend/', pkg);
const requireFrontend = createRequire(new URL('package.json', frontend));
const { build } = requireFrontend('esbuild');
const destination = new URL('demo-kretsrom/', pkg);
await mkdir(destination, { recursive: true });

const resizeBundle = await build({
  stdin: {
    contents: `
      import { createRoot } from 'react-dom/client';
      import { PanelResizers } from './src/ui/react/panel-resizers';
      const layout = document.querySelector('.spr .layout');
      layout.classList.add('sp-workspace');
      const mount = document.createElement('div');
      document.body.append(mount);
      createRoot(mount).render(<PanelResizers discussion={layout.querySelector('main')} threadOpen={false} />);
    `,
    resolveDir: fileURLToPath(frontend), loader: 'tsx'
  },
  bundle: true, jsx: 'automatic', minify: true, write: false,
  define: { 'process.env.NODE_ENV': '"production"' }
});

const stylesheet = await readFile(new URL('dist/kretsrom-reference.css', pkg), 'utf8');
for (const [filename, workshop] of [['kretsrom.html', false], ['verkstad-aktiv.html', true]]) {
  const original = await readFile(new URL(filename, archive), 'utf8');
  const shell = original.match(/<template\b[\s\S]*?<\/template>/)?.[0];
  const scenario = original.match(/<script>([\s\S]*?)<\/script>/)?.[1];
  if (!shell || !scenario) throw new Error('Missing archived fixture in ' + filename);
  // Retain the original people, data and state transitions. Only the package supplies styles.
  const page = `<!doctype html><html lang="nn"><meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <meta name="color-scheme" content="light dark"><title>${workshop ? 'Bileteverkstad' : 'Kretsrom'} · Sprøyt</title>
  <style>${stylesheet}</style>
  <div id="fredagsprat" class="kr-reference${workshop ? ' kr-workshop' : ''}">
    <div class="spr book tight" aria-label="Kretsrom"></div>${shell}
  </div>
  <script>${scenario}</script>
  <script>${resizeBundle.outputFiles[0].text.replace(/<\/script/gi, '<\\/script')}</script>
  </html>`;
  await writeFile(new URL(filename, destination), page);
}
