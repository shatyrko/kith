<script setup>
import { onMounted, ref } from 'vue';
import { state, refresh, theApi, notify } from '../state.js';
import Icon from './Icon.vue';
import Avatar from './Avatar.vue';
import { vContext } from '../contextmenu.js';

const items = ref(null);
const load = async () => { items.value = await theApi().trash(); };
onMounted(load);
async function restore(c) { await theApi().restore(c.id); await load(); await refresh(); notify('Restored'); }
async function empty() { await theApi().emptyTrash(); await load(); notify('Trash emptied'); }
</script>

<template>
  <div class="scrim" @mousedown.self="state.dialog = null">
    <div class="dialog">
      <header><Icon name="trash" /><h2>Trash</h2><span class="spacer"></span>
        <button class="btn ghost icon" @click="state.dialog = null"><Icon name="x" /></button></header>
      <div class="body">
        <div v-if="items && !items.length" class="empty" style="height: 160px">Trash is empty. Deleted contacts stay here for 30 days.</div>
        <div v-for="c in items || []" :key="c.id" class="item" style="cursor: default" v-context="() => [{ label: 'Restore', icon: 'undo', action: () => restore(c) }]">
          <Avatar :name="c.display" :size="34" />
          <div class="meta"><div class="name">{{ c.display }}</div><div class="sub">{{ c.subtitle }}</div></div>
          <button class="btn small" @click="restore(c)"><Icon name="undo" :size="14" /> Restore</button>
        </div>
      </div>
      <footer><button class="btn danger" :disabled="!items || !items.length" @click="empty">Empty Trash</button><button class="btn" @click="state.dialog = null">Close</button></footer>
    </div>
  </div>
</template>
