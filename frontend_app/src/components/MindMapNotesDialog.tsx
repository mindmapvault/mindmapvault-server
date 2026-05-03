import { useState } from 'react';
import type { ClipboardEvent, DragEvent, RefObject } from 'react';
import type { NodeAttachmentRef } from '../types';
import DynamicLucideIcon from './DynamicLucideIcon';
import type { NodeAttachmentViewerAsset } from './MindMapEditor.types';

interface MindMapNotesDialogProps {
  open: boolean;
  notesDropActive: boolean;
  nodeTitle: string;
  hasNodeNotes: boolean;
  nodeIcons: string[];
  nodeTags: Array<{ name: string; color: string }>;
  attachmentCount: number;
  attachmentLabel: string;
  attachments: NodeAttachmentRef[];
  attachmentPreviewUrls: Record<string, string>;
  canDeleteAttachment: boolean;
  showMarkdownHelp: boolean;
  notesUploadBusy: boolean;
  notesText: string;
  notesPreviewHtml: string;
  notesRef: RefObject<HTMLTextAreaElement>;
  notesImageInputRef: RefObject<HTMLInputElement>;
  onClose: () => void;
  onDragOver: (e: DragEvent<HTMLDivElement>) => void;
  onDragLeave: (e: DragEvent<HTMLDivElement>) => void;
  onDrop: (e: DragEvent<HTMLDivElement>) => void;
  onOpenAttachment?: (attachment: NodeAttachmentRef) => void;
  onDeleteAttachment?: (attachment: NodeAttachmentRef) => void;
  onLoadAttachmentViewer?: (attachment: NodeAttachmentRef) => Promise<NodeAttachmentViewerAsset | null>;
  onAddPictureFiles: (files: File[]) => void;
  onInsertMarkdownAction: (action: 'h1' | 'h2' | 'h3' | 'bold' | 'italic' | 'ul' | 'ol' | 'task' | 'quote' | 'code' | 'link') => void;
  onToggleMarkdownHelp: () => void;
  onNotesTextChange: (value: string) => void;
  onNotesPaste: (e: ClipboardEvent<HTMLTextAreaElement>) => void;
  onSaveNotes: () => void;
  onDeleteNotes: () => void;
}

export function MindMapNotesDialog({
  open,
  notesDropActive,
  nodeTitle,
  hasNodeNotes,
  nodeIcons,
  nodeTags,
  attachmentCount,
  attachmentLabel,
  attachments,
  attachmentPreviewUrls,
  canDeleteAttachment,
  showMarkdownHelp,
  notesUploadBusy,
  notesText,
  notesPreviewHtml,
  notesRef,
  notesImageInputRef,
  onClose,
  onDragOver,
  onDragLeave,
  onDrop,
  onOpenAttachment,
  onDeleteAttachment,
  onLoadAttachmentViewer,
  onAddPictureFiles,
  onInsertMarkdownAction,
  onToggleMarkdownHelp,
  onNotesTextChange,
  onNotesPaste,
  onSaveNotes,
  onDeleteNotes,
}: MindMapNotesDialogProps) {
  const [attachmentViewer, setAttachmentViewer] = useState<NodeAttachmentViewerAsset | null>(null);
  const [viewerBusyAttachmentId, setViewerBusyAttachmentId] = useState<string | null>(null);

  if (!open) return null;

  const canPreviewAttachment = (attachment: NodeAttachmentRef) => {
    return attachment.content_type.startsWith('image/') || attachment.content_type === 'application/pdf';
  };

  const openAttachmentViewer = async (attachment: NodeAttachmentRef, fallbackUrl?: string) => {
    if (!canPreviewAttachment(attachment)) {
      onOpenAttachment?.(attachment);
      return;
    }

    if (attachment.content_type.startsWith('image/') && fallbackUrl) {
      setAttachmentViewer({
        url: fallbackUrl,
        contentType: attachment.content_type,
        name: attachment.name,
      });
      return;
    }

    if (!onLoadAttachmentViewer) {
      onOpenAttachment?.(attachment);
      return;
    }

    setViewerBusyAttachmentId(attachment.attachment_id);
    try {
      const viewerAsset = await onLoadAttachmentViewer(attachment);
      if (viewerAsset) {
        setAttachmentViewer(viewerAsset);
      } else {
        onOpenAttachment?.(attachment);
      }
    } finally {
      setViewerBusyAttachmentId(null);
    }
  };

  const handlePreviewClick = (event: React.MouseEvent<HTMLDivElement>) => {
    const target = event.target;
    if (!(target instanceof HTMLElement)) return;

    const image = target.closest('img.mm-notes-inline-image');
    if (image instanceof HTMLImageElement) {
      event.preventDefault();
      event.stopPropagation();
      setAttachmentViewer({
        url: image.currentSrc || image.src,
        contentType: 'image/*',
        name: image.alt || 'Attachment preview',
      });
      return;
    }

    const anchor = target.closest('a[href^="attachment://"]');
    if (!(anchor instanceof HTMLAnchorElement)) return;

    event.preventDefault();
    event.stopPropagation();

    const attachmentId = (anchor.getAttribute('href') ?? '').replace(/^attachment:\/\//, '');
    if (!attachmentId) return;

    const attachment = attachments.find((item) => item.attachment_id === attachmentId);
    if (attachment) {
      void openAttachmentViewer(attachment, attachmentPreviewUrls[attachment.attachment_id]);
    }
  };

  return (
    <>
      <div className="mm-overlay" onClick={onClose} />
      <div
        className={`mm-notes-modal${notesDropActive ? ' mm-notes-panel--drop' : ''}`}
        onDragOver={onDragOver}
        onDragLeave={onDragLeave}
        onDrop={onDrop}
      >
        <div className="mm-notes-header"><span>Notes — {nodeTitle}</span>
          <div style={{ display: 'flex', gap: 4, alignItems: 'center' }}>
            <button className="mm-btn-icon" onClick={onClose} title="Close"><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2.5} strokeLinecap="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg></button>
          </div>
        </div>
        <div className="mm-notes-node-preview">
          <div className="mm-notes-node-preview-row">
            <span className="mm-notes-node-preview-title">Node preview</span>
            <div className="mm-notes-node-preview-flags">
              {hasNodeNotes && <span className="mm-notes-node-flag">Note</span>}
              {attachmentCount > 0 && <span className="mm-notes-node-flag">{attachmentLabel}</span>}
            </div>
          </div>
          <div className="mm-notes-node-preview-row" style={{ gap: 6 }}>
            {nodeIcons.length > 0 ? (
              nodeIcons.map((iconName, idx) => (
                <span key={`${iconName}-${idx}`} className="mm-notes-node-icon">
                  <DynamicLucideIcon name={iconName} size={14} />
                </span>
              ))
            ) : (
              <span className="mm-notes-node-preview-muted">No icons</span>
            )}
          </div>
          <div className="mm-notes-node-preview-row" style={{ flexWrap: 'wrap' }}>
            {nodeTags.length > 0 ? (
              nodeTags.map((tag) => (
                <span key={tag.name} className="mm-notes-node-tag" style={{ background: tag.color }}>{tag.name}</span>
              ))
            ) : (
              <span className="mm-notes-node-preview-muted">No labels</span>
            )}
          </div>
        </div>
        {attachments.length > 0 && (
          <div className="mm-notes-attachments-wrap">
            <div className="mm-notes-attachments-header">
              <span>Files on this node</span>
              <span>{attachmentLabel}</span>
            </div>
            <div className="mm-notes-attachments">
              {attachments.map((attachment) => {
                const previewUrl = attachmentPreviewUrls[attachment.attachment_id];
                return (
                  <div key={attachment.attachment_id} className="mm-notes-attachment-card">
                    <button
                      type="button"
                      className="mm-notes-attachment-open"
                      onClick={() => {
                        if (attachment.content_type === 'application/pdf') {
                          void openAttachmentViewer(attachment);
                          return;
                        }
                        onOpenAttachment?.(attachment);
                      }}
                    >
                      <div
                        className={`mm-notes-attachment-thumb${previewUrl ? ' is-previewable' : ''}`}
                        onClick={(event) => {
                          if (!previewUrl) return;
                          event.preventDefault();
                          event.stopPropagation();
                          void openAttachmentViewer(attachment, previewUrl);
                        }}
                        title={previewUrl ? 'Open preview' : undefined}
                      >
                        {previewUrl ? (
                          <img src={previewUrl} alt={attachment.name} className="mm-notes-attachment-image" />
                        ) : (
                          <span>{attachment.preview_kind === 'image' ? 'IMG' : 'FILE'}</span>
                        )}
                      </div>
                      <div className="mm-notes-attachment-meta">
                        <strong>{attachment.name}</strong>
                        <span>{Math.max(1, Math.round(attachment.size_bytes / 1024))} KB{viewerBusyAttachmentId === attachment.attachment_id ? ' · Opening preview…' : ''}</span>
                      </div>
                    </button>
                    {canDeleteAttachment && (
                      <button
                        type="button"
                        className="mm-notes-attachment-delete"
                        title="Delete attachment"
                        onClick={() => onDeleteAttachment?.(attachment)}
                      >
                        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round">
                          <polyline points="3,6 5,6 21,6"/>
                          <path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/>
                          <path d="M10 11v6M14 11v6"/>
                          <path d="M9 6V4h6v2"/>
                        </svg>
                      </button>
                    )}
                  </div>
                );
              })}
            </div>
          </div>
        )}

        <div className="mm-notes-markdown-tools">
          <button
            className="mm-notes-md-btn"
            onClick={() => notesImageInputRef.current?.click()}
            title="Add picture"
            type="button"
          >
            + Picture
          </button>
          <input
            ref={notesImageInputRef}
            type="file"
            accept="image/*"
            multiple
            style={{ display: 'none' }}
            onChange={(e) => {
              const files = Array.from(e.target.files ?? []);
              if (files.length > 0) {
                onAddPictureFiles(files);
              }
              e.currentTarget.value = '';
            }}
          />
          <button className="mm-notes-md-btn" onClick={() => onInsertMarkdownAction('h1')} title="Heading 1">H1</button>
          <button className="mm-notes-md-btn" onClick={() => onInsertMarkdownAction('h2')} title="Heading 2">H2</button>
          <button className="mm-notes-md-btn" onClick={() => onInsertMarkdownAction('h3')} title="Heading 3">H3</button>
          <button className="mm-notes-md-btn" onClick={() => onInsertMarkdownAction('bold')} title="Bold"><strong>B</strong></button>
          <button className="mm-notes-md-btn" onClick={() => onInsertMarkdownAction('italic')} title="Italic"><em>I</em></button>
          <button className="mm-notes-md-btn" onClick={() => onInsertMarkdownAction('ul')} title="Bulleted list">• List</button>
          <button className="mm-notes-md-btn" onClick={() => onInsertMarkdownAction('ol')} title="Numbered list">1. List</button>
          <button className="mm-notes-md-btn" onClick={() => onInsertMarkdownAction('task')} title="Checklist">☐ Task</button>
          <button className="mm-notes-md-btn" onClick={() => onInsertMarkdownAction('quote')} title="Quote">" Quote</button>
          <button className="mm-notes-md-btn" onClick={() => onInsertMarkdownAction('code')} title="Inline code">&lt;/&gt;</button>
          <button className="mm-notes-md-btn" onClick={() => onInsertMarkdownAction('link')} title="Link">Link</button>
          <button className="mm-notes-md-btn" onClick={onToggleMarkdownHelp} title="Markdown help">ⓘ</button>
          {notesUploadBusy && <span className="mm-notes-upload-hint">Uploading files…</span>}
        </div>
        {showMarkdownHelp && (
          <div className="mm-notes-md-help">
            <div><strong>#</strong> / <strong>##</strong> / <strong>###</strong> headings</div>
            <div><strong>**bold**</strong>, <em>*italic*</em>, <strong>`code`</strong></div>
            <div><strong>- item</strong> unordered, <strong>1. item</strong> ordered, <strong>- [ ] task</strong> checklist</div>
            <div><strong>&gt; quote</strong> block quote, <strong>[text](url)</strong> links</div>
            <div>Drop pictures anywhere in this dialog, use <strong>+ Picture</strong>, or paste with <strong>Ctrl+V</strong> to insert image markdown.</div>
          </div>
        )}
        <div className="mm-notes-split">
          <div className="mm-notes-editor-pane">
            <textarea ref={notesRef} className="mm-notes-textarea" value={notesText} onChange={(e) => onNotesTextChange(e.target.value)}
              onKeyDown={(e) => { if (e.key === 'Escape') { e.stopPropagation(); onClose(); } }}
              onPaste={onNotesPaste}
              placeholder="Add notes… (supports **Markdown**)"
              autoFocus
            />
          </div>
          <div className="mm-notes-preview-pane">
            <div className="mm-notes-preview" onClick={handlePreviewClick} dangerouslySetInnerHTML={{ __html: notesPreviewHtml }} />
          </div>
        </div>
        <div className="mm-notes-footer">
          <button className="mm-btn mm-btn--primary" onClick={onSaveNotes}>Save notes</button>
          <button className="mm-btn" onClick={onClose}>Cancel</button>
          <button className="mm-btn mm-btn--danger" style={{ marginLeft: 'auto' }} onClick={onDeleteNotes}>Delete note</button>
        </div>
      </div>
      {attachmentViewer && (
        <>
          <div className="mm-overlay mm-image-preview-overlay" onClick={() => setAttachmentViewer(null)} />
          <div className="mm-image-preview-dialog" role="dialog" aria-modal="true" aria-label={attachmentViewer.name}>
            <div className="mm-image-preview-header">
              <strong>{attachmentViewer.name}</strong>
              <button className="mm-btn-icon" onClick={() => setAttachmentViewer(null)} title="Close preview">
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2.5} strokeLinecap="round"><line x1="18" y1="6" x2="6" y2="18"/><line x1="6" y1="6" x2="18" y2="18"/></svg>
              </button>
            </div>
            <div className="mm-image-preview-body">
              {attachmentViewer.contentType === 'application/pdf' ? (
                <iframe src={attachmentViewer.url} title={attachmentViewer.name} className="mm-image-preview-pdf" />
              ) : (
                <img src={attachmentViewer.url} alt={attachmentViewer.name} className="mm-image-preview-full" />
              )}
            </div>
          </div>
        </>
      )}
    </>
  );
}
