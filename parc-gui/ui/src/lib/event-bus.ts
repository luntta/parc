type Handler = (...args: unknown[]) => void;

const listeners = new Map<string, Set<Handler>>();

export function on(event: string, handler: Handler): () => void {
  if (!listeners.has(event)) listeners.set(event, new Set());
  listeners.get(event)!.add(handler);
  return () => listeners.get(event)?.delete(handler);
}

export function emit(event: string, ...args: unknown[]): void {
  listeners.get(event)?.forEach((h) => h(...args));
}

export function off(event: string, handler: Handler): void {
  listeners.get(event)?.delete(handler);
}
