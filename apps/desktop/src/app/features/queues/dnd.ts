/** Drag type of a queue row in the sidebar (payload: the queue id). */
export const QUEUE_DRAG_TYPE = 'application/x-riptide-queue';

/** Drag type of download rows (payload: a JSON array of download ids). */
export const DOWNLOAD_DRAG_TYPE = 'application/x-riptide-downloads';

/** Whether a drag carries `type` (readable during `dragover`, unlike the payload). */
export function dragHas(event: DragEvent, type: string): boolean {
  return Array.from(event.dataTransfer?.types ?? []).includes(type);
}

/** The download ids of a drop; empty when the payload is missing or not a list of numbers. */
export function droppedDownloadIds(event: DragEvent): readonly number[] {
  try {
    const parsed: unknown = JSON.parse(event.dataTransfer?.getData(DOWNLOAD_DRAG_TYPE) ?? '[]');
    return Array.isArray(parsed) ? parsed.filter((n): n is number => typeof n === 'number' && Number.isInteger(n)) : [];
  } catch {
    return [];
  }
}

/** Whether the pointer is in the lower half of `el` (the drop lands after it). */
export function inLowerHalf(event: DragEvent, el: HTMLElement): boolean {
  const rect = el.getBoundingClientRect();
  return event.clientY > rect.top + rect.height / 2;
}
