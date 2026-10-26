<script setup>
import { reactive, ref, computed } from 'vue';
import { state, saveSettings, notify, googleSync, refreshGoogle, theApi } from '../state.js';
import Icon from './Icon.vue';

const s = reactive(JSON.parse(JSON.stringify(state.settings)));
const report = ref(null);
const busy = ref('');
const g = computed(() => state.google);

async function apply() {
  await saveSettings(JSON.parse(JSON.stringify(s)));
}
async function setTheme(t) { s.theme = t; await apply(); }
async function setSort(v) { s.sort_by = v; await apply(); }
async function setOrder(v) { s.display_order = v; await apply(); }

async function saveClient() {
  await apply();
  await refreshGoogle();
  notify('Google settings saved');
}
async function connect() {
  busy.value = 'login';
  try {
    await apply();
    await theApi().googleLogin();
    await refreshGoogle();
    notify('Connected to Google');
  } catch (e) {
    notify(String(e?.message || e), 'error');
  } finally {
    busy.value = '';
  }
}
async function sync(dry) {
  busy.value = dry ? 'dry' : 'sync';
  report.value = (await googleSync(dry)) || null;
  report.value && (report.value.dry = dry);
  busy.value = '';
}
async function signOut() {
  await theApi().googleLogout();
  await refreshGoogle();
  report.value = null;
}
async function toggleDeletes() {
  s.google.allow_deletes = !s.google.allow_deletes;
  await apply();
  await refreshGoogle();
}
async function setAuto(e) {
  s.google.auto_sync_minutes = parseInt(e.target.value, 10) || 0;
  await apply();
}
async function toggleTwoWay() {
  s.google.two_way = !s.google.two_way;
  await apply();
  await refreshGoogle();
  // turning it on: show at once what would be sent; nothing is sent before you press "Sync now"
  if (s.google.two_way) await sync(true);
  else report.value = null;
}
const docs = 'https://github.com/shatyrko/kith/blob/main/docs/google-setup.md';
</script>

<template>
  <div class="scrim" @mousedown.self="state.dialog = null">
    <div class="dialog" style="width: min(720px, calc(100vw - 48px))">
      <header><Icon name="settings" /><h2>Settings</h2><span class="spacer"></span>
        <button class="btn ghost icon" @click="state.dialog = null"><Icon name="x" /></button></header>
      <div class="body">
        <div class="nav-title" style="padding-left: 0">Appearance</div>
        <div class="row" style="margin-bottom: 14px"><span style="width: 150px">Theme</span>
          <div class="seg">
            <button :class="{ on: s.theme === 'system' }" @click="setTheme('system')"><Icon name="monitor" :size="15" /> System</button>
            <button :class="{ on: s.theme === 'light' }" @click="setTheme('light')"><Icon name="sun" :size="15" /> Light</button>
            <button :class="{ on: s.theme === 'dark' }" @click="setTheme('dark')"><Icon name="moon" :size="15" /> Dark</button>
          </div>
        </div>
        <div class="row" style="margin-bottom: 14px"><span style="width: 150px">Sort by</span>
          <div class="seg"><button :class="{ on: s.sort_by === 'family' }" @click="setSort('family')">Last name</button><button :class="{ on: s.sort_by === 'given' }" @click="setSort('given')">First name</button></div>
        </div>
        <div class="row" style="margin-bottom: 6px"><span style="width: 150px">Show names as</span>
          <div class="seg"><button :class="{ on: s.display_order === 'given' }" @click="setOrder('given')">Jane Doe</button><button :class="{ on: s.display_order === 'family' }" @click="setOrder('family')">Doe, Jane</button></div>
        </div>

        <div class="nav-title" style="padding-left: 0; margin-top: 10px">Google Contacts</div>
        <div v-if="!g.signed_in">
          <p class="muted" style="margin: 0 0 12px">
            Sync with your own Google account, so every phone and computer shares one address book.
            You need an OAuth client of your own (a few minutes, once):
            <a href="#" @click.prevent="theApi().openUrl(docs)" style="color: var(--accent)">step-by-step guide</a>.
          </p>
          <div class="field"><label>Client ID</label><input class="input" placeholder="…apps.googleusercontent.com" v-model="s.google.client_id" /></div>
          <div class="field"><label>Client secret</label><input class="input" type="password" v-model="s.google.client_secret" /></div>
          <div class="row">
            <button class="btn" @click="saveClient">Save</button>
            <button class="btn primary" :disabled="!s.google.client_id || !s.google.client_secret || busy === 'login'" @click="connect">
              <Icon name="cloud" :size="16" /> {{ busy === 'login' ? 'Waiting for the browser…' : 'Connect Google account' }}</button>
          </div>
        </div>
        <div v-else>
          <div class="row" style="margin-bottom: 12px"><Icon name="shield" style="color: var(--ok)" /> Connected as <b>{{ g.account || 'your Google account' }}</b>
            <span class="spacer"></span><button class="btn small" @click="signOut">Disconnect</button></div>
          <div class="row" style="padding: 10px 12px; border: 1px solid var(--border); border-radius: 12px; margin-bottom: 12px">
            <div style="flex: 1"><div style="font-weight: 600">Send my changes to Google</div>
              <div class="muted" style="font-size: 12.5px">Off: Kith only reads from Google and never changes it. Turn it on after a first sync looks right; you then see a preview of what would be sent, and nothing leaves until you press “Sync now”.</div></div>
            <button class="switch" :class="{ on: s.google.two_way }" @click="toggleTwoWay"></button>
          </div>
          <div v-if="s.google.two_way" class="row" style="padding: 10px 12px; border: 1px solid var(--border); border-radius: 12px; margin-bottom: 12px">
            <div style="flex: 1"><div style="font-weight: 600">Also delete on Google what I delete here</div>
              <div class="muted" style="font-size: 12.5px">Off: deleted contacts stay on Google (and come back on the next sync). Google keeps deleted contacts 30 days.</div></div>
            <button class="switch" :class="{ on: s.google.allow_deletes }" @click="toggleDeletes"></button>
          </div>
          <div class="row" style="margin-bottom: 12px"><span style="flex: 1">Sync automatically while Kith is open</span>
            <select class="select" style="width: 150px" :value="s.google.auto_sync_minutes" @change="setAuto">
              <option :value="0">Never</option><option :value="15">Every 15 min</option><option :value="30">Every 30 min</option><option :value="60">Every hour</option>
            </select></div>
          <div class="row">
            <button class="btn" :disabled="!!busy" @click="sync(true)"><Icon name="search" :size="16" /> {{ busy === 'dry' ? 'Checking…' : 'Preview what a sync would do' }}</button>
            <button class="btn primary" :disabled="!!busy" @click="sync(false)"><Icon name="refresh" :size="16" /> {{ busy === 'sync' ? 'Syncing…' : 'Sync now' }}</button>
          </div>
          <div v-if="report" class="card" style="margin-top: 12px; padding: 12px 14px">
            <div style="font-weight: 600; margin-bottom: 6px">{{ report.dry ? 'Preview (nothing was changed)' : 'Sync finished' }}</div>
            <div class="muted selectable">
              From Google: {{ report.pulled_new }} new, {{ report.pulled_updated }} updated, {{ report.pulled_deleted }} removed.
              To Google: {{ report.pushed_new }} new, {{ report.pushed_updated }} updated, {{ report.pushed_deleted }} removed.
              <span v-if="report.conflicts"> {{ report.conflicts }} conflict(s): Google's version won, yours is kept in the conflicts folder.</span>
            </div>
            <div v-for="w in report.warnings" :key="w" style="color: var(--warn); margin-top: 6px">{{ w }}</div>
          </div>
          <p class="faint" style="font-size: 12.5px; margin: 10px 0 0">A backup of all contacts is written before every sync (last 10 are kept).</p>
        </div>
      </div>
      <footer><button class="btn primary" @click="state.dialog = null">Done</button></footer>
    </div>
  </div>
</template>
