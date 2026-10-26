<script setup>
import { MONTHS } from '../util.js';
// a date with an optional year: { month, day, year }
const props = defineProps({ modelValue: Object });
function year(e) {
  const v = parseInt(e.target.value, 10);
  props.modelValue.year = Number.isFinite(v) && v > 0 ? v : null;
}
function day(e) {
  const v = parseInt(e.target.value, 10);
  props.modelValue.day = Number.isFinite(v) ? Math.min(31, Math.max(0, v)) : 0;
}
</script>

<template>
  <div class="dt">
    <input class="input" type="number" min="1" max="31" placeholder="Day" :value="modelValue.day || ''" @input="day" />
    <select class="select" v-model.number="modelValue.month">
      <option :value="0">Month</option>
      <option v-for="(m, i) in MONTHS" :key="m" :value="i + 1">{{ m }}</option>
    </select>
    <input class="input" type="number" min="1" max="2100" placeholder="Year (optional)" :value="modelValue.year || ''" @input="year" />
  </div>
</template>

<style scoped>
.dt { display: grid; grid-template-columns: 80px 1fr 150px; gap: 8px; }
</style>
