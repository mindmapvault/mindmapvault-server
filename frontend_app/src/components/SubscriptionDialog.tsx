import { useEffect } from 'react';

export function SubscriptionDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  useEffect(() => {
    if (!open) return;
  }, [open]);

  if (!open) return null;

  return (
    <div className="subscription-dialog fixed inset-0 z-50 flex items-center justify-center bg-slate-950/80 px-4 backdrop-blur-sm" onClick={onClose}>
      <div
        className="subscription-dialog__panel w-full max-w-xl overflow-hidden rounded-3xl border border-slate-700/70 bg-slate-950 shadow-[0_24px_120px_rgba(15,23,42,0.55)]"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="subscription-dialog__hero border-b border-slate-800 bg-[radial-gradient(circle_at_top_left,_rgba(99,102,241,0.2),_transparent_38%),linear-gradient(135deg,_rgba(15,23,42,0.96),_rgba(2,6,23,0.98))] px-6 py-6">
          <div className="flex items-start justify-between gap-4">
            <div>
              <div className="subscription-dialog__pill inline-flex rounded-full border border-emerald-400/30 bg-emerald-400/10 px-3 py-1 text-xs font-semibold uppercase tracking-[0.2em] text-emerald-200">
                Community server
              </div>
              <h2 className="mt-4 text-2xl font-semibold text-white">Subscription</h2>
              <p className="mt-2 max-w-md text-sm leading-6 text-slate-300">
                Hosted billing and paid plan actions are disabled in this community server build.
              </p>
            </div>
            <button onClick={onClose} className="subscription-dialog__close rounded-full border border-slate-700 bg-slate-900/70 px-4 py-2 text-sm text-slate-300 transition hover:border-slate-500 hover:text-white">Close</button>
          </div>
        </div>

        <div className="subscription-dialog__body bg-[linear-gradient(180deg,_rgba(15,23,42,0.98),_rgba(2,6,23,1))] px-6 py-6">
          <div className="subscription-dialog__section rounded-2xl border border-slate-800 bg-slate-900/55 p-5">
            <p className="text-sm leading-6 text-slate-300">
              Hosted subscription and payment flows have been moved to the dedicated SaaS repository.
            </p>
            <p className="mt-3 text-sm leading-6 text-slate-300">
              This community server repository keeps sync/share platform functionality without hosted commercial integrations.
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}

export default SubscriptionDialog;
