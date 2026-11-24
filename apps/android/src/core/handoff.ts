/**
 * A scanned code's way back to the screen that opened the scanner: that screen registers a
 * receiver before pushing `/scan`, and the scanner hands the first code it reads to it.
 */
let receiver: ((code: string) => void) | null = null;

export function expectScan(onCode: (code: string) => void) {
  receiver = onCode;
}

export function deliverScan(code: string): boolean {
  const r = receiver;
  receiver = null;
  if (!r) return false;
  r(code);
  return true;
}

/** The scanner closed without a code: nothing is waiting any more. */
export function cancelScan() {
  receiver = null;
}
