import { startControlPlaneServer } from './server.js';

const port = Number(process.env.PORT ?? 3000);
const guestRuntimeUrl = process.env.GUEST_RUNTIME_URL ?? 'http://127.0.0.1:4001';

startControlPlaneServer(port, guestRuntimeUrl)
  .then(() => {
    console.log(`control-plane listening on http://127.0.0.1:${port}`);
    console.log(`control-plane guest runtime -> ${guestRuntimeUrl}`);
  })
  .catch((error) => {
    console.error('failed to start control-plane', error);
    process.exit(1);
  });
