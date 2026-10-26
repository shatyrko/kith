<script setup>
import { computed, onMounted, ref, watch } from 'vue';
import { state, setSearch, select, startNew, photoFor } from '../state.js';
import Icon from './Icon.vue';
import Avatar from './Avatar.vue';

const titles = { all: 'All contacts', favorites: 'Favorites', recent: 'Recently added', birthdays: 'Upcoming birthdays', nolabel: 'No label' };
const title = computed(() => titles[state.query.group] || state.query.group.replace('label:', ''));

// rows with letter headers
const rows = computed(() => {
  const out = [];
  let last = null;
  const flat = state.query.group === 'recent' || state.query.group === 'birthdays';
  for (const c of state.list) {
    if (!flat && c.letter !== last) {
      out.push({ header: c.letter, key: 'h' + c.letter });
      last = c.letter;
    }
    out.push({ c, key: c.id });
  }
  return out;
});

// photos are fetched when a row first shows
const root = ref(null);
let io;
onMounted(() => {
  io = new IntersectionObserver((es) => es.forEach((e) => { if (e.isIntersecting) { photoFor(e.target.dataset.id); io.unobserve(e.target); } }), { root: root.value });
});
function track(el) { if (el && io && el.dataset.photo === '1') io.observe(el); }
watch(() => state.selectedId, (id) => {
  if (!id) return;
  requestAnimationFrame(() => root.value?.querySelector('.item.active')?.scrollIntoView({ block: 'nearest' }));
});
</script>

<template>
  <section class="list">
    <div class="list-head">
      <div class="list-title">
        <h2>{{ title }}</h2>
        <span class="faint">{{ state.list.length }}</span>
        <span class="spacer"></span>
        <button class="btn primary small" title="New contact (Ctrl+N)" @click="startNew"><Icon name="plus" :size="15" /> New</button>
      </div>
      <div class="search">
        <Icon name="search" :size="16" />
        <input class="input" placeholder="Search name, phone, email, notes…" :value="state.query.text" @input="setSearch($event.target.value)" />
      </div>
    </div>
    <div ref="root" class="list-scroll">
      <template v-for="r in rows" :key="r.key">
        <div v-if="r.header" class="letter">{{ r.header }}</div>
        <button v-else class="item" :class="{ active: state.selectedId === r.c.id }" :data-id="r.c.id" :data-photo="r.c.has_photo ? 1 : 0" :ref="track" @click="select(r.c.id)">
          <Avatar :name="r.c.display" :photo="state.photos[r.c.id]" :size="38" :org="r.c.is_org" />
          <div class="meta">
            <div class="name">{{ r.c.display }}</div>
            <div class="sub">{{ r.c.subtitle }}</div>
          </div>
          <Icon v-if="r.c.favorite" name="star" :size="14" fill class="star" />
        </button>
      </template>
      <div v-if="!rows.length" class="empty" style="height: 220px">
        <div>
          <div style="font-weight: 600; color: var(--text)">{{ state.query.text ? 'No matches' : 'Nothing here yet' }}</div>
          <div>{{ state.query.text ? 'Try fewer words, or part of a phone number.' : 'Add a contact or import a vCard.' }}</div>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
.item { content-visibility: auto; contain-intrinsic-size: auto 54px; }
</style>
