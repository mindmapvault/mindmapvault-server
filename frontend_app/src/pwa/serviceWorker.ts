import { registerSW } from 'virtual:pwa-register';

/**
 * Service-worker registration and the update signal that goes with it.
 *
 * The build uses `registerType: 'prompt'`, which means a new worker installs
 * and then *waits* until the page tells it to take over. Registering without an
 * `onNeedRefresh` callback leaves it waiting indefinitely: the operator has
 * upgraded the server, the new bundle is already downloaded, and every user
 * keeps running the release they first loaded until they happen to hard-reload.
 * That is what this module exists to prevent — it collects the signal so the UI
 * can offer the reload, and holds the function that activates the new worker.
 */

type Listener = (ready: boolean) => void;

const listeners = new Set<Listener>();
let updateReady = false;
let activate: ((reload: boolean) => Promise<void>) | null = null;

/** Called once from the entry point, before React mounts. */
export function startServiceWorker(): void {
  if (activate) return;
  activate = registerSW({
    onNeedRefresh() {
      updateReady = true;
      listeners.forEach((listener) => listener(true));
    },
  });
}

export function isUpdateReady(): boolean {
  return updateReady;
}

export function onUpdateReady(listener: Listener): () => void {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}

/**
 * Activates the waiting worker and reloads. `registerSW` resolves once the new
 * worker has taken control; it triggers the reload itself.
 */
export function applyUpdate(): void {
  void activate?.(true);
}
