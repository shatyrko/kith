<script setup>
import { inTauri, win } from '../window.js';
const dirs = { n: 'North', s: 'South', w: 'West', e: 'East', nw: 'NorthWest', ne: 'NorthEast', sw: 'SouthWest', se: 'SouthEast' };
const grab = (e, d) => { if (e.button === 0) win.startResize(dirs[d]); };
</script>

<template>
  <template v-if="inTauri">
    <div v-for="(_, d) in dirs" :key="d" class="rz" :class="d" @mousedown.prevent="grab($event, d)"></div>
  </template>
</template>
