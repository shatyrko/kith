<script setup>
import { onMounted, onBeforeUnmount } from 'vue';
import { state, init, importFile, startNew, startEdit, cancelEdit, saveDraft, select } from './state.js';
import Sidebar from './components/Sidebar.vue';
import ContactList from './components/ContactList.vue';
import ContactView from './components/ContactView.vue';
import ContactEditor from './components/ContactEditor.vue';
import Icon from './components/Icon.vue';
import SettingsDialog from './components/SettingsDialog.vue';
import FieldsDialog from './components/FieldsDialog.vue';
import DuplicatesDialog from './components/DuplicatesDialog.vue';
import TrashDialog from './components/TrashDialog.vue';

function onKey(e) {
  const mod = e.ctrlKey || e.metaKey;
  if (mod && e.key.toLowerCase() === 'n') { e.preventDefault(); startNew(); }
  else if (mod && e.key.toLowerCase() === 'f') { e.preventDefault(); document.querySelector('.search input')?.focus(); }
  else if (mod && e.key.toLowerCase() === 's' && state.mode !== 'view') { e.preventDefault(); saveDraft(); }
  else if (mod && e.key.toLowerCase() === 'e' && state.mode === 'view' && state.contact) { e.preventDefault(); startEdit(); }
  else if ((e.key === 'ArrowDown' || e.key === 'ArrowUp') && state.mode === 'view' && !state.dialog && !['INPUT', 'TEXTAREA', 'SELECT'].includes(document.activeElement?.tagName)) {
    e.preventDefault();
    const i = state.list.findIndex((c) => c.id === state.selectedId);
    const n = state.list[Math.min(state.list.length - 1, Math.max(0, i + (e.key === 'ArrowDown' ? 1 : -1)))];
    if (n) select(n.id);
  } else if (e.key === 'Escape') {
    if (state.dialog) state.dialog = null;
    else if (state.mode !== 'view') cancelEdit();
  }
}
onMounted(() => { init(); window.addEventListener('keydown', onKey); });
onBeforeUnmount(() => window.removeEventListener('keydown', onKey));
</script>

<template>
  <div v-if="state.ready" class="app">
    <Sidebar />
    <ContactList />
    <main class="pane">
      <ContactEditor v-if="state.mode !== 'view'" :key="state.draft?.id || 'new'" />
      <ContactView v-else-if="state.contact" :key="state.contact.id" />
      <div v-else-if="state.total === 0" class="empty">
        <div style="max-width: 460px">
          <div class="big"><Icon name="contact" :size="40" /></div>
          <h2 style="margin: 0 0 6px">Welcome to Kith</h2>
          <div style="margin-bottom: 18px">Your address book is empty. Bring your contacts in, or start with a new one.</div>
          <div class="row" style="justify-content: center; flex-wrap: wrap">
            <button class="btn primary" @click="startNew"><Icon name="plus" :size="16" /> New contact</button>
            <button class="btn" @click="importFile"><Icon name="download" :size="16" /> Import vCard…</button>
            <button class="btn" @click="state.dialog = 'settings'"><Icon name="cloud" :size="16" /> Connect Google</button>
          </div>
        </div>
      </div>
      <div v-else class="empty">
        <div>
          <div class="big"><Icon name="contact" :size="40" /></div>
          <h2 style="margin: 0 0 4px">No contact selected</h2>
          <div>Pick one from the list, or press <span class="kbd">Ctrl</span> <span class="kbd">N</span> to add a new one.</div>
          <button class="btn primary" style="margin-top: 16px" @click="startNew"><Icon name="plus" :size="16" /> New contact</button>
        </div>
      </div>
    </main>
    <SettingsDialog v-if="state.dialog === 'settings'" />
    <FieldsDialog v-if="state.dialog === 'fields'" />
    <DuplicatesDialog v-if="state.dialog === 'duplicates'" />
    <TrashDialog v-if="state.dialog === 'trash'" />
    <div v-if="state.toast" class="toast">
      <Icon :name="state.toast.kind === 'error' ? 'warn' : 'check'" :size="17" :style="{ color: state.toast.kind === 'error' ? 'var(--danger)' : 'var(--ok)' }" />
      {{ state.toast.msg }}
    </div>
  </div>
</template>
