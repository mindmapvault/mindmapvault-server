import { useEffect } from 'react';
import { useAuthStore } from '../store/auth';
import { useLobbyMetaStore } from '../store/lobbyMeta';
import { useModeStore } from '../store/mode';

export type { UserLabel } from '../utils/lobbyMeta';

/**
 * The label library the editor's tag picker and the lobby share.
 *
 * Server mode keeps it in the account settings on the server; local mode keeps
 * it in the encrypted lobby metadata of the unlocked profile. Either way it is
 * held in memory for the session and never written to WebView storage.
 */
export function useUserLabels() {
  const mode = useModeStore((s) => s.mode);
  const sessionKeys = useAuthStore((s) => s.sessionKeys);
  const username = useAuthStore((s) => s.username);
  const accessToken = useAuthStore((s) => s.accessToken);
  const labels = useLobbyMetaStore((s) => s.userLabels);

  useEffect(() => {
    const store = useLobbyMetaStore.getState();
    if (mode === 'server') {
      if (accessToken) void store.hydrateServer();
    } else if (mode === 'local' && sessionKeys && username) {
      void store.hydrateEncrypted(sessionKeys.masterKey, username);
    }
  }, [mode, sessionKeys, username, accessToken]);

  const { addUserLabel, removeUserLabel, updateUserLabelColor } = useLobbyMetaStore.getState();
  return {
    labels,
    addLabel: addUserLabel,
    removeLabel: removeUserLabel,
    updateLabelColor: updateUserLabelColor,
  };
}
