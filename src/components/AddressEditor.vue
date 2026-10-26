<script setup>
import { watchEffect } from 'vue';
import Icon from './Icon.vue';
import { ADDRESS_LABELS } from '../util.js';

const props = defineProps({ modelValue: Array });
const blank = () => ({ label: 'home', po_box: '', extended: '', street: '', city: '', region: '', postcode: '', country: '', pref: false });
watchEffect(() => { if (props.modelValue.length === 0) props.modelValue.push(blank()); });
</script>

<template>
  <div>
    <datalist id="adr-labels"><option v-for="l in ADDRESS_LABELS" :key="l" :value="l" /></datalist>
    <div v-for="(a, i) in modelValue" :key="i" class="addr">
      <div class="row" style="margin-bottom: 8px">
        <input class="input" style="width: 140px" list="adr-labels" placeholder="Label" v-model="a.label" />
        <span class="spacer"></span>
        <button class="btn ghost icon small" title="Remove" @click="modelValue.splice(i, 1)"><Icon name="x" :size="16" /></button>
      </div>
      <input class="input" placeholder="Street and number" v-model="a.street" style="margin-bottom: 8px" />
      <div class="grid2" style="margin-bottom: 8px">
        <input class="input" placeholder="Apartment, building, c/o" v-model="a.extended" />
        <input class="input" placeholder="PO box" v-model="a.po_box" />
      </div>
      <div class="grid3" style="margin-bottom: 8px">
        <input class="input" placeholder="Postcode" v-model="a.postcode" />
        <input class="input" placeholder="City" v-model="a.city" />
        <input class="input" placeholder="State / region" v-model="a.region" />
      </div>
      <input class="input" placeholder="Country" v-model="a.country" />
    </div>
    <button class="btn ghost small" @click="modelValue.push(blank())"><Icon name="plus" :size="14" /> Add address</button>
  </div>
</template>

<style scoped>
.addr { padding-bottom: 12px; margin-bottom: 12px; border-bottom: 1px dashed var(--border); }
.addr:last-of-type { border-bottom: 0; }
</style>
