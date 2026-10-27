<script setup>
import { computed } from 'vue';
import {
  User, Badge, Speech, Smile, Building, Layers, Briefcase, Phone, Mail, MapPin, Globe, MessageCircle, AtSign, Key, Cake,
  Heart, Calendar, Users, UserRound, MessageSquare, Languages, Clock, Map, Sparkles, NotebookPen, Tag, SquarePlus, Building2,
  Search, Plus, Star, Pencil, Trash2, X, Check, ChevronDown, ChevronRight, Settings, Cloud, CloudOff, RefreshCw, Download,
  Upload, Copy, Ellipsis, UsersRound, History, Folder, Contact, Merge, Undo2, SlidersHorizontal, LayoutTemplate, Save,
  ExternalLink, Sun, Moon, Monitor, Camera, ImagePlus, Info, TriangleAlert, ArrowLeft, Link, Hash, Clock3, ShieldCheck, Trash, Minus, Square,
} from 'lucide-vue-next';

const icons = {
  user: User, badge: Badge, speech: Speech, smile: Smile, building: Building, layers: Layers, briefcase: Briefcase, phone: Phone,
  mail: Mail, 'map-pin': MapPin, globe: Globe, 'message-circle': MessageCircle, 'at-sign': AtSign, key: Key, cake: Cake,
  heart: Heart, calendar: Calendar, users: Users, 'user-round': UserRound, 'message-square': MessageSquare, languages: Languages,
  clock: Clock, map: Map, sparkles: Sparkles, 'notebook-pen': NotebookPen, tag: Tag, 'plus-square': SquarePlus, 'building-2': Building2,
  search: Search, plus: Plus, star: Star, pencil: Pencil, trash: Trash2, 'trash-empty': Trash, x: X, check: Check, 'chevron-down': ChevronDown,
  'chevron-right': ChevronRight, settings: Settings, cloud: Cloud, 'cloud-off': CloudOff, refresh: RefreshCw, download: Download,
  upload: Upload, copy: Copy, more: Ellipsis, 'users-round': UsersRound, history: History, folder: Folder, contact: Contact,
  merge: Merge, undo: Undo2, sliders: SlidersHorizontal, template: LayoutTemplate, save: Save, external: ExternalLink, sun: Sun,
  moon: Moon, monitor: Monitor, camera: Camera, 'image-plus': ImagePlus, info: Info, warn: TriangleAlert, back: ArrowLeft, link: Link,
  hash: Hash, clock3: Clock3, shield: ShieldCheck, minus: Minus, square: Square,
};
const props = defineProps({ name: String, size: { type: Number, default: 18 }, fill: Boolean });
const comp = computed(() => icons[props.name] || Hash);
</script>
<template>
  <component :is="comp" :size="size" :stroke-width="1.8" :fill="fill ? 'currentColor' : 'none'" />
</template>
