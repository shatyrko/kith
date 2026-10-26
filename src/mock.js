// Mock backend for `npm run dev` in a browser and for the README screenshots: invented people only.
import catalog from './catalog.json';

const today = new Date();
const inDays = (n, year) => {
  const d = new Date(today.getTime() + n * 86400000);
  return { label: 'birthday', year, month: d.getMonth() + 1, day: d.getDate() };
};

let seq = 0;
const id = () => 'demo-' + ++seq;
const L = (label, value, pref = false) => ({ label, value, pref });

function person(given, family, extra = {}) {
  return {
    id: id(), name: { prefix: '', given, middle: '', family, suffix: '', maiden: '', phonetic_given: '', phonetic_middle: '', phonetic_family: '' },
    nicknames: [], orgs: [], phones: [], emails: [], urls: [], ims: [], social: [], addresses: [], dates: [], relations: [], keys: [],
    notes: '', photo: null, gender: '', pronouns: '', languages: [], timezone: '', geo: '', interests: [], categories: [], favorite: false,
    kind: '', custom: [], extra: [], created: 1790000000 - seq * 5000, updated: 1790000000, ...extra,
  };
}

const first = ['Amelia', 'Noah', 'Sofia', 'Liam', 'Mira', 'Ethan', 'Chloe', 'Oscar', 'Ivy', 'Leo', 'Nora', 'Felix', 'Zoe', 'Hugo', 'Elena', 'Marco', 'Aiko', 'Kenji', 'Priya', 'Arjun', 'Layla', 'Omar', 'Greta', 'Lars', 'Tomás', 'Inés', 'Dmitri', 'Katya', 'Yusuf', 'Amara'];
const last = ['Hart', 'Lindqvist', 'Moreau', 'Okafor', 'Rossi', 'Tanaka', 'Novak', 'Silva', 'Brandt', 'Kowalski', 'Nair', 'Haddad', 'Fischer', 'Bianchi', 'Sørensen', 'Ibarra', 'Volkov', 'Mensah', 'Park', 'Dubois'];
const cos = ['Northwind Labs', 'Helio Studio', 'Bluebird Café', 'Orbit Logistics', 'Pinecone Bank', 'Atlas Design', 'Quill & Co.', 'Lumen Health'];
const jobs = ['Product Designer', 'Engineer', 'Accountant', 'Barista', 'Architect', 'Physician', 'Photographer', 'Teacher'];

const demo = [];
const rich = person('Amelia', 'Hart', {
  name: { prefix: 'Dr.', given: 'Amelia', middle: 'Rose', family: 'Hart', suffix: 'PhD', maiden: 'Whitfield', phonetic_given: '', phonetic_middle: '', phonetic_family: '' },
  nicknames: ['Mel'], favorite: true, pronouns: 'she/her', languages: ['English', 'French'], timezone: 'Europe/London',
  orgs: [{ company: 'Northwind Labs', department: 'Research', title: 'Head of Design', phonetic: '' }],
  phones: [L('mobile', '+44 7700 900123', true), L('work', '+44 20 7946 0958')],
  emails: [L('work', 'amelia.hart@northwind.example', true), L('home', 'mel@hart.example')],
  addresses: [{ label: 'home', po_box: '', extended: 'Flat 4', street: '12 Brook Lane', city: 'Bristol', region: 'Avon', postcode: 'BS1 4DJ', country: 'United Kingdom', pref: true }],
  urls: [L('homepage', 'https://amelia.example')],
  social: [L('mastodon', '@mel@social.example')], ims: [L('Signal', '@mel.42')],
  dates: [inDays(9, 1988), { label: 'anniversary', year: 2015, month: 6, day: 20 }],
  relations: [L('spouse', 'Jonas Hart'), L('assistant', 'Priya Nair')],
  categories: ['Work', 'Friends'], notes: 'Met at the design conference in Lisbon.\nPrefers calls before noon.',
  interests: ['Climbing', 'Letterpress'], custom: [L('Allergies', 'Peanuts')],
});
demo.push(rich);
demo.push(person('Noah', 'Lindqvist', { orgs: [{ company: 'Helio Studio', department: '', title: 'Engineer', phonetic: '' }], phones: [L('mobile', '+46 70 123 45 67')], emails: [L('work', 'noah@helio.example')], categories: ['Work'], favorite: true, dates: [inDays(41, 1991)] }));
demo.push(person('', '', { kind: 'org', orgs: [{ company: 'Bluebird Café', department: '', title: '', phonetic: '' }], phones: [L('main', '+1 415 555 0142')], urls: [L('homepage', 'https://bluebird.example')], addresses: [{ label: 'work', po_box: '', extended: '', street: '88 Market St', city: 'San Francisco', region: 'CA', postcode: '94103', country: 'USA', pref: false }], categories: ['Places'] }));
for (let i = 0; i < 38; i++) {
  const f = first[(i * 7) % first.length], l = last[(i * 11 + 3) % last.length];
  const c = person(f, l, {
    phones: [L(i % 3 ? 'mobile' : 'home', `+1 555 ${String(100 + ((i * 37) % 900)).padStart(3, '0')} ${String(1000 + ((i * 193) % 9000))}`)],
    emails: i % 4 === 3 ? [] : [L('home', `${f}.${l}@example.org`.toLowerCase().normalize('NFD').replace(/[̀-ͯ]/g, ''))],
    orgs: i % 2 ? [{ company: cos[i % cos.length], department: '', title: jobs[i % jobs.length], phonetic: '' }] : [],
    categories: i % 5 === 0 ? ['Family'] : i % 5 === 1 ? ['Friends'] : i % 5 === 2 ? ['Work'] : [],
  });
  demo.push(c);
}

let contacts = demo;
let trash = [];
let settings = { sort_by: 'family', display_order: 'given', theme: 'system', active_template: 'Personal', custom_templates: [], google: { client_id: '', client_secret: '', account: '', two_way: false, allow_deletes: false, auto_sync_minutes: 0, last_sync: 0 } };
let google = { signedIn: false, account: '' };

const fold = (s) => (s || '').toLowerCase().normalize('NFD').replace(/[̀-ͯ]/g, '');
const display = (c, familyFirst) => {
  const n = c.name;
  if (c.kind === 'org') return c.orgs[0]?.company || 'Unnamed';
  const parts = familyFirst && n.family ? [n.family + ',', n.given, n.middle] : [n.given, n.middle, n.family];
  return parts.filter(Boolean).join(' ') || c.nicknames[0] || c.orgs[0]?.company || c.emails[0]?.value || c.phones[0]?.value || 'Unnamed';
};
const hay = (c) => fold(JSON.stringify([c.name, c.nicknames, c.orgs, c.emails, c.urls, c.ims, c.social, c.addresses, c.notes, c.categories, c.custom, c.relations]));
const clone = (x) => JSON.parse(JSON.stringify(x));

function summary(c, q) {
  const key = fold(c.kind === 'org' || (!c.name.given && !c.name.family) ? display(c) : q.sort_family ? `${c.name.family} ${c.name.given}` : `${c.name.given} ${c.name.family}`).trim();
  const o = c.orgs.find((x) => x.company || x.title);
  return {
    id: c.id, display: display(c, q.family_first), letter: /\p{L}/u.test(key[0] || '') ? key[0].toUpperCase() : '#', _k: key,
    subtitle: (o ? [o.title, o.company].filter(Boolean).join(' · ') : '') || c.emails[0]?.value || c.phones[0]?.value || '',
    favorite: c.favorite, has_photo: !!c.photo, is_org: c.kind === 'org', updated: c.updated,
  };
}

export const mockApi = {
  async list(q = {}) {
    const text = fold(q.text || '').trim();
    let rows = contacts.filter((c) => {
      if (q.group === 'favorites' && !c.favorite) return false;
      if (q.group === 'nolabel' && c.categories.length) return false;
      if (q.group?.startsWith('label:') && !c.categories.includes(q.group.slice(6))) return false;
      return !text || text.split(/\s+/).every((t) => hay(c).includes(t) || c.phones.some((p) => p.value.replace(/\D/g, '').includes(t.replace(/\D/g, '')) && t.replace(/\D/g, '').length >= 3));
    });
    let out = rows.map((c) => summary(c, q));
    if (q.group === 'birthdays') {
      const t0 = new Date(today.getFullYear(), today.getMonth(), today.getDate());
      out = rows.map((c) => {
        const b = c.dates.find((d) => d.label === 'birthday');
        if (!b) return null;
        let n = new Date(today.getFullYear(), b.month - 1, b.day);
        if (n < t0) n = new Date(today.getFullYear() + 1, b.month - 1, b.day);
        const days = Math.round((n - t0) / 86400000);
        if (days > 60) return null;
        const s = summary(c, q);
        s.letter = '';
        s.subtitle = `${n.toLocaleString('en', { month: 'short' })} ${b.day} · ${b.year ? 'turns ' + (n.getFullYear() - b.year) : 'birthday'} ${days === 0 ? 'today' : days === 1 ? 'tomorrow' : 'in ' + days + ' days'}`;
        return { s, days };
      }).filter(Boolean).sort((a, b) => a.days - b.days).map((x) => x.s);
    } else if (q.group === 'recent') out = rows.slice().sort((a, b) => b.created - a.created).slice(0, 50).map((c) => summary(c, q));
    else out.sort((a, b) => a._k.localeCompare(b._k));
    return out;
  },
  async get(i) { return clone(contacts.find((c) => c.id === i) || null); },
  async photo(i) { return contacts.find((c) => c.id === i)?.photo || null; },
  async save(c) {
    c = clone(c);
    if (!c.id) { c.id = id(); c.created = Math.floor(Date.now() / 1000); }
    c.updated = Math.floor(Date.now() / 1000);
    const k = contacts.findIndex((x) => x.id === c.id);
    if (k >= 0) contacts[k] = c; else contacts.push(c);
    return clone(c);
  },
  async remove(i) { const c = contacts.find((x) => x.id === i); contacts = contacts.filter((x) => x.id !== i); if (c) trash.push(c); },
  async favorite(i, on) { const c = contacts.find((x) => x.id === i); if (c) c.favorite = on; },
  async labels() {
    const m = {};
    contacts.forEach((c) => c.categories.forEach((l) => (m[l] = (m[l] || 0) + 1)));
    return Object.entries(m).sort((a, b) => a[0].localeCompare(b[0])).map(([name, count]) => ({ name, count }));
  },
  async renameLabel(from, to) { contacts.forEach((c) => { if (c.categories.includes(from)) c.categories = [...new Set(c.categories.map((l) => (l === from ? to : l)).filter(Boolean))]; }); },
  async catalog() { const c = clone(catalog); c.templates.push(...clone(settings.custom_templates)); return c; },
  async settings() { return clone(settings); },
  async saveSettings(s) { settings = clone(s); return clone(settings); },
  async importVcf() { return { added: 3, updated: 0, skipped: 1 }; },
  async exportVcf() { return contacts.length; },
  async duplicates() {
    const a = contacts[3], b = clone(contacts[3]); b.id = id();
    return [[summary(a, {}), summary(b, {})]];
  },
  async merge() { return null; },
  async trash() { return trash.map((c) => summary(c, {})); },
  async restore(i) { const c = trash.find((x) => x.id === i); trash = trash.filter((x) => x.id !== i); if (c) contacts.push(c); },
  async emptyTrash() { trash = []; },
  async stats() { return { total: contacts.length }; },
  async googleStatus() { return { configured: !!settings.google.client_id, signed_in: google.signedIn, account: google.account, two_way: settings.google.two_way, last_sync: settings.google.last_sync }; },
  async googleLogin() { google = { signedIn: true, account: 'you@gmail.example' }; return google; },
  async googleSync(dry) { return { pulled_new: dry ? 0 : 12, pulled_updated: 0, pulled_deleted: 0, pushed_new: 0, pushed_updated: 0, pushed_deleted: 0, conflicts: 0, skipped: 0, warnings: [], planned: [] }; },
  async googleLogout() { google = { signedIn: false, account: '' }; },
  async openUrl(u) { window.open(u, '_blank'); },
  async pickOpen() { return '/demo/contacts.vcf'; },
  async pickSave(n) { return '/demo/' + n; },
};
