<script setup>
import { reactive, ref, computed } from 'vue';
import { state, saveSettings, activeTemplate, notify, theApi } from '../state.js';
import Icon from './Icon.vue';

const base = activeTemplate.value;
const checked = reactive(new Set(base.fields));
const name = ref(base.builtin ? `${base.name} (mine)` : base.name);
const groups = computed(() => {
  const m = {};
  for (const f of state.catalog.fields) (m[f.group] ||= []).push(f);
  return m;
});
function flip(f) {
  if (f.required) return;
  checked.has(f.key) ? checked.delete(f.key) : checked.add(f.key);
}
function all(on) {
  for (const f of state.catalog.fields) on || f.required ? checked.add(f.key) : checked.delete(f.key);
}
async function save() {
  const n = name.value.trim();
  if (!n) return notify('Give the template a name', 'error');
  if (state.catalog.templates.some((t) => t.builtin && t.name === n)) return notify('That name is used by a built-in template', 'error');
  const fields = state.catalog.fields.filter((f) => checked.has(f.key) || f.required).map((f) => f.key);
  const custom = state.settings.custom_templates.filter((t) => t.name !== n);
  custom.push({ name: n, builtin: false, fields });
  await saveSettings({ ...state.settings, custom_templates: custom, active_template: n });
  state.catalog = await theApi().catalog();
  state.dialog = null;
  notify(`Template “${n}” saved`);
}
async function remove() {
  const custom = state.settings.custom_templates.filter((t) => t.name !== base.name);
  await saveSettings({ ...state.settings, custom_templates: custom, active_template: 'Personal' });
  state.catalog = await theApi().catalog();
  state.dialog = null;
}
</script>

<template>
  <div class="scrim" @mousedown.self="state.dialog = null">
    <div class="dialog" style="width: min(760px, calc(100vw - 48px))">
      <header><Icon name="template" /><h2>Fields in the form</h2><span class="spacer"></span>
        <button class="btn ghost icon" @click="state.dialog = null"><Icon name="x" /></button></header>
      <div class="body">
        <p class="muted" style="margin: 0 0 12px">Choose which fields the contact form shows. A field that already has data is always shown, so nothing gets hidden.</p>
        <div class="row" style="margin-bottom: 8px">
          <input class="input" style="max-width: 260px" v-model="name" placeholder="Template name" />
          <button class="btn small" @click="all(true)">All</button>
          <button class="btn small" @click="all(false)">Only the basics</button>
        </div>
        <div v-for="(fs, g) in groups" :key="g">
          <div class="nav-title" style="padding-left: 0">{{ g }}</div>
          <div class="fgrid">
            <button v-for="f in fs" :key="f.key" class="fitem" :class="{ on: checked.has(f.key) || f.required }" @click="flip(f)">
              <Icon :name="f.icon" :size="16" /> <span>{{ f.label }}</span>
              <Icon v-if="checked.has(f.key) || f.required" name="check" :size="15" class="tick" />
            </button>
          </div>
        </div>
      </div>
      <footer>
        <button v-if="!base.builtin" class="btn danger" style="margin-right: auto" @click="remove"><Icon name="trash" :size="16" /> Delete template</button>
        <button class="btn" @click="state.dialog = null">Cancel</button>
        <button class="btn primary" @click="save">Save template</button>
      </footer>
    </div>
  </div>
</template>

<style scoped>
.fgrid { display: grid; grid-template-columns: repeat(auto-fill, minmax(210px, 1fr)); gap: 8px; }
.fitem { display: flex; align-items: center; gap: 9px; padding: 8px 11px; border: 1px solid var(--border); border-radius: 10px; background: var(--panel-2); color: var(--muted); text-align: left; }
.fitem:hover { background: var(--hover); }
.fitem.on { color: var(--text); border-color: var(--accent); background: var(--accent-soft); }
.fitem .tick { margin-left: auto; color: var(--accent); }
</style>
