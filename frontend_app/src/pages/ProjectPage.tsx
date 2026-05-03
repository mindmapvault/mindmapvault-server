export function ProjectPage() {
  return (
    <div className="min-h-screen bg-surface-0">
      <header className="border-b border-slate-700 bg-surface-1 px-6 py-4">
        <div className="mx-auto max-w-6xl">Project Management</div>
      </header>

      <main className="mx-auto w-full max-w-6xl flex-1 px-4 py-8 sm:px-6">
        <div className="rounded-xl border border-slate-700 bg-surface-1 p-6">
          <h1 className="text-lg font-semibold text-white">Projects</h1>
          <p className="mt-2 text-sm text-slate-400">This is a placeholder for project management features.</p>
          <p className="mt-4 text-xs text-slate-400">You can add tasks, timelines and team collaboration here.</p>
        </div>
      </main>
    </div>
  );
}

export default ProjectPage;
