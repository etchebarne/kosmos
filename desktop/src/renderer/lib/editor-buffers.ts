import type { editor } from "monaco-editor";

import {
  changeEditorSession,
  closeEditorSession,
  getEditorDocument,
  openEditorSession,
  restoreEditorSession,
  syncEditorDocument,
} from "@/renderer/ipc";
import type { EditorDocument, EditorSave, EditorSaveWarning, EditorTextEdit } from "@/shared/ipc";

type LanguageDocumentHandle = { dispose(): void };
type LanguageDocumentAttacher = (
  workspaceId: number,
  tabId: number,
  path: string,
  model: editor.ITextModel,
) => LanguageDocumentHandle;
type EditorBufferLockState = {
  transactions: Map<number, number>;
  listeners: Set<(locked: boolean) => void>;
  operationEpoch: number;
};
type EditorSessionState = {
  lastError: unknown | null;
  opening: Promise<void> | null;
  /** Edits made after `syncedRevision` that the server has not received yet. */
  pendingEdits: EditorTextEdit[];
  /** Set when an edit cannot be expressed incrementally, e.g. a line-ending change. */
  needsFullSync: boolean;
  /** The newest local revision, including pending edits. */
  revision: number;
  /** The revision the server acknowledged; pending edits apply on top of it. */
  syncedRevision: number;
  savedGeneration: number;
  synchronization: Promise<void> | null;
};

let attachLanguageDocument: LanguageDocumentAttacher | null = null;
let sessionRecovery: Promise<void> | null = null;

export function setLanguageDocumentAttacher(attacher: LanguageDocumentAttacher): void {
  attachLanguageDocument = attacher;
}

export function initializeEditorBufferRecovery(): void {
  window.kosmos.onServerReconnected((generation) => {
    if (generation === 0) {
      return;
    }
    const recovery = restoreEditorBufferSessions();
    sessionRecovery = recovery;
    void recovery
      .then(() => {
        if (sessionRecovery === recovery) {
          sessionRecovery = null;
          resumeEditorBufferSynchronization();
        }
        window.kosmos.completeServerRecovery(generation);
      })
      .catch((error: unknown) => {
        window.kosmos.completeServerRecovery(
          generation,
          error instanceof Error ? error.message : String(error),
        );
      });
  });
}

const DETACHED_LANGUAGE_DOCUMENT: LanguageDocumentHandle = { dispose() {} };

function attachDocument(
  workspaceId: number,
  tabId: number,
  path: string,
  model: editor.ITextModel,
  languageFeatures = true,
): LanguageDocumentHandle {
  if (!languageFeatures) {
    return DETACHED_LANGUAGE_DOCUMENT;
  }
  if (!attachLanguageDocument) {
    throw new Error("Language document attachment is not initialized.");
  }
  return attachLanguageDocument(workspaceId, tabId, path, model);
}

export type EditorBuffer = {
  workspaceId: number;
  tabId: number;
  model: editor.ITextModel;
  path: string;
  /** Whether the buffer is attached to language servers (diff drafts are not). */
  languageFeatures: boolean;
  savedContent: string;
  /** Monaco's alternative version id at which the model equals `savedContent`. */
  savedVersionId: number | null;
  /** True while applying server-originated text that must not be echoed back. */
  applyingServerText: boolean;
  languageDocument: LanguageDocumentHandle;
  modelListeners: Set<(model: editor.ITextModel) => void>;
  lockState: EditorBufferLockState;
  session: EditorSessionState;
};

/** A tab's document path, with its text only when the tab's buffer lacks it. */
export type LoadedEditorDocument = {
  path: string;
  document: EditorDocument | null;
};

/** Loads a tab's document, skipping the text when an existing buffer is current. */
export async function loadEditorDocument(
  workspaceId: number,
  tabId: number,
): Promise<LoadedEditorDocument> {
  const buffer = editorBuffer(workspaceId, tabId);
  if (!buffer || buffer.model.isDisposed()) {
    const document = await getEditorDocument({ workspaceId, tabId });
    return { path: document.path, document };
  }
  const sync = await syncEditorDocument({
    workspaceId,
    tabId,
    knownRevision: buffer.session.syncedRevision,
    knownSavedGeneration: buffer.session.savedGeneration,
  });
  return { path: sync.path, document: sync.document ?? null };
}

export type EditorBufferState = {
  buffer: EditorBuffer;
  path: string;
  model: editor.ITextModel;
  version: number;
  content: string;
  savedContent: string;
};

const buffers = new Map<string, EditorBuffer>();

export function pathDerivedModelLanguage(): undefined {
  return undefined;
}

export function getOrCreateEditorBuffer(
  workspaceId: number,
  tabId: number,
  path: string,
  savedContent: string,
  createModel: () => editor.ITextModel,
  { languageFeatures = true }: { languageFeatures?: boolean } = {},
): EditorBuffer {
  const key = bufferKey(workspaceId, tabId);
  const existing = buffers.get(key);

  if (existing?.path === path && !existing.model.isDisposed()) {
    return existing;
  }

  const model = createModel();
  carryOverEditorBufferContent(existing, model);
  existing?.languageDocument.dispose();
  existing?.model.dispose();

  const buffer = {
    workspaceId,
    tabId,
    model,
    path,
    languageFeatures,
    savedContent,
    savedVersionId: null,
    applyingServerText: false,
    languageDocument: attachDocument(workspaceId, tabId, path, model, languageFeatures),
    modelListeners: new Set<(model: editor.ITextModel) => void>(),
    lockState: existing?.lockState ?? {
      transactions: new Map<number, number>(),
      listeners: new Set<(locked: boolean) => void>(),
      operationEpoch: 0,
    },
    session: existing?.session ?? {
      lastError: null,
      opening: null,
      pendingEdits: [],
      needsFullSync: false,
      revision: 0,
      syncedRevision: 0,
      savedGeneration: 0,
      synchronization: null,
    },
  };
  buffers.set(key, buffer);
  markEditorBufferSavedIfEqual(buffer);

  return buffer;
}

/**
 * Returns the tab's buffer for `loaded`, reusing the current one when the path is
 * unchanged so no text is copied, and otherwise creating one from the loaded text.
 */
export function editorBufferForDocument(
  workspaceId: number,
  tabId: number,
  loaded: LoadedEditorDocument,
  createModel: (content: string) => editor.ITextModel,
): EditorBuffer {
  const existing = editorBuffer(workspaceId, tabId);
  const current = existing && !existing.model.isDisposed() ? existing : null;
  const { document } = loaded;
  if (!document && !current) {
    throw new Error(`The document ${loaded.path} is not loaded.`);
  }
  const savedContent = document ? (document.savedContent ?? document.content) : current!.savedContent;
  return getOrCreateEditorBuffer(workspaceId, tabId, loaded.path, savedContent, () =>
    createModel(document?.content ?? current!.model.getValue()),
  );
}

/** Keeps unsynchronized edits when a tab's buffer moves to a renamed path. */
function carryOverEditorBufferContent(
  existing: EditorBuffer | undefined,
  model: editor.ITextModel,
): void {
  if (!existing || existing.model.isDisposed()) {
    return;
  }
  const content = existing.model.getValue();
  if (model.getValue() !== content) {
    model.setValue(content);
  }
}

/**
 * Attaches a buffer to its server session. A buffer built from the document the
 * server just returned is already in sync, so only buffers carrying local text the
 * server has not seen upload it.
 */
export function openEditorBufferSession(
  buffer: EditorBuffer,
  document: EditorDocument | null,
): Promise<void> {
  if (buffer.session.opening) {
    return buffer.session.opening;
  }
  if (document && !isEditorBufferModified(buffer, document)) {
    adoptEditorDocumentVersion(buffer, document);
    return Promise.resolve();
  }
  if (!document) {
    return Promise.resolve();
  }
  const opening = uploadEditorBufferText(buffer, document.revision);
  buffer.session.opening = opening;
  void opening
    .finally(() => {
      if (buffer.session.opening === opening) {
        buffer.session.opening = null;
      }
    })
    .catch(() => {});
  return opening;
}

/** Whether the model holds text the server's `document` does not contain. */
function isEditorBufferModified(buffer: EditorBuffer, document: EditorDocument): boolean {
  if (buffer.session.pendingEdits.length > 0 || buffer.session.needsFullSync) {
    return true;
  }
  if (buffer.session.syncedRevision === document.revision && buffer.session.revision === document.revision) {
    return false;
  }
  return buffer.model.getValue() !== document.content;
}

function adoptEditorDocumentVersion(buffer: EditorBuffer, document: EditorDocument): void {
  buffer.session.revision = Math.max(buffer.session.revision, document.revision);
  buffer.session.syncedRevision = document.revision;
  buffer.session.savedGeneration = document.savedGeneration;
}

/** Replaces the server session text with the model's, e.g. after a rejected edit. */
async function uploadEditorBufferText(buffer: EditorBuffer, serverRevision: number): Promise<void> {
  buffer.session.pendingEdits = [];
  buffer.session.needsFullSync = false;
  const revision = nextEditorBufferRevision(buffer, serverRevision);
  const ack = await openEditorSession({
    workspaceId: buffer.workspaceId,
    tabId: buffer.tabId,
    path: buffer.path,
    content: buffer.model.getValue(),
    revision,
  });
  if (!ack.accepted) {
    throw new Error(`Could not synchronize ${buffer.path}; the server has a newer version.`);
  }
  buffer.session.syncedRevision = revision;
  buffer.session.savedGeneration = ack.savedGeneration;
}

function nextEditorBufferRevision(buffer: EditorBuffer, floor = 0): number {
  buffer.session.revision = Math.max(
    buffer.session.revision + 1,
    floor + 1,
    buffer.model.getVersionId(),
  );
  return buffer.session.revision;
}

/**
 * Starts a server session holding the buffer's current text as its saved baseline;
 * used by views, such as diffs, whose text does not come from an editor document.
 */
export function startEditorBufferSession(buffer: EditorBuffer): Promise<void> {
  const opening = uploadEditorBufferText(buffer, buffer.session.syncedRevision);
  buffer.session.opening = opening;
  void opening
    .finally(() => {
      if (buffer.session.opening === opening) {
        buffer.session.opening = null;
      }
    })
    .catch(() => {});
  return opening;
}

/**
 * Makes `content` the buffer's new saved text, e.g. when a clean diff reloads after
 * the file changed; the server session restarts with it as its baseline.
 */
export async function resetEditorBufferSession(
  buffer: EditorBuffer,
  content: string,
): Promise<void> {
  buffer.applyingServerText = true;
  try {
    replaceEditorBufferContent(buffer, content);
  } finally {
    buffer.applyingServerText = false;
  }
  buffer.savedContent = content;
  buffer.session.pendingEdits = [];
  buffer.session.needsFullSync = false;
  buffer.savedVersionId = buffer.model.getAlternativeVersionId();
  await closeEditorSession({ workspaceId: buffer.workspaceId, tabId: buffer.tabId });
  await startEditorBufferSession(buffer);
}

/** Discards a tab's buffer and server session, including unsaved edits. */
export async function closeEditorBufferSession(workspaceId: number, tabId: number): Promise<void> {
  disposeEditorBuffer(workspaceId, tabId);
  await closeEditorSession({ workspaceId, tabId });
}

/** Records a model change so it reaches the server as incremental edits. */
export function queueEditorBufferSynchronization(
  buffer: EditorBuffer,
  event?: editor.IModelContentChangedEvent,
): void {
  if (buffer.model.isDisposed() || buffer.applyingServerText) {
    return;
  }
  if (!event || event.isEolChange) {
    buffer.session.needsFullSync = true;
  } else {
    buffer.session.pendingEdits.push(...event.changes.map(toEditorTextEdit));
  }
  nextEditorBufferRevision(buffer);
  scheduleEditorBufferSynchronization(buffer);
}

function scheduleEditorBufferSynchronization(buffer: EditorBuffer): void {
  if (sessionRecovery || buffer.session.synchronization) {
    return;
  }
  buffer.session.synchronization = Promise.resolve().then(() => synchronizeEditorBuffer(buffer));
  void buffer.session.synchronization.catch(() => {});
}

function toEditorTextEdit(change: editor.IModelContentChange): EditorTextEdit {
  return { offset: change.rangeOffset, length: change.rangeLength, text: change.text };
}

function hasUnsyncedEditorChanges(buffer: EditorBuffer): boolean {
  return buffer.session.pendingEdits.length > 0 || buffer.session.needsFullSync;
}

export async function flushEditorBuffer(buffer: EditorBuffer): Promise<void> {
  await sessionRecovery;
  await buffer.session.opening;
  if (buffer.session.lastError) {
    throw buffer.session.lastError;
  }
  if (!hasUnsyncedEditorChanges(buffer) && !buffer.session.synchronization) {
    return;
  }
  scheduleEditorBufferSynchronization(buffer);
  await buffer.session.synchronization;
  if (hasUnsyncedEditorChanges(buffer)) {
    await flushEditorBuffer(buffer);
  }
  if (buffer.session.lastError) {
    throw buffer.session.lastError;
  }
}

async function restoreEditorBufferSessions(): Promise<void> {
  await Promise.all(
    [...buffers.values()].map(async (buffer) => {
      await buffer.session.opening?.catch(() => undefined);
      await buffer.session.synchronization?.catch(() => undefined);
      if (buffer.model.isDisposed()) {
        return;
      }

      // The restored text includes every edit made so far; later edits stay queued.
      buffer.session.pendingEdits = [];
      buffer.session.needsFullSync = false;
      const revision = nextEditorBufferRevision(buffer);
      const ack = await restoreEditorSession({
        workspaceId: buffer.workspaceId,
        tabId: buffer.tabId,
        path: buffer.path,
        content: buffer.model.getValue(),
        savedContent: buffer.savedContent,
        revision,
      });
      if (!ack.accepted) {
        await uploadEditorBufferText(buffer, ack.revision);
      } else {
        buffer.session.syncedRevision = revision;
        buffer.session.savedGeneration = ack.savedGeneration;
      }
      buffer.session.lastError = null;
    }),
  );
}

function resumeEditorBufferSynchronization(): void {
  for (const buffer of buffers.values()) {
    if (hasUnsyncedEditorChanges(buffer)) {
      scheduleEditorBufferSynchronization(buffer);
    }
  }
}

export async function flushEditorBuffers(): Promise<void> {
  await Promise.all([...buffers.values()].map((buffer) => flushEditorBuffer(buffer)));
}

function synchronizeEditorBuffer(buffer: EditorBuffer): Promise<void> {
  return (async () => {
    try {
      await buffer.session.opening;
      while (hasUnsyncedEditorChanges(buffer) && !buffer.model.isDisposed()) {
        await sendEditorBufferChanges(buffer);
      }
      buffer.session.lastError = null;
    } catch (error) {
      buffer.session.lastError = error;
      throw error;
    } finally {
      buffer.session.synchronization = null;
    }
  })();
}

async function sendEditorBufferChanges(buffer: EditorBuffer): Promise<void> {
  if (buffer.session.needsFullSync) {
    await uploadEditorBufferText(buffer, buffer.session.syncedRevision);
    return;
  }
  const edits = buffer.session.pendingEdits;
  const revision = buffer.session.revision;
  buffer.session.pendingEdits = [];
  const ack = await changeEditorSession({
    workspaceId: buffer.workspaceId,
    tabId: buffer.tabId,
    baseRevision: buffer.session.syncedRevision,
    revision,
    edits,
  });
  if (ack.accepted) {
    buffer.session.syncedRevision = revision;
    buffer.session.savedGeneration = ack.savedGeneration;
    return;
  }
  await uploadEditorBufferText(buffer, ack.revision);
}

export function editorBuffersForPath(workspaceId: number, path: string): EditorBuffer[] {
  return [...buffers.values()].filter(
    (buffer) =>
      buffer.workspaceId === workspaceId &&
      (buffer.path === path || buffer.path.startsWith(`${path}/`)),
  );
}

export function editorBufferForModel(model: editor.ITextModel): EditorBuffer | null {
  return [...buffers.values()].find((buffer) => buffer.model === model) ?? null;
}

export function editorBuffer(workspaceId: number, tabId: number): EditorBuffer | null {
  return buffers.get(bufferKey(workspaceId, tabId)) ?? null;
}

export async function flushWorkspaceEditorBuffers(workspaceId: number): Promise<void> {
  await Promise.all(
    [...buffers.values()]
      .filter((buffer) => buffer.workspaceId === workspaceId)
      .map((buffer) => flushEditorBuffer(buffer)),
  );
}

export function rebindEditorBuffer(
  buffer: EditorBuffer,
  path: string,
  model: editor.ITextModel,
): void {
  const languageDocument = attachDocument(
    buffer.workspaceId,
    buffer.tabId,
    path,
    model,
    buffer.languageFeatures,
  );
  const previousLanguageDocument = buffer.languageDocument;
  buffer.model = model;
  buffer.path = path;
  buffer.languageDocument = languageDocument;
  markEditorBufferSavedIfEqual(buffer);
  for (const listener of buffer.modelListeners) {
    listener(model);
  }
  previousLanguageDocument.dispose();
}

export function subscribeEditorBufferModel(
  buffer: EditorBuffer,
  listener: (model: editor.ITextModel) => void,
): () => void {
  buffer.modelListeners.add(listener);
  return () => buffer.modelListeners.delete(listener);
}

export function lockEditorBuffer(buffer: EditorBuffer, transactionId: number): () => void {
  const wasLocked = isEditorBufferLocked(buffer);
  const lockState = buffer.lockState;
  lockState.operationEpoch += 1;
  lockState.transactions.set(
    transactionId,
    (lockState.transactions.get(transactionId) ?? 0) + 1,
  );
  if (!wasLocked) {
    notifyBufferLockListeners(lockState);
  }
  let released = false;
  return () => {
    if (released) return;
    released = true;
    const count = lockState.transactions.get(transactionId);
    if (count === undefined) return;
    if (count === 1) {
      lockState.transactions.delete(transactionId);
    } else {
      lockState.transactions.set(transactionId, count - 1);
    }
    if (lockState.transactions.size === 0) {
      notifyBufferLockListeners(lockState);
    }
  };
}

export function beginEditorBufferOperation(
  buffer: EditorBuffer,
  model: editor.ITextModel = buffer.model,
): { isCurrent(): boolean } {
  const operationEpoch = buffer.lockState.operationEpoch;
  return {
    isCurrent() {
      return (
        operationEpoch === buffer.lockState.operationEpoch &&
        !isEditorBufferLocked(buffer) &&
        buffer.model === model &&
        !model.isDisposed()
      );
    },
  };
}

export function captureEditorBufferState(buffer: EditorBuffer): EditorBufferState {
  return {
    buffer,
    path: buffer.path,
    model: buffer.model,
    version: buffer.model.getVersionId(),
    content: buffer.model.getValue(),
    savedContent: buffer.savedContent,
  };
}

export function isEditorBufferStateCurrent(state: EditorBufferState): boolean {
  return (
    state.buffer.path === state.path &&
    state.buffer.model === state.model &&
    !state.model.isDisposed() &&
    state.buffer.savedContent === state.savedContent &&
    state.model.getVersionId() === state.version &&
    state.model.getValue() === state.content
  );
}

export function isEditorBufferLocked(buffer: EditorBuffer): boolean {
  return buffer.lockState.transactions.size > 0;
}

export function assertEditorBufferEditable(buffer: EditorBuffer): void {
  if (isEditorBufferLocked(buffer)) {
    throw new Error("This editor is locked while a workspace edit is being resolved.");
  }
}

export function assertEditorBufferCleanForOverwrite(buffer: EditorBuffer): void {
  if (isEditorBufferDirty(buffer)) {
    throw new Error(`Cannot overwrite dirty open document ${buffer.path}.`);
  }
}

export function subscribeEditorBufferLock(
  buffer: EditorBuffer,
  listener: (locked: boolean) => void,
): () => void {
  buffer.lockState.listeners.add(listener);
  return () => buffer.lockState.listeners.delete(listener);
}

function notifyBufferLockListeners(lockState: EditorBufferLockState): void {
  const locked = lockState.transactions.size > 0;
  for (const listener of lockState.listeners) {
    listener(locked);
  }
}

export function invalidateEditorBuffer(buffer: EditorBuffer): void {
  buffer.languageDocument.dispose();
}

export function revalidateEditorBuffer(buffer: EditorBuffer): void {
  buffer.languageDocument.dispose();
  buffer.languageDocument = attachDocument(
    buffer.workspaceId,
    buffer.tabId,
    buffer.path,
    buffer.model,
    buffer.languageFeatures,
  );
}

export function detachEditorBuffer(buffer: EditorBuffer): void {
  const key = bufferKey(buffer.workspaceId, buffer.tabId);
  if (buffers.get(key) === buffer) {
    buffers.delete(key);
  }
  buffer.languageDocument.dispose();
}

export function suspendEditorBuffer(buffer: EditorBuffer): EditorBufferState {
  const state = captureEditorBufferState(buffer);
  detachEditorBuffer(buffer);
  state.model.dispose();
  return state;
}

export function restoreDetachedEditorBuffer(
  buffer: EditorBuffer,
  path: string,
  model: editor.ITextModel,
): void {
  const key = bufferKey(buffer.workspaceId, buffer.tabId);
  if (buffers.has(key)) {
    throw new Error(`Editor buffer ${key} was replaced while a workspace edit was unresolved.`);
  }
  buffer.model = model;
  buffer.path = path;
  buffer.languageDocument = attachDocument(
    buffer.workspaceId,
    buffer.tabId,
    path,
    model,
    buffer.languageFeatures,
  );
  markEditorBufferSavedIfEqual(buffer);
  buffers.set(key, buffer);
  for (const listener of buffer.modelListeners) {
    listener(model);
  }
}

export function restoreSuspendedEditorBuffer(
  state: EditorBufferState,
  model: editor.ITextModel,
): void {
  if (model.getValue() !== state.content) {
    throw new Error(`Restored editor buffer ${state.path} has unexpected content.`);
  }
  state.buffer.savedContent = state.savedContent;
  restoreDetachedEditorBuffer(state.buffer, state.path, model);
}

/** O(1): compares Monaco's version with the one recorded when the text was saved. */
export function isEditorBufferDirty(buffer: EditorBuffer): boolean {
  return (
    buffer.savedVersionId === null ||
    buffer.model.getAlternativeVersionId() !== buffer.savedVersionId
  );
}

function markEditorBufferSavedIfEqual(buffer: EditorBuffer): void {
  buffer.savedVersionId =
    buffer.model.getValue() === buffer.savedContent
      ? buffer.model.getAlternativeVersionId()
      : null;
}

/**
 * Applies a newer server document. A clean buffer takes the server text; a dirty one
 * keeps its edits and only adopts the new saved baseline. Returns whether it is dirty.
 */
export function reconcileEditorBuffer(buffer: EditorBuffer, document: EditorDocument): boolean {
  const wasDirty = isEditorBufferDirty(buffer);
  const serverIsNewer = document.revision > buffer.session.syncedRevision;
  buffer.savedContent = document.savedContent ?? document.content;
  buffer.session.savedGeneration = document.savedGeneration;

  if (serverIsNewer && !wasDirty && !hasUnsyncedEditorChanges(buffer)) {
    adoptServerText(buffer, document.content, document.revision);
  } else if (serverIsNewer) {
    // Local edits win over a server change the renderer never saw.
    buffer.session.needsFullSync = true;
    scheduleEditorBufferSynchronization(buffer);
  }

  markEditorBufferSavedIfEqual(buffer);
  return isEditorBufferDirty(buffer);
}

/** Puts server text into the model without echoing it back as local edits. */
function adoptServerText(buffer: EditorBuffer, content: string, serverRevision: number): void {
  buffer.applyingServerText = true;
  try {
    replaceEditorBufferContent(buffer, content);
  } finally {
    buffer.applyingServerText = false;
  }
  buffer.session.syncedRevision = serverRevision;
  alignEditorBufferRevision(buffer);
}

/**
 * Language features compare the server revision with Monaco's version id, so after
 * adopting server text the (unchanged) session revision is advanced to match.
 */
function alignEditorBufferRevision(buffer: EditorBuffer): void {
  if (buffer.model.getVersionId() <= buffer.session.syncedRevision) {
    return;
  }
  buffer.session.pendingEdits.push({ offset: 0, length: 0, text: "" });
  nextEditorBufferRevision(buffer);
  scheduleEditorBufferSynchronization(buffer);
}

/** Replaces the whole buffer as one undoable edit so external reloads keep undo history. */
function replaceEditorBufferContent(buffer: EditorBuffer, content: string): void {
  buffer.model.pushEditOperations(
    [],
    [{ range: buffer.model.getFullModelRange(), text: content }],
    () => null,
  );
}

export function applyEditorSaveProjection(buffer: EditorBuffer, result: EditorSave): boolean {
  if (
    buffer.model.isDisposed() ||
    result.savedRevision !== result.currentRevision ||
    result.savedRevision !== buffer.session.revision
  ) {
    return false;
  }

  buffer.session.savedGeneration = result.savedGeneration;
  if (result.savedContent == null) {
    buffer.savedContent = buffer.model.getValue();
  } else {
    buffer.savedContent = result.savedContent;
    adoptServerText(buffer, result.savedContent, result.currentRevision);
  }
  buffer.savedVersionId = buffer.model.getAlternativeVersionId();
  return true;
}

export function editorSaveWarningMessage(warning: EditorSaveWarning): string {
  const label =
    warning.kind === "formatting"
      ? "Formatting failed"
      : "Language server save notification failed";
  return `${label}: ${warning.message}`;
}

export function disposeEditorBuffer(workspaceId: number, tabId: number): void {
  const key = bufferKey(workspaceId, tabId);
  const buffer = buffers.get(key);

  buffer?.languageDocument.dispose();
  buffer?.model.dispose();
  buffers.delete(key);
}

export function disposeWorkspaceEditorBuffers(workspaceId: number): void {
  const prefix = `${workspaceId}:`;

  for (const [key, buffer] of buffers) {
    if (!key.startsWith(prefix)) {
      continue;
    }

    buffer.languageDocument.dispose();
    buffer.model.dispose();
    buffers.delete(key);
  }
}

/** Disposes buffers whose tabs were closed elsewhere, such as by a file tree deletion. */
export function disposeEditorBuffersForTabKeys(tabKeys: readonly string[]): void {
  for (const key of tabKeys) {
    const buffer = buffers.get(key);
    if (buffer) {
      disposeEditorBuffer(buffer.workspaceId, buffer.tabId);
    }
  }
}

export function disposeAllEditorBuffers(): void {
  for (const buffer of [...buffers.values()]) {
    disposeEditorBuffer(buffer.workspaceId, buffer.tabId);
  }
}

function bufferKey(workspaceId: number, tabId: number): string {
  return `${workspaceId}:${tabId}`;
}
