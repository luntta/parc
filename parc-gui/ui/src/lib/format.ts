export function relativeTime(dateStr: string): string {
  const date = new Date(dateStr);
  const now = new Date();
  const diff = now.getTime() - date.getTime();
  const seconds = Math.floor(diff / 1000);
  const minutes = Math.floor(seconds / 60);
  const hours = Math.floor(minutes / 60);
  const days = Math.floor(hours / 24);

  if (seconds < 60) return "just now";
  if (minutes < 60) return `${minutes}m ago`;
  if (hours < 24) return `${hours}h ago`;
  if (days < 7) return `${days}d ago`;
  if (days < 30) return `${Math.floor(days / 7)}w ago`;
  return date.toLocaleDateString();
}

export function shortId(id: string): string {
  return id.slice(0, 8);
}

export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function typeColor(type: string): string {
  const map: Record<string, string> = {
    note: "var(--type-note)",
    todo: "var(--type-todo)",
    decision: "var(--type-decision)",
    risk: "var(--type-risk)",
    idea: "var(--type-idea)",
  };
  return map[type] || "var(--text-muted)";
}

export function statusColor(status: string | null): string {
  if (!status) return "var(--text-muted)";
  const map: Record<string, string> = {
    open: "var(--status-open)",
    active: "var(--status-active)",
    done: "var(--status-done)",
    accepted: "var(--status-done)",
    rejected: "var(--status-cancelled)",
    cancelled: "var(--status-cancelled)",
    mitigated: "var(--status-done)",
  };
  return map[status] || "var(--text-muted)";
}
