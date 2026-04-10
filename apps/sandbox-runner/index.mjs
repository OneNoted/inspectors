import { spawn } from 'node:child_process';

const guestPort = process.env.GUEST_PORT ?? '4001';
const port = process.env.PORT ?? '3000';
const controlPlaneWorkspace = '@acu/control-plane';

function run(command, args, env = process.env) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { stdio: 'inherit', env });
    child.on('exit', (code) => {
      if ((code ?? 0) === 0) {
        resolve();
      } else {
        reject(new Error(`${command} ${args.join(' ')} exited with ${code}`));
      }
    });
  });
}

await run('bun', ['run', '--filter', controlPlaneWorkspace, 'build']);

const guest = spawn('cargo', ['run', '-p', 'guest-runtime', '--', '--port', guestPort], {
  stdio: 'inherit',
  env: { ...process.env, RUST_LOG: process.env.RUST_LOG ?? 'info' },
});

let control;
const stop = () => {
  guest.kill('SIGTERM');
  control?.kill('SIGTERM');
};

process.on('SIGINT', stop);
process.on('SIGTERM', stop);

await new Promise((resolve) => setTimeout(resolve, 1500));

control = spawn('bun', ['run', '--filter', controlPlaneWorkspace, 'start'], {
  stdio: 'inherit',
  env: { ...process.env, PORT: port, GUEST_RUNTIME_URL: `http://127.0.0.1:${guestPort}` },
});

control.on('exit', (code) => {
  guest.kill('SIGTERM');
  process.exit(code ?? 0);
});

guest.on('exit', (code) => {
  if ((code ?? 0) !== 0) {
    control?.kill('SIGTERM');
    process.exit(code ?? 1);
  }
});
