import { cpSync, mkdirSync, rmSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const controlPlaneRoot = join(repoRoot, 'apps', 'control-plane');
const webUiRoot = join(repoRoot, 'apps', 'web-ui', 'public');
const destinationRoot = join(repoRoot, 'crates', 'desktop-app', 'resources', 'control-plane');

rmSync(destinationRoot, { recursive: true, force: true });
mkdirSync(destinationRoot, { recursive: true });
cpSync(join(controlPlaneRoot, 'package.json'), join(destinationRoot, 'package.json'));
cpSync(join(controlPlaneRoot, 'dist'), join(destinationRoot, 'dist'), {
  recursive: true,
  filter: (source) => !source.endsWith('.test.js'),
});
cpSync(webUiRoot, join(destinationRoot, 'ui'), { recursive: true });
console.log(`synced desktop resources -> ${destinationRoot}`);
