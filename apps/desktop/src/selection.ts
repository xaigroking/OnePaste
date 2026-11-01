export interface Selection { ids: string[]; anchor: string | null; }
export function selectRow(previous: Selection, order: string[], id: string, toggle = false, range = false): Selection {
  if (!order.includes(id)) return previous;
  if (range) {
    const start = order.indexOf(previous.anchor ?? id), end = order.indexOf(id);
    const from = start < 0 ? end : start;
    const span = order.slice(Math.min(from, end), Math.max(from, end) + 1);
    const selected = new Set(toggle ? [...previous.ids, ...span] : span);
    return { ids: order.filter((value) => selected.has(value)), anchor: start < 0 || previous.anchor === null ? id : previous.anchor };
  }
  if (!toggle) return { ids: [id], anchor: id };
  const selected = new Set(previous.ids);
  if (selected.has(id)) selected.delete(id); else selected.add(id);
  return { ids: order.filter((value) => selected.has(value)), anchor: id };
}
export function retainSelection(previous: Selection, order: string[]): Selection {
  const selected = new Set(previous.ids);
  const ids = order.filter((id) => selected.has(id));
  const anchor = previous.anchor && order.includes(previous.anchor) ? previous.anchor : null;
  return ids.length === previous.ids.length && ids.every((id, index) => id === previous.ids[index]) && anchor === previous.anchor ? previous : { ids, anchor };
}
