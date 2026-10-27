<script setup>
import { computed, reactive, ref } from 'vue';
import { state, saveDraft, cancelEdit, activeTemplate, saveSettings } from '../state.js';
import { fullName, shortName, PHONE_LABELS, EMAIL_LABELS, URL_LABELS, RELATION_LABELS, IM_LABELS, SOCIAL_LABELS } from '../util.js';
import Icon from './Icon.vue';
import Avatar from './Avatar.vue';
import LabeledList from './LabeledList.vue';
import ChipsInput from './ChipsInput.vue';
import DateField from './DateField.vue';
import AddressEditor from './AddressEditor.vue';

const d = state.draft;
const isNew = state.mode === 'new';
const added = reactive(new Set()); // fields switched on by hand for this contact
const addMenu = ref(false);
const fileInput = ref(null);

// does the draft hold data for a field (then it is shown whatever the template says)?
function hasData(key, c) {
  const n = c.name;
  const any = (l) => l.some((x) => x.value);
  switch (key) {
    case 'name.prefix': return !!n.prefix;
    case 'name.middle': return !!n.middle;
    case 'name.suffix': return !!n.suffix;
    case 'name.maiden': return !!n.maiden;
    case 'name.phonetic': return !!(n.phonetic_given || n.phonetic_middle || n.phonetic_family);
    case 'nicknames': return c.nicknames.some(Boolean);
    case 'org.company': return c.orgs.some((o) => o.company);
    case 'org.department': return c.orgs.some((o) => o.department);
    case 'org.title': return c.orgs.some((o) => o.title);
    case 'phones': return any(c.phones);
    case 'emails': return any(c.emails);
    case 'addresses': return c.addresses.some((a) => a.street || a.city || a.region || a.postcode || a.country || a.po_box || a.extended);
    case 'urls': return any(c.urls);
    case 'ims': return any(c.ims);
    case 'social': return any(c.social);
    case 'keys': return any(c.keys);
    case 'birthday': return c.dates.some((x) => x.label === 'birthday');
    case 'anniversary': return c.dates.some((x) => x.label === 'anniversary');
    case 'dates.other': return c.dates.some((x) => x.label !== 'birthday' && x.label !== 'anniversary');
    case 'relations': return any(c.relations);
    case 'gender': return !!c.gender;
    case 'pronouns': return !!c.pronouns;
    case 'languages': return c.languages.some(Boolean);
    case 'timezone': return !!c.timezone;
    case 'geo': return !!c.geo;
    case 'interests': return c.interests.some(Boolean);
    case 'notes': return !!c.notes;
    case 'categories': return c.categories.some(Boolean);
    case 'custom': return any(c.custom);
    case 'kind': return c.kind === 'org';
    default: return false;
  }
}

const templateKeys = computed(() => new Set(activeTemplate.value.fields));
const on = (key) => templateKeys.value.has(key) || added.has(key) || hasData(key, d);
const defs = computed(() => state.catalog.fields);
const hidden = computed(() => defs.value.filter((f) => !f.required && !on(f.key)));
const hiddenByGroup = computed(() => {
  const m = {};
  for (const f of hidden.value) (m[f.group] ||= []).push(f);
  return m;
});
const labelOf = (key) => defs.value.find((f) => f.key === key)?.label || key;

function addField(key) {
  added.add(key);
  addMenu.value = false;
  if (key === 'birthday' || key === 'anniversary') {
    if (!d.dates.some((x) => x.label === key)) d.dates.push({ label: key, year: null, month: 0, day: 0 });
  }
  if (key.startsWith('org.') && d.orgs.length === 0) d.orgs.push({ company: '', department: '', title: '', phonetic: '' });
}
// birthday / anniversary live in `dates`; make sure the entry exists when the field is on
function dateOf(label) {
  let x = d.dates.find((v) => v.label === label);
  if (!x) { x = { label, year: null, month: 0, day: 0 }; d.dates.push(x); }
  return x;
}
const otherDates = computed(() => d.dates.filter((x) => x.label !== 'birthday' && x.label !== 'anniversary'));
function addOtherDate() { d.dates.push({ label: '', year: null, month: 0, day: 0 }); }
function dropDate(x) { d.dates.splice(d.dates.indexOf(x), 1); }
if (on('dates.other') && otherDates.value.length === 0) addOtherDate();
if (on('birthday')) dateOf('birthday');
if (on('anniversary')) dateOf('anniversary');
if ((on('org.company') || on('org.department') || on('org.title')) && d.orgs.length === 0) d.orgs.push({ company: '', department: '', title: '', phonetic: '' });

function dropEmptyDates() {
  d.dates = d.dates.filter((x) => x.month && x.day);
}
async function save() {
  dropEmptyDates();
  await saveDraft();
}

async function pickPhoto(e) {
  const f = e.target.files?.[0];
  e.target.value = '';
  if (!f) return;
  const url = URL.createObjectURL(f);
  const img = new Image();
  img.onload = () => {
    const s = Math.min(img.width, img.height);
    const c = document.createElement('canvas');
    c.width = c.height = 320;
    c.getContext('2d').drawImage(img, (img.width - s) / 2, (img.height - s) / 2, s, s, 0, 0, 320, 320);
    d.photo = c.toDataURL('image/jpeg', 0.86);
    URL.revokeObjectURL(url);
  };
  img.src = url;
}

async function setTemplate(name) {
  await saveSettings({ ...state.settings, active_template: name });
}
const labelSuggestions = computed(() => state.labels.map((l) => l.name));
const name = computed(() => fullName(d));
const anyOrg = computed(() => on('org.company') || on('org.department') || on('org.title'));
</script>

<template>
  <div class="pane-bar" data-drag>
    <button class="btn ghost" @click="cancelEdit"><Icon name="back" :size="16" /> Cancel</button>
    <span class="spacer"></span>
    <div class="row" title="Which fields the form shows">
      <Icon name="template" :size="16" class="muted" />
      <select class="select" style="width: 150px" :value="state.settings.active_template" @change="setTemplate($event.target.value)">
        <option v-for="t in state.catalog.templates" :key="t.name" :value="t.name">{{ t.name }}</option>
      </select>
      <button class="btn ghost icon" title="Choose the fields of this template…" @click="state.dialog = 'fields'"><Icon name="sliders" /></button>
    </div>
    <button class="btn primary" @click="save"><Icon name="check" :size="16" /> Save</button>
  </div>

  <div class="pane-scroll">
    <div class="pane-inner">
      <div class="hero" style="padding-bottom: 14px">
        <div style="position: relative">
          <Avatar :name="shortName(d)" :photo="d.photo" :size="96" :org="d.kind === 'org'" />
          <button class="btn icon" style="position: absolute; right: -4px; bottom: -4px; border-radius: 50%" title="Change photo" @click="fileInput.click()"><Icon name="camera" :size="15" /></button>
          <input ref="fileInput" type="file" accept="image/*" hidden @change="pickPhoto" />
        </div>
        <div style="min-width: 0">
          <h1>{{ isNew ? 'New contact' : name }}</h1>
          <button v-if="d.photo" class="btn ghost small" style="margin: 4px 0 0 -8px" @click="d.photo = null">Remove photo</button>
        </div>
      </div>

      <!-- name -->
      <div class="form-card">
        <h3><Icon name="user" :size="14" /> Name</h3>
        <div class="grid2">
          <div v-if="on('name.prefix')" class="field"><label>Title</label><input class="input" placeholder="Dr., Ms." v-model="d.name.prefix" /></div>
          <div class="field"><label>First name</label><input class="input" v-model="d.name.given" autofocus /></div>
          <div v-if="on('name.middle')" class="field"><label>Middle name</label><input class="input" v-model="d.name.middle" /></div>
          <div class="field"><label>Last name</label><input class="input" v-model="d.name.family" /></div>
          <div v-if="on('name.suffix')" class="field"><label>Suffix</label><input class="input" placeholder="Jr., PhD" v-model="d.name.suffix" /></div>
          <div v-if="on('name.maiden')" class="field"><label>Maiden name</label><input class="input" v-model="d.name.maiden" /></div>
        </div>
        <div v-if="on('name.phonetic')" class="field">
          <label>Phonetic name (how it is pronounced)</label>
          <div class="grid3">
            <input class="input" placeholder="First" v-model="d.name.phonetic_given" />
            <input class="input" placeholder="Middle" v-model="d.name.phonetic_middle" />
            <input class="input" placeholder="Last" v-model="d.name.phonetic_family" />
          </div>
        </div>
        <div v-if="on('nicknames')" class="field"><label>Nickname</label><ChipsInput :model-value="d.nicknames" placeholder="Type and press Enter" /></div>
      </div>

      <!-- work -->
      <div v-if="anyOrg" class="form-card">
        <h3><Icon name="briefcase" :size="14" /> Work</h3>
        <div v-for="(o, i) in d.orgs" :key="i" class="orgrow">
          <div class="grid3">
            <input v-if="on('org.company')" class="input" placeholder="Company" v-model="o.company" />
            <input v-if="on('org.department')" class="input" placeholder="Department" v-model="o.department" />
            <input v-if="on('org.title')" class="input" placeholder="Job title" v-model="o.title" />
          </div>
          <button v-if="d.orgs.length > 1" class="btn ghost icon" title="Remove" @click="d.orgs.splice(i, 1)"><Icon name="x" :size="16" /></button>
        </div>
        <button class="btn ghost small" @click="d.orgs.push({ company: '', department: '', title: '', phonetic: '' })"><Icon name="plus" :size="14" /> Add another job</button>
      </div>

      <!-- ways to reach -->
      <div v-if="on('phones')" class="form-card"><h3><Icon name="phone" :size="14" /> Phone</h3>
        <LabeledList :model-value="d.phones" :labels="PHONE_LABELS" default-label="mobile" placeholder="+1 555 0100" add-text="Add phone" />
      </div>
      <div v-if="on('emails')" class="form-card"><h3><Icon name="mail" :size="14" /> Email</h3>
        <LabeledList :model-value="d.emails" :labels="EMAIL_LABELS" default-label="home" placeholder="name@example.com" add-text="Add email" />
      </div>
      <div v-if="on('addresses')" class="form-card"><h3><Icon name="map-pin" :size="14" /> Address</h3>
        <AddressEditor :model-value="d.addresses" />
      </div>
      <div v-if="on('urls')" class="form-card"><h3><Icon name="globe" :size="14" /> Website</h3>
        <LabeledList :model-value="d.urls" :labels="URL_LABELS" default-label="homepage" placeholder="https://" add-text="Add website" />
      </div>
      <div v-if="on('ims')" class="form-card"><h3><Icon name="message-circle" :size="14" /> Messengers</h3>
        <LabeledList :model-value="d.ims" :labels="IM_LABELS" default-label="Signal" placeholder="Username or number" label-placeholder="Service" add-text="Add messenger" />
      </div>
      <div v-if="on('social')" class="form-card"><h3><Icon name="at-sign" :size="14" /> Social profiles</h3>
        <LabeledList :model-value="d.social" :labels="SOCIAL_LABELS" default-label="mastodon" placeholder="@name or link" label-placeholder="Network" add-text="Add profile" />
      </div>
      <div v-if="on('keys')" class="form-card"><h3><Icon name="key" :size="14" /> Keys</h3>
        <LabeledList :model-value="d.keys" :labels="['pgp', 'ssh', 'age']" default-label="pgp" placeholder="Fingerprint or link" add-text="Add key" />
      </div>

      <!-- personal -->
      <div v-if="on('birthday') || on('anniversary') || on('dates.other')" class="form-card"><h3><Icon name="cake" :size="14" /> Dates</h3>
        <div v-if="on('birthday')" class="field"><label>Birthday</label><DateField :model-value="dateOf('birthday')" /></div>
        <div v-if="on('anniversary')" class="field"><label>Anniversary</label><DateField :model-value="dateOf('anniversary')" /></div>
        <template v-if="on('dates.other')">
          <div v-for="x in otherDates" :key="d.dates.indexOf(x)" class="field">
            <div class="row"><input class="input" style="width: 170px" placeholder="What is it? (first met…)" v-model="x.label" /><span class="spacer"></span>
              <button class="btn ghost icon small" @click="dropDate(x)"><Icon name="x" :size="16" /></button></div>
            <DateField :model-value="x" />
          </div>
          <button class="btn ghost small" @click="addOtherDate"><Icon name="plus" :size="14" /> Add date</button>
        </template>
      </div>
      <div v-if="on('relations')" class="form-card"><h3><Icon name="users" :size="14" /> Related people</h3>
        <LabeledList :model-value="d.relations" :labels="RELATION_LABELS" default-label="spouse" placeholder="Name" label-placeholder="Relation" add-text="Add person" />
      </div>
      <div v-if="on('gender') || on('pronouns') || on('languages') || on('timezone') || on('geo') || on('interests')" class="form-card">
        <h3><Icon name="user-round" :size="14" /> About</h3>
        <div class="grid2">
          <div v-if="on('gender')" class="field"><label>Gender</label><input class="input" v-model="d.gender" /></div>
          <div v-if="on('pronouns')" class="field"><label>Pronouns</label><input class="input" placeholder="she/her, they/them" v-model="d.pronouns" /></div>
          <div v-if="on('timezone')" class="field"><label>Time zone</label><input class="input" placeholder="Europe/Berlin" v-model="d.timezone" /></div>
          <div v-if="on('geo')" class="field"><label>Location (latitude, longitude)</label><input class="input" placeholder="52.52, 13.40" v-model="d.geo" /></div>
        </div>
        <div v-if="on('languages')" class="field"><label>Languages</label><ChipsInput :model-value="d.languages" placeholder="Type and press Enter" /></div>
        <div v-if="on('interests')" class="field"><label>Interests</label><ChipsInput :model-value="d.interests" placeholder="Type and press Enter" /></div>
      </div>

      <!-- other -->
      <div v-if="on('categories')" class="form-card"><h3><Icon name="tag" :size="14" /> Labels</h3>
        <ChipsInput :model-value="d.categories" placeholder="Family, Work…" :suggestions="labelSuggestions" />
      </div>
      <div v-if="on('notes')" class="form-card"><h3><Icon name="notebook-pen" :size="14" /> Notes</h3>
        <textarea class="textarea" v-model="d.notes" />
      </div>
      <div v-if="on('custom')" class="form-card"><h3><Icon name="plus-square" :size="14" /> Custom fields</h3>
        <LabeledList :model-value="d.custom" default-label="" placeholder="Value" label-placeholder="Name" add-text="Add field" />
      </div>
      <div v-if="on('kind')" class="form-card">
        <label class="row" style="cursor: pointer"><input type="checkbox" :checked="d.kind === 'org'" @change="d.kind = $event.target.checked ? 'org' : ''" />
          Company or place (show by its company name, not a person)</label>
      </div>

      <!-- more fields -->
      <div style="position: relative; margin: 6px 0 4px">
        <button class="btn" @click="addMenu = !addMenu"><Icon name="plus" :size="16" /> Add field</button>
        <div v-if="addMenu" class="pop" style="left: 0; bottom: 42px; width: 290px" @mouseleave="addMenu = false">
          <div v-if="!hidden.length" class="muted" style="padding: 10px">Every field is already shown.</div>
          <template v-for="(fs, g) in hiddenByGroup" :key="g">
            <div class="nav-title" style="padding-top: 8px">{{ g }}</div>
            <button v-for="f in fs" :key="f.key" class="nav-item" @click="addField(f.key)"><Icon :name="f.icon" :size="16" /> {{ f.label }}</button>
          </template>
        </div>
      </div>
      <div class="sticky-save">
        <button class="btn primary" @click="save"><Icon name="check" :size="16" /> Save</button>
        <button class="btn" @click="cancelEdit">Cancel</button>
        <span class="faint" style="font-size: 12.5px">Fields filled in are always shown, whatever the template.</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.orgrow { display: flex; gap: 8px; align-items: flex-start; margin-bottom: 8px; }
.orgrow .grid3 { flex: 1; }
</style>
