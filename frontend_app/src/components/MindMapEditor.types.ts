import type { MindMapTree, NodeAttachmentRef } from '../types';
import type { LinkableVault } from './MindMapVaultLinkDialog';

export interface NodeAttachmentViewerAsset {
  url: string;
  contentType: string;
  name: string;
}

export type MindMapEditorMode = 'desktop' | 'mobile';
export type MindMapEditorModePreference = 'auto' | MindMapEditorMode;

export interface MindMapEditorProps {
  initialTree: MindMapTree | null;
  initialShowShortcuts?: boolean;
  disableAutoPanToSelection?: boolean;
  externalNodeAttachments?: Record<string, NodeAttachmentRef[]>;
  title: string;
  onSave: (tree: MindMapTree, title: string) => Promise<void>;
  onTitleChange: (title: string) => void;
  saving: boolean;
  saveMsg: string;
  error: string;
  onBack?: () => void;
  onShowHistory?: () => void;
  onDownloadEncrypted?: (fileBaseName?: string) => void;
  onDownloadJson?: (tree: MindMapTree, title: string) => void;
  onExportMarkdown?: (tree: MindMapTree, title: string) => void;
  onExportFreemind?: (tree: MindMapTree, title: string) => void;
  onExportFreeplane?: (tree: MindMapTree, title: string) => void;
  onExportWisemapping?: (tree: MindMapTree, title: string) => void;
  onExportXmind?: (tree: MindMapTree, title: string) => void;
  titleChanged?: boolean;
  onRenameTitle?: () => void;
  renamingTitle?: boolean;
  versionLabel?: string;
  versionTooltip?: string;
  onTreeChange?: (tree: MindMapTree) => void;
  onSelectionChange?: (nodeId: string | null) => void;
  onOpenSecurePanel?: (tab: 'attachments' | 'shares') => void;
  onNodeFileDrop?: (nodeId: string, files: File[]) => Promise<NodeAttachmentRef[]>;
  onOpenNodeAttachment?: (attachment: NodeAttachmentRef) => Promise<void> | void;
  onFetchNodeAttachmentContent?: (attachment: NodeAttachmentRef) => Promise<{ name: string; contentType: string; blob: Blob } | null>;
  onDeleteNodeAttachment?: (attachment: NodeAttachmentRef) => Promise<void> | void;
  /**
   * Duplicates one attachment onto another node, returning the new reference.
   * Node duplication needs it so the copy owns its files rather than sharing
   * the original's — see `duplicateNode`.
   */
  onCopyNodeAttachment?: (attachment: NodeAttachmentRef, nodeId: string) => Promise<NodeAttachmentRef | null>;
  onLoadNodeAttachmentPreview?: (attachment: NodeAttachmentRef) => Promise<string | null>;
  onLoadNodeAttachmentViewer?: (attachment: NodeAttachmentRef) => Promise<NodeAttachmentViewerAsset | null>;
  /** This vault's own id, so it can be kept out of the link picker. */
  vaultId?: string;
  /** Vaults this map can link a node to, titles already decrypted. */
  linkableVaults?: LinkableVault[];
  linkableVaultsLoading?: boolean;
  /** Asked for when the picker opens, so the list is not fetched on every edit. */
  onRequestLinkableVaults?: () => void;
  /** Follows a node's vault link. Navigation belongs to the page. */
  onOpenVaultLink?: (vaultId: string) => void;
}