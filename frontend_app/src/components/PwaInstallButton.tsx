import { useEffect, useMemo, useState } from 'react';

const DISMISSED_STORAGE_KEY = 'mindmapvault-pwa-install-dismissed-v1';

interface BeforeInstallPromptEvent extends Event {
  prompt: () => Promise<void>;
  userChoice: Promise<{ outcome: 'accepted' | 'dismissed'; platform: string }>;
}

function isStandaloneMode(): boolean {
  if (typeof window === 'undefined') return false;
  const nav = window.navigator as Navigator & { standalone?: boolean };
  return window.matchMedia('(display-mode: standalone)').matches || nav.standalone === true;
}

function isIosSafariBrowser(): boolean {
  if (typeof window === 'undefined') return false;
  const ua = window.navigator.userAgent;
  const isiOS = /iPad|iPhone|iPod/.test(ua);
  const isSafari = /Safari/.test(ua) && !/CriOS|FxiOS|EdgiOS/.test(ua);
  return isiOS && isSafari;
}

function isTauriShell(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

interface PwaInstallButtonProps {
  large?: boolean;
  className?: string;
  showDismiss?: boolean;
  onDismissed?: () => void;
}

export function PwaInstallButton({ large = false, className = '', showDismiss = false, onDismissed }: PwaInstallButtonProps) {
  const [deferredPrompt, setDeferredPrompt] = useState<BeforeInstallPromptEvent | null>(null);
  const [installed, setInstalled] = useState(false);
  const [showIosHelp, setShowIosHelp] = useState(false);
  const [dismissed, setDismissed] = useState(false);

  const iosSafari = useMemo(() => isIosSafariBrowser(), []);

  useEffect(() => {
    if (isTauriShell()) return;

    try {
      setDismissed(localStorage.getItem(DISMISSED_STORAGE_KEY) === '1');
    } catch {
      setDismissed(false);
    }

    setInstalled(isStandaloneMode());

    const onBeforeInstallPrompt = (event: Event) => {
      event.preventDefault();
      setDeferredPrompt(event as BeforeInstallPromptEvent);
    };

    const onAppInstalled = () => {
      setInstalled(true);
      setDeferredPrompt(null);
      setShowIosHelp(false);
    };

    const onDisplayModeChange = () => {
      if (isStandaloneMode()) {
        setInstalled(true);
        setShowIosHelp(false);
      }
    };

    window.addEventListener('beforeinstallprompt', onBeforeInstallPrompt);
    window.addEventListener('appinstalled', onAppInstalled);
    window.matchMedia('(display-mode: standalone)').addEventListener('change', onDisplayModeChange);

    return () => {
      window.removeEventListener('beforeinstallprompt', onBeforeInstallPrompt);
      window.removeEventListener('appinstalled', onAppInstalled);
      window.matchMedia('(display-mode: standalone)').removeEventListener('change', onDisplayModeChange);
    };
  }, []);

  if (isTauriShell() || installed) {
    return null;
  }

  if (dismissed) {
    return null;
  }

  const onInstallClick = async () => {
    if (deferredPrompt) {
      await deferredPrompt.prompt();
      const choice = await deferredPrompt.userChoice;
      if (choice.outcome === 'accepted') {
        setInstalled(true);
      } else if (choice.outcome === 'dismissed') {
        try {
          localStorage.setItem(DISMISSED_STORAGE_KEY, '1');
        } catch {
          // Ignore storage failures and continue hiding for current session.
        }
        setDismissed(true);
        onDismissed?.();
      }
      setDeferredPrompt(null);
      return;
    }

    setShowIosHelp((prev) => !prev);
  };

  const onDismissClick = () => {
    try {
      localStorage.setItem(DISMISSED_STORAGE_KEY, '1');
    } catch {
      // Ignore storage failures and continue hiding for current session.
    }
    setDismissed(true);
    onDismissed?.();
  };

  const buttonClassName = large
    ? 'w-full rounded-xl border border-accent/40 bg-accent/10 px-4 py-3 text-base font-semibold text-slate-100 transition hover:border-accent hover:bg-accent/20 hover:text-white'
    : 'rounded-lg border border-slate-600 px-3 py-2 text-sm text-slate-300 transition hover:border-slate-500 hover:text-white sm:py-1.5';

  const helpText = iosSafari
    ? 'In Safari: Share -> Add to Home Screen'
    : 'If no prompt appears, use your browser menu and choose Install app.';

  return (
    <div className={`flex flex-col items-start gap-2 ${className}`.trim()}>
      <div className={`flex ${showDismiss ? 'w-full items-center gap-2' : 'items-center'}`}>
        <button
          onClick={() => { void onInstallClick(); }}
          className={`${buttonClassName} ${showDismiss ? 'flex-1' : ''}`.trim()}
          title="Install the web app on this device"
        >
          Install app
        </button>
        {showDismiss && (
          <button
            type="button"
            onClick={onDismissClick}
            className="rounded-xl border border-slate-500/70 px-4 py-3 text-sm font-medium text-slate-300 transition hover:border-slate-400 hover:text-white"
            title="Dismiss install prompt"
          >
            Dismiss
          </button>
        )}
      </div>
      {showIosHelp && (
        <span className="text-xs text-slate-400">
          {helpText}
        </span>
      )}
    </div>
  );
}
