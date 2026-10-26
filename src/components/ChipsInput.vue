<script setup>
import { ref } from 'vue';
import Icon from './Icon.vue';

const props = defineProps({ modelValue: Array, placeholder: String, suggestions: { type: Array, default: () => [] } });
const text = ref('');
const id = 'ch' + Math.random().toString(36).slice(2, 8);
function commit() {
  for (const t of text.value.split(',').map((s) => s.trim()).filter(Boolean)) {
    if (!props.modelValue.includes(t)) props.modelValue.push(t);
  }
  text.value = '';
}
function key(e) {
  if (e.key === 'Enter' || e.key === ',') { e.preventDefault(); commit(); }
  else if (e.key === 'Backspace' && !text.value && props.modelValue.length) props.modelValue.pop();
}
</script>

<template>
  <div class="chips input" @click="$refs.inp.focus()">
    <span v-for="(c, i) in modelValue.filter(Boolean)" :key="c" class="chip">{{ c }} <button tabindex="-1" @click.stop="modelValue.splice(i, 1)"><Icon name="x" :size="12" /></button></span>
    <input ref="inp" :list="id" :placeholder="modelValue.length ? '' : placeholder" v-model="text" @keydown="key" @blur="commit" />
    <datalist :id="id"><option v-for="s in suggestions.filter((x) => !modelValue.includes(x))" :key="s" :value="s" /></datalist>
  </div>
</template>

<style scoped>
.chips { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; padding: 5px 8px; min-height: 40px; }
.chips input { flex: 1; min-width: 90px; background: none; border: 0; outline: none; padding: 3px 2px; }
.chip button { display: grid; place-items: center; color: inherit; opacity: .7; }
.chip button:hover { opacity: 1; }
</style>
