<script setup>
import { watchEffect } from 'vue';
import Icon from './Icon.vue';

const props = defineProps({
  modelValue: Array,
  labels: { type: Array, default: () => [] },
  addText: { type: String, default: 'Add' },
  placeholder: { type: String, default: '' },
  labelPlaceholder: { type: String, default: 'Label' },
  defaultLabel: { type: String, default: '' },
  multiline: Boolean,
  id: { type: String, default: () => 'll' + Math.random().toString(36).slice(2, 8) },
  noPref: Boolean,
});
// always one empty row to type into
watchEffect(() => {
  if (props.modelValue.length === 0) props.modelValue.push({ label: props.defaultLabel, value: '', pref: false });
});
function add() { props.modelValue.push({ label: props.defaultLabel, value: '', pref: false }); }
function remove(i) { props.modelValue.splice(i, 1); }
</script>

<template>
  <div>
    <datalist :id="id"><option v-for="l in labels" :key="l" :value="l" /></datalist>
    <div v-for="(row, i) in modelValue" :key="i" class="lrow">
      <input class="input" :list="id" :placeholder="labelPlaceholder" v-model="row.label" />
      <textarea v-if="multiline" class="textarea" style="min-height: 64px" :placeholder="placeholder" v-model="row.value" />
      <input v-else class="input" :placeholder="placeholder" v-model="row.value" />
      <button class="btn ghost icon" title="Remove" tabindex="-1" @click="remove(i)"><Icon name="x" :size="16" /></button>
    </div>
    <button class="btn ghost small" @click="add"><Icon name="plus" :size="14" /> {{ addText }}</button>
  </div>
</template>
