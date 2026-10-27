// The app's own right-click menu. A webview shows the browser's menu (Back, Reload, Inspect…) on a right click, which makes a native
// app look like a web page, so that menu is switched off everywhere and replaced:
//   - `v-context="(event) => items"` on an element gives it a menu (the innermost element with one wins; items may come as a promise),
//   - text fields get Cut / Copy / Paste / Select all, selected text gets Copy, everything else gets no menu at all.
// Item: { label, icon, hint, danger, disabled, checked, action } or { separator: true }.
import { reactive, nextTick } from 'vue';
import { notify } from './state.js';

export const menu = reactive({ open: false, x: 0, y: 0, items: [], focus: -1 });

const registry = new WeakMap();
let host = null;
let returnTo = null;

export const vContext = {
  mounted: (el, b) => registry.set(el, b.value),
  updated: (el, b) => registry.set(el, b.value),
  unmounted: (el) => registry.delete(el),
};

const TEXT_TYPES = ['text', 'search', 'email', 'url', 'tel', 'password', 'number', ''];
const isField = (el) => el && (el.tagName === 'TEXTAREA' || (el.tagName === 'INPUT' && TEXT_TYPES.includes(el.type)));

function textMenu(t) {
  const f = t.closest && t.closest('input, textarea');
  if (isField(f)) {
    const sel = f.selectionStart !== f.selectionEnd;
    const ro = f.readOnly || f.disabled;
    return [
      { label: 'Cut', icon: 'scissors', hint: 'Ctrl+X', disabled: !sel || ro, action: () => { f.focus(); document.execCommand('cut'); } },
      { label: 'Copy', icon: 'copy', hint: 'Ctrl+C', disabled: !sel, action: () => { f.focus(); document.execCommand('copy'); } },
      { label: 'Paste', icon: 'clipboard-paste', hint: 'Ctrl+V', disabled: ro, action: () => paste(f) },
      { separator: true },
      { label: 'Select all', icon: 'text-select', hint: 'Ctrl+A', disabled: !f.value, action: () => { f.focus(); f.select(); } },
    ];
  }
  const s = String(getSelection());
  if (s.trim()) return [{ label: 'Copy', icon: 'copy', hint: 'Ctrl+C', action: () => navigator.clipboard.writeText(s).catch(() => document.execCommand('copy')) }];
  return [];
}

async function paste(f) {
  f.focus();
  try {
    const text = await navigator.clipboard.readText();
    f.setRangeText(text, f.selectionStart, f.selectionEnd, 'end');
    f.dispatchEvent(new Event('input', { bubbles: true }));
  } catch {
    notify('The clipboard is not readable here. Press Ctrl+V instead.');
  }
}

async function onContext(e) {
  // `npm run dev` only: Shift + right click still opens the browser menu, for Inspect
  if (import.meta.env.DEV && e.shiftKey) return;
  e.preventDefault();
  closeMenu(false);
  const { clientX: x, clientY: y } = e;
  let items = null;
  for (let el = e.target; el && el !== document; el = el.parentElement) {
    const fn = registry.get(el);
    if (!fn) continue;
    items = await fn(e);
    if (items && items.length) { host = el; break; }
  }
  if (!items || !items.length) items = textMenu(e.target);
  if (!items.length) return;
  returnTo = document.activeElement;
  host && host.classList.add('ctx-open');
  Object.assign(menu, { open: true, x, y, items, focus: -1 });
  nextTick(() => document.querySelector('.ctxmenu')?.focus({ preventScroll: true }));
}

export function closeMenu(restoreFocus = true) {
  if (!menu.open) return;
  menu.open = false;
  host && host.classList.remove('ctx-open');
  host = null;
  if (restoreFocus && returnTo && returnTo.focus) returnTo.focus({ preventScroll: true });
  returnTo = null;
}

// the keyboard route to the same menu: the Menu key or Shift+F10 opens it at the focused element (or the selected row)
function onMenuKey(e) {
  if (menu.open || !(e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10'))) return;
  const a = document.activeElement;
  const el = a && a !== document.body ? a : document.querySelector('.item.active');
  if (!el) return;
  e.preventDefault();
  const r = el.getBoundingClientRect();
  el.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: r.left + Math.min(40, r.width / 2), clientY: r.top + r.height / 2 }));
}

export function installContextMenu() {
  window.addEventListener('contextmenu', onContext, true);
  window.addEventListener('keydown', onMenuKey);
  // anything that moves or changes the page under the menu closes it
  window.addEventListener('mousedown', (e) => { if (menu.open && !e.target.closest('.ctxmenu')) closeMenu(false); }, true);
  window.addEventListener('wheel', () => closeMenu(false), { capture: true, passive: true });
  window.addEventListener('resize', () => closeMenu(false));
  window.addEventListener('blur', () => closeMenu(false));
}
