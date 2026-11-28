// Elements and rendering: function components are called until only intrinsic elements
// (`<box>`, `<extrude>`, ...) are left, as a plain tree of nodes.

/** Where an element is in the user's file, from the JSX development transform. */
export interface Source {
  fileName?: string;
  lineNumber?: number;
  columnNumber?: number;
}

export const Fragment = Symbol.for("solidcraft.fragment");

export type Component = (props: any) => Child;
export interface Element {
  readonly $$element: true;
  type: string | Component | typeof Fragment;
  props: Record<string, unknown>;
  source?: Source;
}
export type Child = Element | Child[] | null | undefined | false | true | string | number;

export function createElement(type: Element["type"], props: Record<string, unknown>, _key?: unknown, source?: Source): Element {
  return { $$element: true, type, props: props ?? {}, source: source?.fileName ? source : callerSource() };
}

/**
 * Where the JSX was written, from the stack, when the transform passes no source (the
 * production transform `"jsx": "react-jsx"`): the first frame outside this package's runtime.
 */
function callerSource(): Source | undefined {
  const stack = new Error().stack?.split("\n") ?? [];
  for (const line of stack.slice(1)) {
    const m = /\(?((?:file:\/\/)?[^()\s]+?):(\d+):(\d+)\)?\s*$/.exec(line);
    if (!m) continue;
    const file = m[1]!;
    if (/[\\/]src[\\/](element|jsx-runtime|jsx-dev-runtime)\.ts$/.test(file)) continue;
    return { fileName: file, lineNumber: Number(m[2]), columnNumber: Number(m[3]) };
  }
  return undefined;
}

export function isElement(x: unknown): x is Element {
  return typeof x === "object" && x !== null && (x as Element).$$element === true;
}

/** An intrinsic element after rendering: its tag, its props without children, its children. */
export interface Node {
  tag: string;
  props: Record<string, unknown>;
  children: Node[];
  source?: Source;
  /** The nearest named function component this node came from, for messages. */
  component?: string;
}

export function render(child: Child, component?: string, source?: Source): Node[] {
  if (child === null || child === undefined || child === false || child === true) return [];
  if (Array.isArray(child)) return child.flatMap((c) => render(c, component, source));
  if (typeof child === "string" || typeof child === "number")
    throw new TypeError(`text ${JSON.stringify(child)} is not a shape${where(source)}: put text in <text>, numbers in props`);
  if (!isElement(child)) throw new TypeError(`${String(child)} is not an element${where(source)}`);
  const { type, props } = child;
  const src = child.source ?? source;
  if (type === Fragment) return render(props.children as Child, component, src);
  if (typeof type === "function") return render(type(props), type.name || component, src);
  const { children, ...rest } = props;
  return [{ tag: type, props: rest, children: render(children as Child, component, src), source: src, component }];
}

export function where(src?: Source): string {
  if (!src?.fileName) return "";
  const file = src.fileName.split(/[\\/]/).pop();
  return ` (${file}:${src.lineNumber ?? "?"})`;
}
