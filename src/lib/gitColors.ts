/** Get the CSS color class for a git file status. */
export function gitStatusColor(status: string): string {
  switch (status) {
    case "added":
    case "untracked":
      return "text-[var(--color-status-green)]";
    case "deleted":
      return "text-[var(--color-status-red)]";
    default:
      return "text-[var(--color-status-amber)]";
  }
}

/** Get a short label for a git file status. */
export function gitStatusLabel(status: string): string {
  switch (status) {
    case "added":
      return "A";
    case "deleted":
      return "D";
    case "renamed":
      return "R";
    default:
      return "M";
  }
}
