/**
 * The HTML shell is part of the browser contract.  Fail immediately with the
 * selector in the error rather than allowing a later null dereference.
 */
export type ElementConstructor<TElement extends Element> = abstract new (...arguments_: never[]) => TElement;

export function requireElement<TElement extends Element>(
  selector: string,
  constructor: ElementConstructor<TElement>
): TElement {
  const element = document.querySelector(selector);
  if (element === null) throw new Error(`Manglar påkravd klientelement: ${selector}`);
  if (!(element instanceof constructor)) {
    throw new Error(`Klientelementet ${selector} har feil DOM-type`);
  }
  return element;
}

export function requireElements<TElement extends Element>(
  selector: string,
  constructor: ElementConstructor<TElement>
): TElement[] {
  const elements = Array.from(document.querySelectorAll(selector));
  if (elements.length === 0) throw new Error(`Manglar påkravde klientelement: ${selector}`);
  const typedElements: TElement[] = [];
  for (const element of elements) {
    if (!(element instanceof constructor)) {
      throw new Error(`Eitt eller fleire klientelement for ${selector} har feil DOM-type`);
    }
    typedElements.push(element);
  }
  return typedElements;
}

export function isKeyboardEvent(event: Event): event is KeyboardEvent {
  return event instanceof KeyboardEvent;
}

export function isClipboardEvent(event: Event): event is ClipboardEvent {
  return event instanceof ClipboardEvent;
}

export function isPointerEvent(event: Event): event is PointerEvent {
  return event instanceof PointerEvent;
}
