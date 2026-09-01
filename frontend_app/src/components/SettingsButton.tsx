import { useState } from 'react';
import { SettingsModal, type SettingsTab } from './SettingsModal';

interface SettingsButtonProps {
  /** Styling for the trigger, so it can sit in any of the app's toolbars. */
  className?: string;
  /** The editor toolbar sizes its own icons; everywhere else needs a size. */
  iconClassName?: string;
  initialTab?: SettingsTab;
}

/**
 * The gear that opens the settings hub. It owns the open state so each toolbar
 * can drop in a single element, the way the theme popover it replaced did.
 */
export function SettingsButton({
  className = '',
  iconClassName = 'h-5 w-5',
  initialTab = 'account',
}: SettingsButtonProps) {
  const [open, setOpen] = useState(false);

  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        title="Settings"
        aria-label="Settings"
        data-testid="open-settings"
        className={className}
      >
        <svg className={iconClassName} fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.8}>
          <circle cx="12" cy="12" r="3" />
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.6 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.6a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9v0a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"
          />
        </svg>
      </button>
      <SettingsModal open={open} onClose={() => setOpen(false)} initialTab={initialTab} />
    </>
  );
}

export default SettingsButton;
