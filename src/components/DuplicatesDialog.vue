<script setup>
import { onMounted, ref } from 'vue';
import { state, refresh, notify, theApi, select } from '../state.js';
import Icon from './Icon.vue';
import Avatar from './Avatar.vue';

const groups = ref(null);
onMounted(async () => { groups.value = await theApi().duplicates(); });
async function merge(g) {
  try {
    const m = await theApi().merge(g.map((c) => c.id));
    groups.value = groups.value.filter((x) => x !== g);
    await refresh();
    if (m) await select(m.id);
    notify('Merged into one contact');
  } catch (e) { notify(String(e?.message || e), 'error'); }
}
</script>

<template>
  <div class="scrim" @mousedown.self="state.dialog = null">
    <div class="dialog">
      <header><Icon name="merge" /><h2>Possible duplicates</h2><span class="spacer"></span>
        <button class="btn ghost icon" @click="state.dialog = null"><Icon name="x" /></button></header>
      <div class="body">
        <div v-if="groups === null" class="muted">Looking…</div>
        <div v-else-if="!groups.length" class="empty" style="height: 160px"><div><div class="big" style="width: 60px; height: 60px"><Icon name="check" :size="28" /></div>No duplicates found.</div></div>
        <div v-for="(g, i) in groups" :key="i" class="card" style="padding: 10px">
          <div class="row" style="flex-wrap: wrap; gap: 14px; margin-bottom: 10px">
            <div v-for="c in g" :key="c.id" class="row"><Avatar :name="c.display" :size="30" /><div><div style="font-weight: 600">{{ c.display }}</div><div class="muted" style="font-size: 12px">{{ c.subtitle }}</div></div></div>
          </div>
          <button class="btn small primary" @click="merge(g)"><Icon name="merge" :size="14" /> Merge {{ g.length }} into one</button>
          <span class="faint" style="font-size: 12px; margin-left: 8px">Everything is combined; the others go to Trash.</span>
        </div>
      </div>
    </div>
  </div>
</template>
