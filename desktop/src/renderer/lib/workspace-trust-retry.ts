export type WorkspaceTrustRetryDocument = {
  disposed: boolean;
  connectionEpoch: number;
};

export function canRetryWorkspaceTrustDocument(
  document: WorkspaceTrustRetryDocument,
  expectedConnectionEpoch: number,
): boolean {
  return !document.disposed && document.connectionEpoch === expectedConnectionEpoch;
}
