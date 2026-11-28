// The JSX runtime: `"jsxImportSource": "solidcraft"` in tsconfig.json (or `/** @jsxImportSource
// solidcraft */` at the top of a file) makes `<box .../>` an Element of this module.
import { Fragment, createElement, type Element, type Source } from "./element.ts";
import type { IntrinsicElements as Intrinsics } from "./elements.ts";
export { Fragment };

export function jsx(type: Element["type"], props: Record<string, unknown>, key?: unknown): Element {
  return createElement(type, props, key);
}
export const jsxs = jsx;

/** Bun and TypeScript's development transform: the same, plus where the element is in the source. */
export function jsxDEV(type: Element["type"], props: Record<string, unknown>, key?: unknown, _static?: boolean, source?: Source): Element {
  return createElement(type, props, key, source);
}

export declare namespace JSX {
  type Element = import("./element.ts").Element;
  interface IntrinsicElements extends Intrinsics {}
  interface ElementChildrenAttribute {
    children: {};
  }
}
