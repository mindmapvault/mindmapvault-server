import DOMPurify from 'dompurify';
import { marked } from 'marked';
import { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { LogoWithText } from '../components/Logo';
import { useThemeStore } from '../store/theme';
import packageJson from '../../package.json';

function WindowsMark({ className = 'h-4 w-4' }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
      <path d="M2 3.5 10.5 2v9H2v-7.5Zm10.5-1.7L22 0v11h-9.5V1.8ZM2 13h8.5v9L2 20.5V13Zm10.5 0H22v11l-9.5-1.5V13Z" />
    </svg>
  );
}

function LinuxMark({ className = 'h-4 w-4' }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" aria-hidden="true">
      <path d="M12 2.8c1.56 0 2.7 1.24 2.7 2.87 0 .87-.3 1.64-.78 2.17 1.1.38 2.15 1.31 2.68 2.85l1.43 4.15c.32.93.12 1.96-.53 2.7-.63.73-1.58 1.15-2.56 1.15H9.06c-.98 0-1.93-.42-2.56-1.15-.65-.74-.85-1.77-.53-2.7l1.43-4.15c.53-1.54 1.58-2.47 2.68-2.85-.48-.53-.78-1.3-.78-2.17 0-1.63 1.14-2.87 2.7-2.87Z" fill="currentColor" />
      <circle cx="10.15" cy="5.55" r="0.7" fill="#0F172A" />
      <circle cx="13.85" cy="5.55" r="0.7" fill="#0F172A" />
      <path d="M10.4 7.55c.42.36.96.55 1.6.55.64 0 1.18-.19 1.6-.55" stroke="#0F172A" strokeWidth="1" strokeLinecap="round" />
      <path d="M8.45 18.25c.36 1.48 1.57 2.4 3.55 2.4 1.98 0 3.19-.92 3.55-2.4" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
      <path d="M8.1 14.9c.82.52 1.92.8 3.9.8 1.98 0 3.08-.28 3.9-.8" stroke="#E5E7EB" strokeWidth="1.1" strokeLinecap="round" />
    </svg>
  );
}

function TauriWordmark() {
  return (
    <span className="inline-flex items-center gap-2">
      <img
        src="https://v2.tauri.app/logo.png"
        alt="Tauri"
        className="h-5 w-auto shrink-0"
        loading="lazy"
      />
    </span>
  );
}

const currentReleaseVersion = packageJson.version;
const windowsDownloadUrl = `https://downloads.mindmapvault.com/windows/${currentReleaseVersion}/MindMapVault_${currentReleaseVersion}_x64-setup.exe`;

const customerChangelog = [
  {
    version: currentReleaseVersion,
    date: 'April 29, 2026',
    title: 'File uploads and note attachment previews are fixed in the hosted app',
    summary:
      'This release fixes the production attachment upload path, makes note attachment links behave correctly again, and adds a larger in-app preview for note images.',
    changes: [
      'Encrypted attachment uploads now use the correct hosted backend path even when the API base is configured with an `/api` suffix.',
      'Attachment links rendered inside note previews now open through the app instead of triggering a broken `attachment://...` browser navigation attempt.',
      'Clicking an image attachment thumbnail or inline note image now opens a larger in-app preview dialog.',
      'The hosted login screen now shows the current app version directly on the canvas, with light and dark theme styling kept consistent.',
    ],
  },
  {
    version: '0.3.12',
    date: 'May 4, 2026',
    title: 'More storage, light mode for the files dialog',
    summary:
      'Free accounts now get 25 MB of encrypted cloud storage and Pro accounts get 250 MB. The vault files and share exports dialog now correctly follows light mode.',
    changes: [
      'Free cloud accounts now include 25 MB of encrypted storage (up from 10 MB).',
      'Pro cloud accounts now include 250 MB of encrypted storage (up from 100 MB).',
      'Per-file attachment size limit raised to 5 MB for free and 50 MB for Pro.',
      'The Files / Share exports dialog now correctly follows the light/dark theme instead of always appearing dark.',
    ],
  },
  {
    version: '0.3.11',
    date: 'April 18, 2026',
    title: 'Encrypted share links now open in a real recipient page',
    summary:
      'This release finishes the recipient side of encrypted share exports, so people opening a share link can unlock, preview, download, and import the encrypted snapshot directly in the browser.',
    changes: [
      'Shared encrypted vault links now open a dedicated browser page instead of returning raw JSON from the backend.',
      'Recipients can enter the share passphrase locally, preview the decrypted outline, and download the decrypted export package in the browser.',
      'Signed-in users can import a shared encrypted vault into their own workspace, and included attachments are re-encrypted for the recipient during import when supported.',
    ],
  },
  {
    version: '0.3.10',
    date: 'April 18, 2026',
    title: 'Encrypted attachments and share exports now live inside the editor',
    summary:
      'This release moves the encrypted cloud workflow into the hosted editor, so attachments and password-protected share exports can be managed directly from the current vault view.',
    changes: [
      'The hosted editor now includes a secure vault panel for encrypted attachments, including upload, node assignment, owner-side decrypted download, and deletion.',
      'Password-protected encrypted share exports can now be created directly from the current editor snapshot, with optional encrypted attachment inclusion and one-click link copying.',
      'The app now says the current limit clearly: encrypted share links are export-based today, and encrypted live collaboration with editor presence is not available yet.',
    ],
  },
  {
    version: '0.3.9',
    date: 'April 18, 2026',
    title: 'Cloud account settings and pager inbox are now available in the app',
    summary:
      'This release connects the hosted app to the new backend account settings and notification surfaces, so cloud users can manage synced defaults and triage pager items directly inside the product.',
    changes: [
      'The hosted app settings panel now includes synced cloud defaults for locale, timezone, date format, share expiry, map layout, export format, and share attachment preferences.',
      'Notification preferences for inbox, desktop, email, digest, quiet hours, and key event categories can now be updated directly from the hosted app.',
      'A new pager inbox inside the app now lists recent backend events and lets users mark items read, saved, or done without leaving the main workspace.',
    ],
  },
  {
    version: '0.3.8',
    date: 'April 18, 2026',
    title: 'Encrypted attachments and password-protected sharing are now available',
    summary:
      'This release expands the encrypted cloud workflow directly: cloud mind maps can now carry encrypted attachments, and encrypted share exports can now be created and revoked through dedicated share links.',
    changes: [
      'Encrypted cloud mind maps can now include uploaded attachments with encrypted metadata, download access, and cleanup when the parent map is removed.',
      'Password-protected encrypted share exports can now be created from a cloud mind map and downloaded through a dedicated share link.',
      'Shared encrypted attachments can now travel with the exported encrypted mind map package when needed.',
    ],
  },
  {
    version: '0.3.7',
    date: 'April 13, 2026',
    title: 'Admin controls are more complete for support and billing work',
    summary:
      'This release expands the internal admin dashboard so support actions, billing exceptions, and feedback cleanup can be handled from one place instead of with manual backend checks.',
    changes: [
      'The admin dashboard now shows each user’s used storage, vault count, effective plan, billing source, and lock state in a more actionable management view.',
      'Admins can now add internal notes, record a lock reason, and manually grant or clear paid access overrides without replacing the Stripe-owned billing record.',
      'Public feedback items can now be archived and restored in addition to being deleted, and the admin overview now includes a recent action timeline.',
    ],
  },
  {
    version: '0.3.6',
    date: 'April 13, 2026',
    title: 'Mobile editing is more usable, and hosted sign-in is temporarily simpler',
    summary:
      'This release improves the hosted app directly: mobile vault editing now has a touch-first interface, and hosted sign-in can temporarily run without the verification step while we stabilize browser and mobile challenge reliability.',
    changes: [
      'The app can now switch between the desktop canvas editor and a touch-first mobile editor without changing routes or data format.',
      'The mobile editor now uses list-first navigation, branch breadcrumbs, quick child actions, and a bottom-sheet inspector that is easier to use on small screens.',
      'Hosted sign-in verification can now be disabled with a runtime switch while we work through the challenge failures seen on some mobile and privacy-heavy browser setups.',
    ],
  },
  {
    version: '0.3.5',
    date: 'April 12, 2026',
    title: 'Verification is stricter and hosted security wiring is more reliable',
    summary:
      'This release tightens the hosted sign-in and public feedback verification flow behind the scenes so human-verification checks are applied more reliably in production.',
    changes: [
      'Hosted sign-in, registration, and public feedback verification now validate the expected MindMapVault host more strictly before a challenge response is accepted.',
      'The hosted feedback flow now fails safely if backend verification is misconfigured instead of silently accepting unverified submissions.',
      'Release metadata and backend versioning are now aligned to the same 0.3.5 release line.',
    ],
  },
  {
    version: '0.3.4',
    date: 'April 10, 2026',
    title: 'Safer sign-in and clearer website status labels',
    summary:
      'This release improves the public-facing web experience: the hosted sign-in and feedback flow now have better abuse protection, and the marketing site more clearly marks links and downloads that are not published yet.',
    changes: [
      'The public feedback form and hosted web sign-in now include a human-verification check to reduce automated abuse.',
      'The marketing site now clearly marks the GitHub link as coming soon.',
      'The marketing site now clearly marks the Linux desktop download as coming soon.',
    ],
  },
  {
    version: '0.3.3',
    date: 'April 6, 2026',
    title: 'Windows desktop download is now published for direct install',
    summary:
      'This release makes the Windows desktop build available as a direct download, aligns the visible product version across the app and desktop package, and adds human-verification checks to the public feedback flow and hosted web sign-in.',
    changes: [
      'The Windows desktop app is now published as a direct downloadable Windows installer package.',
      'Version labels across the app surfaces and desktop package were aligned to the same public release.',
      'The Windows download button now points at the hosted desktop build instead of a placeholder route.',
      'The hosted web sign-in and public feedback form now include a human-verification step to reduce automated abuse.',
    ],
  },
  {
    version: '0.3.2',
    date: 'April 6, 2026',
    title: 'Cloud saves are smoother, and web screens are cleaner',
    summary:
      'This release improves the hosted app experience directly: encrypted cloud uploads are now handled more reliably through the app backend, and a few rough edges in the web interface were removed.',
    changes: [
      'Cloud vault saves now use a backend-mediated upload path, which avoids the earlier public object-storage upload failure some hosted users were seeing.',
      'The subscription badge now looks correct in light mode, including the free-plan label styling.',
      'The web login and registration screens were simplified by removing the unnecessary login-mode link from the hosted app flow.',
    ],
  },
  {
    version: '0.3.1',
    date: 'April 6, 2026',
    title: 'Clearer updates, direct support, and simpler feedback handling',
    summary:
      'This release makes MindMapVault easier to follow from the outside: customers can now see recent progress, contact support directly, and expect feedback to be reviewed inside the product instead of disappearing into a technical mail setup.',
    changes: [
      'The app landing page now has a readable changelog focused on user-facing improvements rather than internal implementation details.',
      'Project version labels are visible in the interface so users can tell which release they are looking at.',
      'Support contact is clearer, with direct email access and a simpler feedback path for future admin review.',
    ],
  },
  {
    version: '0.3.0',
    date: 'March 21, 2026',
    title: 'Cloud backend upgrade and stronger reliability',
    summary:
      'MindMapVault now runs on a more flexible backend stack, with better storage reliability and clearer room to grow without changing the encryption model.',
    changes: [
      'Cloud accounts can now run on more than one database setup behind the scenes without changing the user-facing product.',
      'Version history handling is more reliable, especially for users who save often or keep a longer trail of changes.',
      'Cloud storage access became more stable under heavier usage and repeated encrypted uploads.',
    ],
  },
  {
    version: '0.2.0',
    date: 'March 18, 2026',
    title: 'Vault management became much more practical',
    summary:
      'This release focused on day-to-day usability: clearer storage information, version browsing, and easier vault organization without re-uploading everything.',
    changes: [
      'Vaults can be renamed more cleanly from both the vault list and the editor.',
      'A version history panel makes it easier to inspect or restore earlier encrypted saves.',
      'Storage usage is clearer, so free and paid limits are easier to understand before they become a problem.',
    ],
  },
  {
    version: '0.1.0',
    date: 'March 17, 2026',
    title: 'First usable encrypted release',
    summary:
      'The first public milestone established the core product: encrypted cloud vaults, a working web app, desktop local mode, and the current security model.',
    changes: [
      'Cloud registration, login, and encrypted vault saving became available end to end.',
      'The browser and desktop clients started sharing the same core encrypted editing experience.',
      'The product security model and whitepaper became available for review from day one.',
    ],
  },
];

const upcomingChanges = [
  'Admin review of landing-page feedback inside the product instead of handling everything by email.',
  'Smoother deployment flow for backend updates so hosted releases are less manual to maintain.',
  'Cleaner customer-facing release notes and support surfaces across the web app and landing page.',
];

export function LandingPage() {
  const { mode, toggleMode } = useThemeStore();
  const [whitepaperOpen, setWhitepaperOpen] = useState(false);
  const [whitepaperLoading, setWhitepaperLoading] = useState(false);
  const [whitepaperError, setWhitepaperError] = useState('');
  const [whitepaperHtml, setWhitepaperHtml] = useState('');
  const uiVersion = packageJson.version;

  useEffect(() => {
    if (!whitepaperOpen || whitepaperHtml) return;

    let cancelled = false;
    (async () => {
      setWhitepaperLoading(true);
      setWhitepaperError('');
      try {
        const res = await fetch('/SECURITY.md', { cache: 'no-store' });
        if (!res.ok) throw new Error(`Failed to load whitepaper (${res.status})`);
        const md = await res.text();
        const parsed = marked.parse(md, { async: false }) as string;
        if (!cancelled) setWhitepaperHtml(DOMPurify.sanitize(parsed));
      } catch (err) {
        if (!cancelled) {
          setWhitepaperError(err instanceof Error ? err.message : 'Failed to load whitepaper');
        }
      } finally {
        if (!cancelled) setWhitepaperLoading(false);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [whitepaperOpen, whitepaperHtml]);

  return (
    <div className="min-h-screen bg-surface text-white">
      <header className="border-b border-slate-700 bg-surface-1/80">
        <div className="mx-auto flex w-full max-w-6xl items-center justify-between px-4 py-4 sm:px-6">
          <div className="flex items-center gap-3">
            <LogoWithText size={28} />
            <span className="rounded-full border border-slate-700 bg-surface px-3 py-1 text-[11px] font-semibold uppercase tracking-[0.2em] text-slate-400">
              Project v{currentReleaseVersion}
            </span>
          </div>
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={toggleMode}
              className="inline-flex items-center gap-1.5 rounded-lg border border-slate-600 px-3 py-1.5 text-sm text-slate-300 transition hover:border-slate-500 hover:text-white"
              title={mode === 'dark' ? 'Switch to light mode' : 'Switch to dark mode'}
            >
              {mode === 'dark' ? (
                <>
                  <svg className="h-4 w-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={1.8}>
                    <circle cx="12" cy="12" r="4" />
                    <path strokeLinecap="round" strokeLinejoin="round" d="M12 2v2m0 16v2M4.93 4.93l1.41 1.41m11.32 11.32 1.41 1.41M2 12h2m16 0h2M4.93 19.07l1.41-1.41m11.32-11.32 1.41-1.41" />
                  </svg>
                  <span>Light</span>
                </>
              ) : (
                <>
                  <svg className="h-4 w-4" fill="currentColor" viewBox="0 0 24 24">
                    <path d="M21 12.79A9 9 0 1111.21 3a7 7 0 009.79 9.79z" />
                  </svg>
                  <span>Dark</span>
                </>
              )}
            </button>
            <Link to="/login" className="rounded-lg border border-slate-600 px-3 py-1.5 text-sm text-slate-300 transition hover:border-slate-500 hover:text-white">
              Sign in
            </Link>
            <Link to="/register" className="rounded-lg bg-accent px-3 py-1.5 text-sm font-medium text-white transition hover:bg-accent-hover">
              Create account
            </Link>
          </div>
        </div>
      </header>

      <main className="mx-auto w-full max-w-6xl px-4 py-12 sm:px-6">
        <section className="rounded-2xl border border-slate-700 bg-surface-1 p-8">
          <h1 className="text-4xl font-bold tracking-tight text-white sm:text-5xl">Private mind maps, online or offline.</h1>
          <p className="mt-4 max-w-3xl text-base text-slate-300 sm:text-lg">
            MindMapVault is a zero-knowledge encrypted workspace for your ideas. Use it in the cloud on
            mindmapvault.com or install the desktop app for local-first vaults on Windows and Linux.
          </p>
          <div className="mt-6 flex flex-wrap gap-3">
            <Link to="/register" className="rounded-lg bg-accent px-5 py-2.5 text-sm font-medium text-white transition hover:bg-accent-hover">
              Start free cloud account
            </Link>
            <a
              href={windowsDownloadUrl}
              className="inline-flex items-center gap-3 rounded-lg border border-slate-600 px-5 py-2.5 text-sm text-slate-300 transition hover:border-slate-500 hover:text-white"
            >
              <span className="inline-flex items-center gap-2">
                <WindowsMark className="h-4 w-4" />
                <span>Download for Windows</span>
              </span>
              <span className="hidden h-4 w-px bg-slate-600 sm:block" aria-hidden="true" />
              <span className="hidden items-center gap-2 sm:inline-flex">
                <span className="text-[10px] font-semibold uppercase tracking-[0.2em] text-slate-400">Made with</span>
                <TauriWordmark />
              </span>
            </a>
            <a href="https://mindmapvault.com/download/linux" className="inline-flex items-center gap-2 rounded-lg border border-slate-600 px-5 py-2.5 text-sm text-slate-300 transition hover:border-slate-500 hover:text-white">
              <LinuxMark className="h-4 w-4" />
              <span>Download for Linux</span>
            </a>
          </div>
          <div className="mt-4 flex flex-wrap gap-4 text-sm text-slate-300">
            <button
              type="button"
              onClick={() => setWhitepaperOpen(true)}
              className="underline decoration-slate-500 underline-offset-4 hover:text-white"
            >
              Read security whitepaper
            </button>
            <a href="https://github.com/cryptmind/app" target="_blank" rel="noreferrer" className="underline decoration-slate-500 underline-offset-4 hover:text-white">
              Inspect source on GitHub
            </a>
            <a href="mailto:admin@mindmapvault.com" className="underline decoration-slate-500 underline-offset-4 hover:text-white">
              Email admin@mindmapvault.com
            </a>
          </div>
          {/* Hero illustration — vault-mindmap v10 */}
          <div className="mt-8 flex justify-center">
            <img
              src="/vault-mindmap-hero.svg"
              alt="MindMapVault — encrypted vault with security mind map"
              className="w-full max-w-3xl"
              draggable={false}
            />
          </div>

          <div className="mt-5 rounded-xl border border-amber-500/50 bg-amber-500/10 p-4">
            <p className="text-sm text-slate-700 dark:text-slate-200">
              Enjoying MindMapVault? Support development:
              <a
                href="https://buymeacoffee.com/kornelko"
                target="_blank"
                rel="noreferrer"
                className="ml-2 inline-flex items-center gap-1 rounded-md border border-amber-500/70 bg-amber-50 px-2.5 py-1 text-xs font-semibold text-amber-700 transition hover:border-amber-500 hover:bg-amber-100 dark:border-amber-500/50 dark:bg-transparent dark:text-amber-300 dark:hover:border-amber-400 dark:hover:text-amber-200"
              >
                ☕ Buy me a coffee
              </a>
            </p>
          </div>
        </section>

        <section className="mt-8 rounded-2xl border border-slate-700 bg-surface-1 p-6">
          <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
            <div>
              <h2 className="text-xl font-semibold text-white">Questions or support</h2>
              <p className="mt-2 text-sm text-slate-400">
                If you would rather write directly than use the website forms, email
                {' '}
                <a href="mailto:admin@mindmapvault.com" className="text-white underline decoration-slate-500 underline-offset-4 hover:text-accent">
                  admin@mindmapvault.com
                </a>
                .
              </p>
            </div>
            <a href="mailto:admin@mindmapvault.com" className="rounded-lg border border-slate-600 px-5 py-2.5 text-sm text-slate-300 transition hover:border-slate-500 hover:text-white">
              Contact admin
            </a>
          </div>
        </section>

        <section className="mt-8 rounded-2xl border border-slate-700 bg-surface-1 p-6">
          <h2 className="text-xl font-semibold text-white">Security highlights</h2>
          <div className="mt-4 grid gap-4 md:grid-cols-2">
            <article className="rounded-xl border border-slate-700 bg-surface p-4">
              <h3 className="text-sm font-semibold text-white">Zero-knowledge by design</h3>
              <p className="mt-2 text-sm text-slate-400">
                Your password stays on your device. Vault content and titles are encrypted client-side before upload.
              </p>
            </article>
            <article className="rounded-xl border border-slate-700 bg-surface p-4">
              <h3 className="text-sm font-semibold text-white">Hybrid post-quantum encryption</h3>
              <p className="mt-2 text-sm text-slate-400">
                MindMapVault combines X25519 and ML-KEM-768 with HKDF, then encrypts data with AES-256-GCM.
              </p>
            </article>
            <article className="rounded-xl border border-slate-700 bg-surface p-4">
              <h3 className="text-sm font-semibold text-white">Per-save key isolation</h3>
              <p className="mt-2 text-sm text-slate-400">
                Each save uses a fresh DEK and ephemeral key material, reducing blast radius if any single artifact leaks.
              </p>
            </article>
            <article className="rounded-xl border border-slate-700 bg-surface p-4">
              <h3 className="text-sm font-semibold text-white">Short-lived data access</h3>
              <p className="mt-2 text-sm text-slate-400">
                Cloud blobs are fetched through time-limited presigned URLs, not permanently public links.
              </p>
            </article>
          </div>
        </section>

        <section className="mt-8 rounded-2xl border border-slate-700 bg-surface-1 p-6">
          <h2 className="text-xl font-semibold text-white">Plans and pricing</h2>
          <p className="mt-2 text-sm text-slate-400">Current settings: free cloud 10 MiB, paid cloud 100 MiB, offline desktop always free.</p>
          <div className="mt-4 grid gap-4 md:grid-cols-2 xl:grid-cols-4">
            <article className="rounded-xl border border-slate-700 bg-surface p-5">
              <h3 className="text-base font-semibold text-white">Cloud Free Forever</h3>
              <p className="mt-1 text-lg font-bold text-white">$0/year</p>
              <p className="mt-2 text-sm text-slate-400">
                Always-available cloud account with up to 10 MiB encrypted storage and cross-device access.
              </p>
            </article>
            <article className="rounded-xl border border-slate-700 bg-surface p-5">
              <h3 className="text-base font-semibold text-white">Offline Desktop (Local)</h3>
              <p className="mt-1 text-lg font-bold text-white">Free</p>
              <p className="mt-2 text-sm text-slate-400">
                Windows and Linux desktop apps are free for local use, with offline vaults stored on your machine.
              </p>
            </article>
            <article className="rounded-xl border border-accent bg-surface p-5">
              <h3 className="text-base font-semibold text-white">Cloud Pro</h3>
              <p className="mt-1 text-lg font-bold text-white">$10/year</p>
              <p className="mt-2 text-sm text-slate-400">
                Keep the current setup and increase encrypted cloud storage to 100 MiB with the same zero-knowledge model.
              </p>
            </article>
            <article className="rounded-xl border border-slate-700 bg-surface p-5">
              <div className="flex items-center justify-between gap-2">
                <h3 className="text-base font-semibold text-white">Lifetime License (Optional)</h3>
                <span className="rounded-full border border-slate-600 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-slate-300">
                  Coming soon
                </span>
              </div>
              <p className="mt-1 text-lg font-bold text-white">$40–60 one-time</p>
              <p className="mt-2 text-sm text-slate-400">
                One-time lifetime license for users who hate subscriptions.
              </p>
            </article>
          </div>
        </section>

        <section className="mt-8 rounded-2xl border border-slate-700 bg-surface-1 p-6">
          <div className="flex flex-col gap-3 md:flex-row md:items-end md:justify-between">
            <div>
              <p className="text-xs font-semibold uppercase tracking-[0.22em] text-slate-400">Changelog</p>
              <h2 className="mt-2 text-2xl font-semibold text-white">What changed recently, in customer terms.</h2>
            </div>
            <p className="max-w-2xl text-sm leading-6 text-slate-400">
              This page keeps the useful version story visible without burying people in internal implementation details.
            </p>
          </div>

          <div className="mt-5 grid gap-4 lg:grid-cols-[minmax(0,1.1fr)_minmax(280px,0.9fr)]">
            <div className="grid gap-4">
              {customerChangelog.map((entry) => (
                <article key={entry.version} className="rounded-[1.5rem] border border-slate-700 bg-surface p-5">
                  <div className="flex flex-wrap items-center gap-3">
                    <span className="rounded-full border border-accent/40 bg-accent/10 px-3 py-1 text-[11px] font-semibold uppercase tracking-[0.18em] text-violet-200">
                      v{entry.version}
                    </span>
                    <span className="text-xs font-semibold uppercase tracking-[0.18em] text-slate-500">{entry.date}</span>
                  </div>
                  <h3 className="mt-4 text-lg font-semibold text-white">{entry.title}</h3>
                  <p className="mt-2 text-sm leading-6 text-slate-300">{entry.summary}</p>
                  <div className="mt-4 grid gap-2">
                    {entry.changes.map((change) => (
                      <div key={change} className="flex items-start gap-3 text-sm text-slate-400">
                        <span className="mt-2 h-2 w-2 rounded-full bg-accent" />
                        <span>{change}</span>
                      </div>
                    ))}
                  </div>
                </article>
              ))}
            </div>

            <aside className="rounded-[1.5rem] border border-slate-700 bg-surface p-5">
              <p className="text-xs font-semibold uppercase tracking-[0.22em] text-slate-400">Next up</p>
              <h3 className="mt-2 text-xl font-semibold text-white">Planned improvements</h3>
              <p className="mt-2 text-sm leading-6 text-slate-400">
                These are the near-term improvements currently in focus, not long-range promises.
              </p>
              <div className="mt-4 grid gap-3">
                {upcomingChanges.map((item) => (
                  <div key={item} className="rounded-2xl border border-slate-700 bg-surface-1 px-4 py-4 text-sm leading-6 text-slate-300">
                    {item}
                  </div>
                ))}
              </div>
              <div className="mt-5 rounded-2xl border border-slate-700 bg-surface-1 px-4 py-4">
                <p className="text-xs font-semibold uppercase tracking-[0.18em] text-slate-500">Visible version</p>
                <p className="mt-2 text-lg font-semibold text-white">Project v{currentReleaseVersion}</p>
                <p className="mt-1 text-sm text-slate-400">App UI package v{uiVersion}</p>
              </div>
            </aside>
          </div>
        </section>
      </main>

      <footer className="border-t border-slate-800/80 bg-surface-1/80">
        <div className="mx-auto flex w-full max-w-6xl flex-col gap-2 px-4 py-5 text-sm text-slate-400 sm:flex-row sm:items-center sm:justify-between sm:px-6">
          <span>MindMapVault app landing</span>
          <span>Project v{currentReleaseVersion} · App UI package v{uiVersion}</span>
        </div>
      </footer>

      {whitepaperOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4" onClick={() => setWhitepaperOpen(false)}>
          <div
            className="max-h-[90vh] w-full max-w-4xl overflow-hidden rounded-2xl border border-slate-700 bg-surface-1 shadow-2xl"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center justify-between border-b border-slate-700 px-5 py-3">
              <h2 className="text-base font-semibold text-white">MindMapVault Security Whitepaper</h2>
              <button
                type="button"
                onClick={() => setWhitepaperOpen(false)}
                className="rounded-md border border-slate-600 px-2.5 py-1 text-sm text-slate-300 hover:border-slate-500 hover:text-white"
              >
                Close
              </button>
            </div>

            <div className="max-h-[calc(90vh-56px)] overflow-y-auto px-5 py-4" style={{ color: 'var(--text-primary)' }}>
              {whitepaperLoading && <p className="text-sm" style={{ color: 'var(--text-secondary)' }}>Loading whitepaper…</p>}
              {whitepaperError && <p className="text-sm text-red-400">{whitepaperError}</p>}
              {!whitepaperLoading && !whitepaperError && (
                <article
                  className="whitepaper-md text-sm leading-6"
                  dangerouslySetInnerHTML={{ __html: whitepaperHtml }}
                />
              )}
              <div className="mt-4 border-t border-slate-700 pt-3 text-xs" style={{ color: 'var(--text-secondary)' }}>
                Prefer raw file view? <a href="/SECURITY.md" target="_blank" rel="noreferrer" className="text-accent underline">Open SECURITY.md</a>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}