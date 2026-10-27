<script setup>
import { onMounted, onBeforeUnmount, ref } from 'vue';
import Icon from './Icon.vue';
import { inTauri, win } from '../window.js';

const maximized = ref(false);
const show = inTauri || location.search.includes('chrome');
let off;
async function refresh() { maximized.value = await win.isMaximized(); }
onMounted(async () => { refresh(); off = await win.onResized(refresh); });
onBeforeUnmount(() => off && off());
</script>

<template>
  <div v-if="show" class="winctl" data-nodrag>
    <button title="Minimise" @click="win.minimize()"><Icon name="minus" :size="16" /></button>
    <button :title="maximized ? 'Restore' : 'Maximise'" @click="win.toggleMaximize()"><Icon :name="maximized ? 'copy' : 'square'" :size="maximized ? 14 : 13" /></button>
    <button class="close" title="Close" @click="win.close()"><Icon name="x" :size="17" /></button>
  </div>
</template>
