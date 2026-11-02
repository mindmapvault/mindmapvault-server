// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from 'vitest';
import { act, createElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { VaultCard, type VaultCardProps } from '../VaultCard';
import type { MapWithTitle } from '../types';

(globalThis as unknown as { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let container: HTMLDivElement | null = null;

const map = {
  id: 'v1',
  title: 'Home & garden',
  updated_at: '2026-10-27T10:00:00Z',
  vault_color: '#f59e0b',
  max_versions: 50,
  vault_labels: [],
  vaultNote: '',
  draftNote: '',
  draftLabels: [],
  draftColor: '#f59e0b',
  draftMaxVersions: 50,
  metaSaving: false,
} as unknown as MapWithTitle;

function render(overrides: Partial<VaultCardProps> = {}) {
  const props: VaultCardProps = {
    map,
    isLocalMode: false,
    renamingId: null,
    renameValue: '',
    renaming: false,
    userLabels: [],
    activeShareCount: 0,
    previewPanelStyle: {},
    previewOverlayStyle: {},
    previewOverlayBadgeStyle: {},
    onNavigate: vi.fn(),
    onStartRename: vi.fn(),
    onRenameValueChange: vi.fn(),
    onRenameConfirm: vi.fn(async () => {}),
    onRenameCancel: vi.fn(),
    onOpenHistory: vi.fn(),
    onOpenShares: vi.fn(),
    onDeleteRequest: vi.fn(),
    onSetDraftColor: vi.fn(),
    onSetDraftNote: vi.fn(),
    onSetDraftLabels: vi.fn(),
    onSetDraftMaxVersions: vi.fn(),
    onUpdateUserLabelColor: vi.fn(),
    onAddUserLabel: vi.fn(),
    onSaveMeta: vi.fn(async () => {}),
    ...overrides,
  };
  container = document.createElement('div');
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => { root!.render(createElement(VaultCard, props)); });
  return { el: container, props };
}

const buttonByText = (el: HTMLElement, text: string) =>
  [...el.querySelectorAll('button')].find((b) => b.textContent?.trim().startsWith(text));

afterEach(() => {
  act(() => { root?.unmount(); });
  container?.remove();
  root = null;
  container = null;
});

describe('VaultCard on a phone', () => {
  it('keeps the settings folded away until Details is tapped', () => {
    const { el } = render({ compact: true });
    expect(el.textContent).not.toContain('Vault note');
    act(() => { buttonByText(el, 'Details')!.click(); });
    expect(el.textContent).toContain('Vault note');
    expect(el.textContent).toContain('Max versions kept');
  });

  it('puts the vault actions behind one menu', () => {
    const { el, props } = render({ compact: true });
    expect(el.querySelector('[title="Delete vault"]')).toBeNull();
    act(() => { el.querySelector<HTMLButtonElement>('[aria-label="Vault actions"]')!.click(); });
    act(() => { buttonByText(el, 'Delete vault')!.click(); });
    expect(props.onDeleteRequest).toHaveBeenCalledWith('v1', 'Home & garden');
    expect(el.querySelector('[role="menu"]')).toBeNull();
  });

  it('offers a Save button once a setting changes', () => {
    const { el, props } = render({ compact: true, map: { ...map, draftNote: 'changed' } });
    expect(el.textContent).toContain('Unsaved');
    act(() => { buttonByText(el, 'Details')!.click(); });
    act(() => { buttonByText(el, 'Save')!.click(); });
    expect(props.onSaveMeta).toHaveBeenCalled();
  });
});

describe('VaultCard on a wider screen', () => {
  it('shows the settings and the action icons as before', () => {
    const { el } = render();
    expect(el.textContent).toContain('Vault note');
    expect(el.querySelector('[title="Delete vault"]')).not.toBeNull();
    expect(el.querySelector('[aria-label="Vault actions"]')).toBeNull();
  });
});
