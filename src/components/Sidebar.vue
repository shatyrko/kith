<script setup>
import { computed, ref, nextTick } from 'vue';
import { state, setGroup, importFile, exportAll, googleSync, notify, refresh, theApi } from '../state.js';
import { vContext } from '../contextmenu.js';
import Icon from './Icon.vue';

const group = computed(() => state.query.group);
const g = computed(() => state.google);
const ago = computed(() => {
  const t = g.value.last_sync;
  if (!t) return 'Never synced';
  const m = Math.round((Date.now() / 1000 - t) / 60);
  if (m < 1) return 'Synced just now';
  if (m < 60) return `Synced ${m} min ago`;
  if (m < 1440) return `Synced ${Math.round(m / 60)} h ago`;
  return `Synced ${Math.round(m / 1440)} d ago`;
});

// labels: right-click to show or rename; renaming happens in place
const renaming = ref(null);
const newName = ref('');
function labelMenu(l) {
  return [
    { label: 'Show contacts', icon: 'users-round', action: () => setGroup('label:' + l.name) },
    { label: 'Rename label…', icon: 'pencil', action: async () => { renaming.value = l.name; newName.value = l.name; await nextTick(); document.querySelector('.rename-input')?.select(); } },
  ];
}
async function finishRename(commit) {
  const from = renaming.value, to = newName.value.trim();
  renaming.value = null;
  if (!commit || !to || to === from) return;
  try {
    await theApi().renameLabel(from, to);
    const was = group.value === 'label:' + from;
    await refresh();
    if (was) setGroup('label:' + to);
    notify('Label renamed');
  } catch (e) { notify(String(e?.message || e), 'error'); }
}
const syncMenu = () => g.value.signed_in
  ? [{ label: 'Sync now', icon: 'refresh', action: sync }, { label: 'Google settings…', icon: 'settings', action: () => (state.dialog = 'settings') }]
  : [{ label: 'Connect Google…', icon: 'cloud', action: () => (state.dialog = 'settings') }];
async function sync() {
  if (!g.value.signed_in) { state.dialog = 'settings'; return; }
  const r = await googleSync(false);
  if (r) notify(`Google: ${r.pulled_new} new, ${r.pulled_updated} updated, ${r.pulled_deleted} removed`);
}
</script>

<template>
  <aside class="side">
    <div class="brand" data-drag>
      <div class="logo"><Icon name="contact" :size="18" /></div>
      Kith
    </div>
    <div class="nav-scroll">
      <button class="nav-item" :class="{ active: group === 'all' }" @click="setGroup('all')"><Icon name="users-round" /> All contacts <span class="count">{{ state.total }}</span></button>
      <button class="nav-item" :class="{ active: group === 'favorites' }" @click="setGroup('favorites')"><Icon name="star" /> Favorites</button>
      <button class="nav-item" :class="{ active: group === 'birthdays' }" @click="setGroup('birthdays')"><Icon name="cake" /> Birthdays</button>
      <button class="nav-item" :class="{ active: group === 'recent' }" @click="setGroup('recent')"><Icon name="clock3" /> Recently added</button>

      <div class="nav-title">Labels</div>
      <template v-for="l in state.labels" :key="l.name">
        <div v-if="renaming === l.name" class="nav-item active">
          <Icon name="tag" />
          <input v-model="newName" class="input rename-input" @keydown.enter.prevent="finishRename(true)" @keydown.esc.stop.prevent="finishRename(false)" @blur="finishRename(true)" />
        </div>
        <button v-else class="nav-item" :class="{ active: group === 'label:' + l.name }" v-context="() => labelMenu(l)" @click="setGroup('label:' + l.name)">
          <Icon name="tag" /> {{ l.name }} <span class="count">{{ l.count }}</span>
        </button>
      </template>
      <button class="nav-item" :class="{ active: group === 'nolabel' }" @click="setGroup('nolabel')"><Icon name="folder" /> No label</button>

      <div class="nav-title">Tools</div>
      <button class="nav-item" @click="state.dialog = 'duplicates'"><Icon name="merge" /> Find duplicates</button>
      <button class="nav-item" @click="importFile"><Icon name="download" /> Import vCard…</button>
      <button class="nav-item" @click="exportAll"><Icon name="upload" /> Export all…</button>
      <button class="nav-item" @click="state.dialog = 'trash'"><Icon name="trash" /> Trash</button>
    </div>
    <div class="row" style="padding: 10px 6px 0; border-top: 1px solid var(--border); margin-top: 8px">
      <button class="nav-item" style="flex: 1; min-width: 0" v-context="syncMenu" :title="g.signed_in ? 'Sync with Google now' : 'Set up Google sync'" @click="sync">
        <Icon :name="g.signed_in ? 'cloud' : 'cloud-off'" :class="{ spin: state.syncing }" />
        <span style="min-width: 0; overflow: hidden">
          <div style="font-weight: 600; color: var(--text); white-space: nowrap; text-overflow: ellipsis; overflow: hidden">{{ g.signed_in ? g.account || 'Google' : 'Google sync' }}</div>
          <div style="font-size: 12px; color: var(--faint)">{{ g.signed_in ? ago : 'Not connected' }}</div>
        </span>
      </button>
      <button class="btn ghost icon" title="Settings" @click="state.dialog = 'settings'"><Icon name="settings" /></button>
    </div>
  </aside>
</template>

<style scoped>
.spin { animation: spin 1s linear infinite; }
@keyframes spin { to { transform: rotate(360deg); } }
</style>
