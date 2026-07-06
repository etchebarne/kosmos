type PlaceholderTabProps = {
  title: string;
  onActivatePane(): void;
};

export function PlaceholderTab({ title, onActivatePane }: PlaceholderTabProps) {
  return (
    <div
      className="grid h-full min-h-0 place-items-center overflow-hidden p-5"
      onPointerDown={onActivatePane}
    >
      <h2 className="max-w-full truncate text-3xl font-semibold tracking-tight text-muted-foreground">
        {title}
      </h2>
    </div>
  );
}
