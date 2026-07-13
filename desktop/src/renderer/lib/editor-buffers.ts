import type { editor } from "monaco-editor";

import {
  changeEditorSession,
  openEditorSession,
  restoreEditorSession,
} from "@/renderer/ipc";
import type { EditorDocument, EditorSave, EditorSaveWarning } from "@/shared/ipc";

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
  acknowledgedRevision: number;
  lastError: unknown | null;
  opening: Promise<void> | null;
  queuedContent: string | null;
  queuedRevision: number | null;
  revision: number;
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

function attachDocument(
  workspaceId: number,
  tabId: number,
  path: string,
  model: editor.ITextModel,
): LanguageDocumentHandle {
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
  savedContent: string;
  languageDocument: LanguageDocumentHandle;
  modelListeners: Set<(model: editor.ITextModel) => void>;
  lockState: EditorBufferLockState;
  session: EditorSessionState;
};

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
  content: string,
  createModel: () => editor.ITextModel,
): EditorBuffer {
  const key = bufferKey(workspaceId, tabId);
  const existing = buffers.get(key);

  if (existing?.path === path && !existing.model.isDisposed()) {
    return existing;
  }

  existing?.languageDocument.dispose();
  existing?.model.dispose();

  const model = createModel();
  const buffer = {
    workspaceId,
    tabId,
    model,
    path,
    savedContent: content,
    languageDocument: attachDocument(workspaceId, tabId, path, model),
    modelListeners: new Set<(model: editor.ITextModel) => void>(),
    lockState: existing?.lockState ?? {
      transactions: new Map<number, number>(),
      listeners: new Set<(locked: boolean) => void>(),
      operationEpoch: 0,
    },
    session: existing?.session ?? {
      acknowledgedRevision: 0,
      lastError: null,
      opening: null,
      queuedContent: null,
      queuedRevision: null,
      revision: 0,
      synchronization: null,
    },
  };
  buffers.set(key, buffer);

  return buffer;
}

export function openEditorBufferSession(
  buffer: EditorBuffer,
  document: EditorDocument,
): Promise<void> {
  if (buffer.session.opening) {
    return buffer.session.opening;
  }
  const content = buffer.model.getValue();
  const localRevision = Math.max(buffer.session.revision, document.revision);
  const opening = (async () => {
    const session = await openEditorSession({
      workspaceId: buffer.workspaceId,
      tabId: buffer.tabId,
      path: buffer.path,
      content,
      revision: localRevision,
    });
    buffer.session.acknowledgedRevision = Math.max(
      buffer.session.acknowledgedRevision,
      session.revision,
    );
    buffer.session.revision = Math.max(buffer.session.revision, session.revision);
    buffer.savedContent = session.savedContent;

    if (content === document.content && content !== session.content) {
      buffer.model.setValue(session.content);
      return;
    }
    if (content !== session.content) {
      queueEditorBufferSynchronization(buffer);
    }
  })();
  buffer.session.opening = opening;
  void opening.finally(() => {
    if (buffer.session.opening === opening) {
      buffer.session.opening = null;
    }
  }).catch(() => {});
  return opening;
}

export function queueEditorBufferSynchronization(buffer: EditorBuffer): void {
  if (buffer.model.isDisposed()) {
    return;
  }
  buffer.session.revision = Math.max(buffer.session.revision + 1, buffer.model.getVersionId());
  buffer.session.queuedRevision = buffer.session.revision;
  buffer.session.queuedContent = buffer.model.getValue();
  if (sessionRecovery) {
    return;
  }
  if (buffer.session.synchronization) {
    return;
  }
  buffer.session.synchronization = Promise.resolve().then(() => synchronizeEditorBuffer(buffer));
  void buffer.session.synchronization.catch(() => {});
}

export async function flushEditorBuffer(buffer: EditorBuffer): Promise<void> {
  await sessionRecovery;
  await buffer.session.opening;
  if (buffer.session.lastError) {
    throw buffer.session.lastError;
  }
  if (buffer.session.queuedContent === null && !buffer.session.synchronization) {
    return;
  }
  await buffer.session.synchronization;
  if (buffer.session.queuedContent !== null) {
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

      let minimumRevision = buffer.session.revision;
      while (!buffer.model.isDisposed()) {
        const content = buffer.model.getValue();
        const revision = Math.max(minimumRevision, buffer.model.getVersionId());
        let session = await restoreEditorSession({
          workspaceId: buffer.workspaceId,
          tabId: buffer.tabId,
          path: buffer.path,
          content,
          savedContent: buffer.savedContent,
          revision,
        });
        if (!session.accepted) {
          session = await restoreEditorSession({
            workspaceId: buffer.workspaceId,
            tabId: buffer.tabId,
            path: buffer.path,
            content,
            savedContent: buffer.savedContent,
            revision: Math.max(revision, session.revision) + 1,
          });
        }
        if (!session.accepted || session.content !== content) {
          throw new Error(`Could not restore editor session ${buffer.path}`);
        }
        buffer.session.acknowledgedRevision = session.revision;
        buffer.session.revision = session.revision;
        buffer.session.lastError = null;
        buffer.session.queuedContent = null;
        buffer.session.queuedRevision = null;
        if (buffer.model.getValue() === content) {
          return;
        }
        minimumRevision = session.revision + 1;
      }
    }),
  );
}

export async function flushEditorBuffers(): Promise<void> {
  await Promise.all([...buffers.values()].map((buffer) => flushEditorBuffer(buffer)));
}

function synchronizeEditorBuffer(buffer: EditorBuffer): Promise<void> {
  return (async () => {
    try {
      await buffer.session.opening;
      while (buffer.session.queuedContent !== null && buffer.session.queuedRevision !== null) {
        const content = buffer.session.queuedContent;
        const revision = buffer.session.queuedRevision;
        buffer.session.queuedContent = null;
        buffer.session.queuedRevision = null;
        const session = await changeEditorSession({
          workspaceId: buffer.workspaceId,
          tabId: buffer.tabId,
          content,
          revision,
        });
        buffer.session.acknowledgedRevision = Math.max(
          buffer.session.acknowledgedRevision,
          session.revision,
        );
        buffer.session.revision = Math.max(buffer.session.revision, session.revision);
        if (!session.accepted) {
          if (buffer.model.getValue() === session.content) {
            buffer.savedContent = session.savedContent;
          } else {
            buffer.session.revision = Math.max(buffer.session.revision, session.revision);
            buffer.session.queuedRevision = buffer.session.revision + 1;
            buffer.session.revision = buffer.session.queuedRevision;
            buffer.session.queuedContent = buffer.model.getValue();
          }
        }
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
  const languageDocument = attachDocument(buffer.workspaceId, buffer.tabId, path, model);
  const previousLanguageDocument = buffer.languageDocument;
  buffer.model = model;
  buffer.path = path;
  buffer.languageDocument = languageDocument;
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
  if (buffer.model.getValue() !== buffer.savedContent) {
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
  buffer.languageDocument = attachDocument(buffer.workspaceId, buffer.tabId, path, model);
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

export function reconcileEditorBuffer(buffer: EditorBuffer, content: string): boolean {
  const wasDirty = buffer.model.getValue() !== buffer.savedContent;
  buffer.savedContent = content;

  if (!wasDirty && buffer.model.getValue() !== content) {
    buffer.model.setValue(content);
  }

  return buffer.model.getValue() !== buffer.savedContent;
}

export function applyEditorSaveProjection(buffer: EditorBuffer, result: EditorSave): boolean {
  if (
    buffer.model.isDisposed() ||
    result.savedRevision !== result.currentRevision ||
    result.savedRevision !== buffer.session.revision
  ) {
    return false;
  }

  buffer.savedContent = result.savedContent;
  if (buffer.model.getValue() !== result.savedContent) {
    buffer.model.setValue(result.savedContent);
  }
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

export function disposeAllEditorBuffers(): void {
  for (const buffer of [...buffers.values()]) {
    disposeEditorBuffer(buffer.workspaceId, buffer.tabId);
  }
}

function bufferKey(workspaceId: number, tabId: number): string {
  return `${workspaceId}:${tabId}`;
}
