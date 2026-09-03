/**
 * Borderless icon button (docs/05 §5.1), used for all inline actions in
 * settings rows, sync pair cards and similar compact places. The accessible
 * name and the tooltip both come from `title`.
 */
export default function IconButton({
  title,
  onClick,
  disabled,
  children,
}: {
  title: string;
  onClick: () => void;
  disabled?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      title={title}
      aria-label={title}
      onClick={onClick}
      disabled={disabled}
      className="shrink-0 p-1 text-text-secondary hover:text-text disabled:opacity-40"
    >
      {children}
    </button>
  );
}
