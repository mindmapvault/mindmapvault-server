import { useNavigate } from 'react-router-dom';
import { isTauri } from '../storage';
import { useModeStore } from '../store/mode';

export function ModePage() {
  const navigate = useNavigate();
  const setMode = useModeStore((s) => s.setMode);
  const isDesktop = isTauri();

  const chooseServer = () => {
    setMode('server');
    navigate('/login');
  };

  const chooseLocal = () => {
    setMode('local');
    navigate('/local-unlock');
  };

  return (
    <div className="min-h-screen flex items-center justify-center bg-[var(--bg)] transition-colors">
      <div className="w-full max-w-lg px-6">
        <div className="text-center mb-10">
          <h1 className="text-4xl font-bold text-[var(--fg)] mb-2">
            <span className="text-[var(--accent)]">MindMap</span>Vault
          </h1>
          <p className="text-[var(--fg-muted)] text-sm">
            Zero-knowledge encrypted mind maps
          </p>
        </div>

        <div className="space-y-4">
          {/* Server mode — always available */}
          <button
            onClick={chooseServer}
            className="w-full px-6 py-5 rounded-xl border border-[var(--border)]
                       bg-[var(--card)] hover:border-[var(--accent)] transition-all
                       text-left group"
          >
            <div className="flex items-center gap-4">
              <div className="w-12 h-12 rounded-lg bg-[var(--accent)]/10 flex items-center justify-center
                              text-[var(--accent)] text-xl">
                ☁
              </div>
              <div>
                <h2 className="text-lg font-semibold text-[var(--fg)] group-hover:text-[var(--accent)] transition-colors">
                  Connect to Server
                </h2>
                <p className="text-sm text-[var(--fg-muted)]">
                  Sync encrypted vaults across devices via MindMapVault Cloud (paid plan later)
                </p>
              </div>
            </div>
          </button>

          {/* Local mode — only in Tauri desktop app */}
          {isDesktop ? (
            <button
              onClick={chooseLocal}
              className="w-full px-6 py-5 rounded-xl border border-[var(--border)]
                         bg-[var(--card)] hover:border-[var(--accent)] transition-all
                         text-left group"
            >
              <div className="flex items-center gap-4">
                <div className="w-12 h-12 rounded-lg bg-emerald-500/10 flex items-center justify-center
                                text-emerald-400 text-xl">
                  💾
                </div>
                <div>
                  <h2 className="text-lg font-semibold text-[var(--fg)] group-hover:text-emerald-400 transition-colors">
                    Local Storage
                  </h2>
                  <p className="text-sm text-[var(--fg-muted)]">
                    Keep everything on this device (free) — no server needed. Files are encrypted on disk.
                  </p>
                </div>
              </div>
            </button>
          ) : (
            <div className="w-full px-6 py-5 rounded-xl border border-[var(--border)]
                            bg-[var(--card)] opacity-50 text-left">
              <div className="flex items-center gap-4">
                <div className="w-12 h-12 rounded-lg bg-gray-500/10 flex items-center justify-center
                                text-gray-400 text-xl">
                  💾
                </div>
                <div>
                  <h2 className="text-lg font-semibold text-[var(--fg)]">
                    Local Storage
                  </h2>
                  <p className="text-sm text-[var(--fg-muted)]">
                    Available in the desktop app only.
                    <a href="https://mindmapvault.com/download" target="_blank"
                       className="text-[var(--accent)] ml-1 hover:underline">
                      Download →
                    </a>
                  </p>
                </div>
              </div>
            </div>
          )}
        </div>

        <p className="text-center text-xs text-[var(--fg-muted)] mt-8">
          All encryption happens locally — the server never sees your data.
        </p>
      </div>
    </div>
  );
}
