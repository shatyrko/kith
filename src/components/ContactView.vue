<script setup>
import { computed, ref } from 'vue';
import { state, startEdit, toggleFavorite, removeContact, exportOne, notify, theApi } from '../state.js';
import { fullName, shortName, formatDate, countdown, addressLines, hasAny, copy } from '../util.js';
import { vContext } from '../contextmenu.js';
import Icon from './Icon.vue';
import Avatar from './Avatar.vue';

const c = computed(() => state.contact);
const menu = ref(false);
const name = computed(() => fullName(c.value));
const org = computed(() => c.value.orgs.filter((o) => o.company || o.title || o.department).map((o) => [o.title, o.department, o.company].filter(Boolean).join(' · ')).join('  /  '));
const dates = computed(() => c.value.dates.filter((d) => d.month));
const personal = computed(() => {
  const x = c.value;
  return [
    ['Gender', x.gender], ['Pronouns', x.pronouns], ['Languages', x.languages.filter(Boolean).join(', ')], ['Time zone', x.timezone],
    ['Location', x.geo], ['Interests', x.interests.filter(Boolean).join(', ')],
  ].filter(([, v]) => v);
});
const nameDetails = computed(() => {
  const n = c.value.name;
  return [
    ['Maiden name', n.maiden], ['Pronounced', [n.phonetic_given, n.phonetic_middle, n.phonetic_family].filter(Boolean).join(' ')],
  ].filter(([, v]) => v);
});

async function doCopy(text) {
  notify((await copy(text)) ? 'Copied' : 'Could not copy', 'info');
}
// right-click menu of a value row: always Copy, plus the one thing that fits the kind of value
function rowMenu(kind, value, label) {
  const items = [{ label: 'Copy', icon: 'copy', action: () => doCopy(value) }];
  if (kind === 'email') items.push({ label: 'Write an email', icon: 'mail', action: () => open('mailto:' + value) });
  if (kind === 'url') items.push({ label: 'Open link', icon: 'external', action: () => open(web(value)) });
  if (kind === 'address') items.push({ label: 'Show on the map', icon: 'map', action: () => open('https://www.openstreetmap.org/search?query=' + encodeURIComponent(value.replace(/\n/g, ', '))) });
  if (label) items.push({ separator: true }, { label: 'Copy with label', icon: 'copy', action: () => doCopy(`${label}: ${value}`) });
  return items;
}
function open(url) { theApi().openUrl(url); }
function web(v) { return /^[a-z][a-z0-9+.-]*:/i.test(v) ? v : 'https://' + v; }
function mapUrl(a) { return 'https://www.openstreetmap.org/search?query=' + encodeURIComponent(addressLines(a).replace(/\n/g, ', ')); }
const firstEmail = computed(() => c.value.emails.find((e) => e.value));
</script>

<template>
  <div class="pane-bar" data-drag>
    <span class="spacer"></span>
    <button class="btn ghost icon" :title="c.favorite ? 'Remove from favorites' : 'Add to favorites'" @click="toggleFavorite(c.id, !c.favorite)">
      <Icon name="star" :fill="c.favorite" :style="{ color: c.favorite ? 'var(--warn)' : '' }" />
    </button>
    <button class="btn" title="Edit (Ctrl+E)" @click="startEdit"><Icon name="pencil" :size="16" /> Edit</button>
    <div style="position: relative">
      <button class="btn ghost icon" @click="menu = !menu"><Icon name="more" /></button>
      <div v-if="menu" class="pop" style="right: 0; top: 40px" @mouseleave="menu = false">
        <button class="nav-item" @click="menu = false; exportOne(c.id, name)"><Icon name="upload" /> Export as vCard…</button>
        <button class="nav-item" @click="menu = false; doCopy(name)"><Icon name="copy" /> Copy name</button>
        <button class="nav-item" style="color: var(--danger)" @click="menu = false; removeContact(c.id)"><Icon name="trash" /> Delete</button>
      </div>
    </div>
  </div>
  <div class="pane-scroll">
    <div class="pane-inner">
      <div class="hero">
        <Avatar :name="shortName(c)" :photo="c.photo" :size="96" :org="c.kind === 'org'" />
        <div style="min-width: 0">
          <h1 class="selectable" v-context="() => rowMenu('text', name)">{{ name }}</h1>
          <div v-if="org" class="org selectable">{{ org }}</div>
          <div v-if="c.nicknames.filter(Boolean).length" class="nick">“{{ c.nicknames.filter(Boolean).join('”, “') }}”</div>
          <div v-if="c.categories.length" class="row" style="margin-top: 10px; flex-wrap: wrap">
            <span v-for="l in c.categories" :key="l" class="chip"><Icon name="tag" :size="12" /> {{ l }}</span>
          </div>
        </div>
      </div>

      <div v-if="firstEmail || c.phones.find((p) => p.value)" class="quick">
        <button v-if="c.phones.find((p) => p.value)" class="btn" @click="doCopy(c.phones.find((p) => p.value).value)"><Icon name="phone" :size="16" /> Copy number</button>
        <button v-if="firstEmail" class="btn" @click="open('mailto:' + firstEmail.value)"><Icon name="mail" :size="16" /> Email</button>
      </div>

      <div v-if="hasAny(c.phones)" class="card">
        <div class="card-title"><Icon name="phone" :size="14" /> Phone</div>
        <div v-for="(p, i) in c.phones.filter((x) => x.value)" :key="'p' + i" class="kv" v-context="() => rowMenu('phone', p.value, p.label)">
          <div class="k">{{ p.label || 'phone' }}</div><div class="v selectable">{{ p.value }}</div>
          <Icon v-if="p.pref" name="star" :size="13" fill class="pref" />
          <div class="acts"><button class="btn ghost icon small" title="Copy" @click="doCopy(p.value)"><Icon name="copy" :size="15" /></button></div>
        </div>
      </div>

      <div v-if="hasAny(c.emails)" class="card">
        <div class="card-title"><Icon name="mail" :size="14" /> Email</div>
        <div v-for="(e, i) in c.emails.filter((x) => x.value)" :key="'e' + i" class="kv" v-context="() => rowMenu('email', e.value, e.label)">
          <div class="k">{{ e.label || 'email' }}</div><div class="v selectable">{{ e.value }}</div>
          <Icon v-if="e.pref" name="star" :size="13" fill class="pref" />
          <div class="acts">
            <button class="btn ghost icon small" title="Copy" @click="doCopy(e.value)"><Icon name="copy" :size="15" /></button>
            <button class="btn ghost icon small" title="Write an email" @click="open('mailto:' + e.value)"><Icon name="external" :size="15" /></button>
          </div>
        </div>
      </div>

      <div v-if="c.addresses.some((a) => addressLines(a))" class="card">
        <div class="card-title"><Icon name="map-pin" :size="14" /> Address</div>
        <div v-for="(a, i) in c.addresses.filter((x) => addressLines(x))" :key="'a' + i" class="kv" v-context="() => rowMenu('address', addressLines(a), a.label)">
          <div class="k">{{ a.label || 'address' }}</div><div class="v selectable">{{ addressLines(a) }}</div>
          <div class="acts">
            <button class="btn ghost icon small" title="Copy" @click="doCopy(addressLines(a))"><Icon name="copy" :size="15" /></button>
            <button class="btn ghost icon small" title="Show on the map" @click="open(mapUrl(a))"><Icon name="map" :size="15" /></button>
          </div>
        </div>
      </div>

      <div v-if="hasAny(c.urls) || hasAny(c.ims) || hasAny(c.social)" class="card">
        <div class="card-title"><Icon name="globe" :size="14" /> Online</div>
        <div v-for="(u, i) in c.urls.filter((x) => x.value)" :key="'u' + i" class="kv" v-context="() => rowMenu('url', u.value, u.label)">
          <div class="k">{{ u.label || 'website' }}</div><div class="v selectable">{{ u.value }}</div>
          <div class="acts"><button class="btn ghost icon small" title="Open" @click="open(web(u.value))"><Icon name="external" :size="15" /></button></div>
        </div>
        <div v-for="(u, i) in c.ims.filter((x) => x.value)" :key="'i' + i" class="kv" v-context="() => rowMenu('text', u.value, u.label)">
          <div class="k">{{ u.label || 'messenger' }}</div><div class="v selectable">{{ u.value }}</div>
          <div class="acts"><button class="btn ghost icon small" title="Copy" @click="doCopy(u.value)"><Icon name="copy" :size="15" /></button></div>
        </div>
        <div v-for="(u, i) in c.social.filter((x) => x.value)" :key="'s' + i" class="kv" v-context="() => rowMenu('text', u.value, u.label)">
          <div class="k">{{ u.label || 'profile' }}</div><div class="v selectable">{{ u.value }}</div>
          <div class="acts"><button class="btn ghost icon small" title="Copy" @click="doCopy(u.value)"><Icon name="copy" :size="15" /></button></div>
        </div>
      </div>

      <div v-if="dates.length" class="card">
        <div class="card-title"><Icon name="cake" :size="14" /> Dates</div>
        <div v-for="(d, i) in dates" :key="'d' + i" class="kv">
          <div class="k">{{ d.label || 'date' }}</div>
          <div class="v">{{ formatDate(d) }} <span v-if="countdown(d, d.label)" class="chip" style="margin-left: 8px">{{ countdown(d, d.label) }}</span></div>
        </div>
      </div>

      <div v-if="hasAny(c.relations)" class="card">
        <div class="card-title"><Icon name="users" :size="14" /> Related people</div>
        <div v-for="(r, i) in c.relations.filter((x) => x.value)" :key="'r' + i" class="kv" v-context="() => rowMenu('text', r.value, r.label)">
          <div class="k">{{ r.label }}</div><div class="v selectable">{{ r.value }}</div>
        </div>
      </div>

      <div v-if="personal.length || nameDetails.length" class="card">
        <div class="card-title"><Icon name="user-round" :size="14" /> About</div>
        <div v-for="[k, v] in [...nameDetails, ...personal]" :key="k" class="kv" v-context="() => rowMenu('text', v, k)"><div class="k">{{ k }}</div><div class="v selectable">{{ v }}</div></div>
      </div>

      <div v-if="hasAny(c.custom) || hasAny(c.keys)" class="card">
        <div class="card-title"><Icon name="plus-square" :size="14" /> More</div>
        <div v-for="(f, i) in c.custom.filter((x) => x.value)" :key="'c' + i" class="kv"><div class="k">{{ f.label || 'field' }}</div><div class="v selectable">{{ f.value }}</div></div>
        <div v-for="(f, i) in c.keys.filter((x) => x.value)" :key="'k' + i" class="kv"><div class="k">{{ f.label || 'key' }}</div><div class="v selectable">{{ f.value }}</div></div>
      </div>

      <div v-if="c.notes" class="card">
        <div class="card-title"><Icon name="notebook-pen" :size="14" /> Notes</div>
        <div class="kv" style="display: block"><div class="v selectable">{{ c.notes }}</div></div>
      </div>
    </div>
  </div>
</template>
