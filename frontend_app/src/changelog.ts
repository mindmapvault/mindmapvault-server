/**
 * Single source of truth for the in-app changelog / "What's New".
 *
 * Changelog hygiene (keep these in sync on every release):
 *   1. Bump `version` in package.json.
 *   2. Set `APP_VERSION` below to the same value.
 *   3. Prepend a new entry to CHANGELOG with that version + today's date.
 *   4. Use the categories: 'feature' | 'improvement' | 'fix'. Keep lines short
 *      and user-facing (what changed for the user, not the implementation).
 *
 * The newest version must be the first array element, and `APP_VERSION` must
 * equal `CHANGELOG[0].version`.
 *
 * This is the operator-facing summary, not the whole of CHANGELOG.md — that
 * file carries the deployment detail (migrations, secrets to rotate, renamed
 * columns) which belongs in release notes rather than in a dialog.
 */

export const APP_VERSION = '0.5.3';

/**
 * localStorage key recording the last version whose "What's New" the user saw.
 * Reserved so the popup can track APP_VERSION without hardcoding it twice;
 * nothing opens the tab automatically yet.
 */
export const WHATS_NEW_SEEN_KEY = 'mindmapvault-whats-new-seen';

export type ChangeKind = 'feature' | 'improvement' | 'fix';

export interface ChangeItem {
  kind: ChangeKind;
  title: string;
  desc?: string;
}

export interface ChangelogEntry {
  version: string;
  date: string; // ISO yyyy-mm-dd
  highlights?: string;
  items: ChangeItem[];
}

export const CHANGELOG: ChangelogEntry[] = [
  {
    version: '0.5.3',
    date: '2026-09-07',
    highlights: 'A canvas colour the whole editor follows, the Lean toolbar put back together, and this tab.',
    items: [
      {
        kind: 'feature',
        title: 'This tab',
        desc: 'Settings now has a What\u2019s New page, so a release explains itself without going to the repository.',
      },
      {
        kind: 'feature',
        title: 'Pick your own canvas background',
        desc: 'Settings \u2192 Appearance. The nodes, toolbar and panels take their colour from it too, so the editor stays of a piece \u2014 and a pale background gets dark text whichever mode you are in. "Match theme" puts it back.',
      },
      {
        kind: 'feature',
        title: 'A URL button in the toolbar',
        desc: 'The Insert tab is grouped Content, Links and Files, with the vault link and the web link side by side.',
      },
      {
        kind: 'fix',
        title: 'The Lean toolbar was empty',
        desc: 'Undo, redo, add child, add sibling, delete, notes and search are back in the row, with everything else behind More \u2014 and the settings gear no longer disappears with them.',
      },
      {
        kind: 'fix',
        title: 'Theme and settings vanished at Large density',
        desc: 'Both were rendered only at the smaller densities. They now sit in the top row, whichever ribbon tab is open.',
      },
      {
        kind: 'fix',
        title: 'Checkboxes in a note can be ticked while reading',
        desc: 'They were drawn but did nothing outside the editor. A plain "[x]" on its own line now counts as a checkbox too, not just the list form.',
      },
      {
        kind: 'improvement',
        title: 'Help points at the repository',
        desc: 'Issues for defects, Discussions for questions \u2014 both answered in the open, where the next person with the same problem can find them.',
      },
    ],
  },
  {
    version: '0.5.2',
    date: '2026-09-05',
    highlights: 'A security fix in the guided installer, links between vaults, and four bug fixes.',
    items: [
      {
        kind: 'fix',
        title: 'The installer could leave an instance on published secrets',
        desc: 'Accepting the defaults during install kept placeholder JWT and admin-token values that anyone can read in the public repository. Placeholders now count as absent, and the server refuses to start on one. Check .env.deploy for values starting "replace_with_" and rotate them.',
      },
      {
        kind: 'feature',
        title: 'Link a node to another vault',
        desc: 'Right-click → Link to Vault…, the Link button in the toolbar, or Ctrl+K. The node draws a strip naming the target, and clicking it opens that vault.',
      },
      {
        kind: 'fix',
        title: 'Edits made while a file uploads are no longer discarded',
        desc: 'Six paths rebuilt the map from a copy taken before the upload began, losing anything changed in between.',
      },
      {
        kind: 'fix',
        title: 'An update no longer discards local Compose edits',
        desc: 'A modified docker-compose.yml or garage.toml is backed up to .bak before it is replaced.',
      },
      {
        kind: 'fix',
        title: 'Smaller fixes',
        desc: 'A node with a vault link is no longer 18px too tall; an export with a blank title is no longer named ".md"; one corrupt local label no longer empties the whole vault list.',
      },
      {
        kind: 'improvement',
        title: 'Groundwork',
        desc: 'Node measurement, layout, tree operations, history and drag geometry moved into shared packages, with 136 frontend tests where there were none. No behaviour change intended — the fixes above are the drift this exposed.',
      },
    ],
  },
  {
    version: '0.5.1',
    date: '2026-09-03',
    highlights: 'A reworked toolbar with three densities, and a fix for version history on stores without S3 versioning.',
    items: [
      {
        kind: 'feature',
        title: 'Three interface densities',
        desc: 'Lean strips the chrome back to the canvas, Standard is the familiar layout, and Large captions every button under a Home / Insert / View / Export ribbon. Each preset can be overridden piece by piece.',
      },
      {
        kind: 'feature',
        title: 'Dockable colour and icon trays',
        desc: 'Either tray can sit against any edge of the canvas.',
      },
      {
        kind: 'feature',
        title: 'Two keyboard layouts',
        desc: 'FreeMind or Mac. Mod resolves to the current OS either way, so a Mac user on the FreeMind layout still gets ⌘. Hints can be shown on the buttons themselves.',
      },
      {
        kind: 'fix',
        title: 'Version history was broken on any store without object versioning',
        desc: 'Garage — the store this repo’s own docker-compose.yml ships — accepted a version id on upload and ignored it on download, so loading an older version appeared to do nothing. Every save now writes its own object. Existing vaults are migrated at startup.',
      },
      {
        kind: 'fix',
        title: 'Every version showed 0 B',
        desc: 'Sizes are recorded when a version is written, so the list no longer asks the object store for them.',
      },
      {
        kind: 'fix',
        title: 'Menus could open off-screen',
        desc: 'The export dropdown, node context menu and shortcut panel are measured and moved to fit; a context menu near the bottom opens upward.',
      },
      {
        kind: 'improvement',
        title: 'A storage self-test at startup',
        desc: 'Writes, reads back, compares and deletes a probe object. A store that accepts a write and serves something else is otherwise indistinguishable from a working one until a user loses data.',
      },
    ],
  },
  {
    version: '0.5.0',
    date: '2026-09-02',
    highlights: 'Pictures on the nodes themselves, and two fixes for bugs that could lose work.',
    items: [
      {
        kind: 'feature',
        title: 'Pictures on the nodes',
        desc: 'Add one from the context menu, by dropping an image on a node, with Ctrl+V, or with Alt+K. The thumbnail lives inside the map, so it inherits the vault’s encryption and shows up in previews and shared links with nothing fetched. PNG and PDF exports include them.',
      },
      {
        kind: 'fix',
        title: 'Duplicating a node shared its files with the original',
        desc: 'Both copies pointed at one stored file, so deleting either one’s file silently broke the other. A duplicate now gets its own copy of every file.',
      },
      {
        kind: 'fix',
        title: 'An attachment upload discarded unsaved edits',
        desc: 'Refreshed attachment references were fed back as a document to load, resetting the working tree and its history — including the change being made at that moment.',
      },
      {
        kind: 'improvement',
        title: 'A reveal toggle on the password fields',
        desc: 'This password is the encryption key and nobody can reset it, so checking what was typed is the cheapest guard against locking yourself out.',
      },
    ],
  },
];
