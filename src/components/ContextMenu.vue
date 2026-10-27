<script setup>
import { ref, watch, nextTick } from 'vue';
import { menu, closeMenu } from '../contextmenu.js';
import Icon from './Icon.vue';

const root = ref(null);
const pos = ref({ x: 0, y: 0 });

// keep the menu inside the window: shift it left, or open it upwards, when it would run off an edge
watch(() => menu.open, async (open) => {
  if (!open) return;
  pos.value = { x: menu.x, y: menu.y };
  await nextTick();
  const el = root.value;
  if (!el) return;
  const w = el.offsetWidth, h = el.offsetHeight, m = 8;
  pos.value = {
    x: Math.max(m, Math.min(menu.x, innerWidth - w - m)),
    y: Math.max(m, menu.y + h + m > innerHeight ? menu.y - h : menu.y),
  };
});

const usable = (i) => menu.items[i] && !menu.items[i].separator && !menu.items[i].disabled;
function step(dir) {
  const n = menu.items.length;
  let i = menu.focus;
  for (let k = 0; k < n; k++) {
    i = (i + dir + n) % n;
    if (usable(i)) { menu.focus = i; return; }
  }
}
async function run(item) {
  if (!item || item.separator || item.disabled) return;
  closeMenu();
  await item.action?.();
}
function onKey(e) {
  const k = e.key;
  if (k === 'ArrowDown') step(1);
  else if (k === 'ArrowUp') step(-1);
  else if (k === 'Home') { menu.focus = -1; step(1); }
  else if (k === 'End') { menu.focus = 0; step(-1); }
  else if (k === 'Enter' || k === ' ') run(menu.items[menu.focus]);
  else if (k === 'Escape' || k === 'Tab') closeMenu();
  else return;
  e.preventDefault();
  e.stopPropagation();
}
</script>

<template>
  <div v-if="menu.open" ref="root" class="ctxmenu" role="menu" tabindex="-1" :style="{ left: pos.x + 'px', top: pos.y + 'px' }" @keydown="onKey" @contextmenu.prevent>
    <template v-for="(it, i) in menu.items" :key="i">
      <div v-if="it.separator" class="ctx-sep" role="separator"></div>
      <button v-else class="ctx-item" role="menuitem" :class="{ focus: menu.focus === i, danger: it.danger }" :disabled="it.disabled" tabindex="-1"
        @mouseenter="usable(i) && (menu.focus = i)" @click="run(it)">
        <Icon v-if="it.checked || it.icon" :name="it.checked ? 'check' : it.icon" :size="16" /><span v-else style="width: 16px; flex: none"></span>
        <span class="ctx-label">{{ it.label }}</span>
        <span v-if="it.hint" class="ctx-hint">{{ it.hint }}</span>
      </button>
    </template>
  </div>
</template>
