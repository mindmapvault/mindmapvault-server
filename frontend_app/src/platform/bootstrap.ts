import { getStorage, isTauri } from '../storage';
import { useAuthStore } from '../store/auth';
import { useModeStore } from '../store/mode';
import type { ConnectorRegistry, StorageConnector } from '@mindmapvault/connectors';

function createStorageConnector(): StorageConnector {
  const resolveStorage = () => {
    const mode = useModeStore.getState().mode;
    return getStorage(mode === 'local' ? 'local' : 'server');
  };

  return {
    listVaults: () => resolveStorage().listVaults(),
    getVault: (id) => resolveStorage().getVault(id),
    createVault: (body) => resolveStorage().createVault(body),
    updateVault: (id, body) => resolveStorage().updateVault(id, body),
    deleteVault: (id) => resolveStorage().deleteVault(id),
    uploadBlob: (id, blob) => resolveStorage().uploadBlob(id, blob),
    downloadBlob: (id) => resolveStorage().downloadBlob(id),
    updateMeta: (id, body) => resolveStorage().updateMeta(id, body),
    getStorage: () => resolveStorage().getStorage(),
  };
}

export function createConnectorRegistry(): ConnectorRegistry {
  return {
    target: 'server',
    storage: createStorageConnector(),
    auth: {
      capabilities: {
        canRegister: true,
        canChangePassword: true,
        hasSso: false,
        supportsOfflineUnlock: isTauri(),
      },
      getIdentity: () => {
        const state = useAuthStore.getState();
        return {
          username: state.username,
          authenticated: state.isAuthenticated(),
        };
      },
      isAuthenticated: () => useAuthStore.getState().isAuthenticated(),
      logout: () => useAuthStore.getState().logout(),
    },
    billing: {
      getPlan: () => 'free',
      isFeatureEnabled: () => false,
      openUpgradeFlow: () => {
        // Server/community default: no upgrade action.
      },
    },
    collaboration: {
      enabled: false,
    },
    features: {
      hasFeature: () => false,
    },
    telemetry: {
      track: () => {
        // No-op by default. Product bootstrap can override.
      },
    },
  };
}
