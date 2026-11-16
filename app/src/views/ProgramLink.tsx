/** A program's name that opens that program's history. Used wherever the Network screens name a program. */
export function ProgramLink({
  name,
  path,
  onOpen,
}: {
  name: string;
  path: string;
  onOpen: (program: { name: string; path: string }) => void;
}) {
  return (
    <button type="button" className="linklike" title={path} onClick={() => onOpen({ name, path })}>
      {name}
    </button>
  );
}
