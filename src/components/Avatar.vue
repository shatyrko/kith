<script setup>
import { computed } from 'vue';
const props = defineProps({ name: String, photo: String, size: { type: Number, default: 40 }, org: Boolean });
const hues = [262, 215, 190, 160, 35, 350, 295, 330];
const style = computed(() => {
  let h = 0;
  for (const c of props.name || '?') h = (h * 31 + c.charCodeAt(0)) >>> 0;
  const hue = hues[h % hues.length];
  return {
    width: props.size + 'px',
    height: props.size + 'px',
    fontSize: Math.round(props.size * 0.38) + 'px',
    background: `linear-gradient(135deg, hsl(${hue} 62% 58%), hsl(${(hue + 28) % 360} 60% 48%))`,
  };
});
const initials = computed(() => {
  const w = (props.name || '?').replace(/[^\p{L}\p{N} ]/gu, '').split(/\s+/).filter(Boolean);
  if (w.length === 0) return '?';
  if (w.length === 1) return w[0].slice(0, 1).toUpperCase();
  return (w[0][0] + w[w.length - 1][0]).toUpperCase();
});
</script>
<template>
  <div class="avatar" :class="{ org }" :style="photo ? { width: size + 'px', height: size + 'px' } : style">
    <img v-if="photo" :src="photo" alt="" draggable="false" />
    <template v-else>{{ initials }}</template>
  </div>
</template>
