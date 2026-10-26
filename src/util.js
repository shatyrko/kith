export const MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];

export function fullName(c) {
  const n = c.name;
  const s = [n.prefix, n.given, n.middle, n.family].filter(Boolean).join(' ');
  const full = n.suffix ? (s ? `${s} ${n.suffix}` : n.suffix) : s;
  if (c.kind === 'org') return c.orgs.find((o) => o.company)?.company || full || 'Unnamed';
  return full || c.nicknames[0] || c.orgs.find((o) => o.company)?.company || c.emails[0]?.value || c.phones[0]?.value || 'Unnamed';
}

export function shortName(c) {
  const n = c.name;
  const s = [n.given, n.family].filter(Boolean).join(' ');
  return c.kind === 'org' ? fullName(c) : s || fullName(c);
}

export function formatDate(d) {
  if (!d || !d.month) return '';
  const s = `${d.day} ${MONTHS[d.month - 1]}`;
  return d.year ? `${s} ${d.year}` : s;
}

/** "turns 38 in 9 days" / "today!" for a date that repeats every year. */
export function countdown(d, label) {
  if (!d || !d.month) return '';
  const now = new Date();
  const t0 = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  let next = new Date(now.getFullYear(), d.month - 1, d.day);
  if (next < t0) next = new Date(now.getFullYear() + 1, d.month - 1, d.day);
  const days = Math.round((next - t0) / 86400000);
  const age = d.year ? next.getFullYear() - d.year : null;
  const what = label === 'birthday' ? (age != null ? `turns ${age}` : 'birthday') : age != null ? `${age} years` : 'anniversary';
  if (days === 0) return `${what} today`;
  if (days === 1) return `${what} tomorrow`;
  if (days > 60) return '';
  return `${what} in ${days} days`;
}

export function addressLines(a) {
  const line1 = [a.street, a.extended].filter(Boolean).join(', ');
  const line2 = [a.postcode, a.city].filter(Boolean).join(' ');
  return [a.po_box && `PO Box ${a.po_box}`, line1, [line2, a.region].filter(Boolean).join(', '), a.country].filter(Boolean).join('\n');
}

export function hasAny(list, key = 'value') {
  return Array.isArray(list) && list.some((x) => (typeof x === 'string' ? x : x[key]));
}

export async function copy(text) {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    return false;
  }
}

export const PHONE_LABELS = ['mobile', 'home', 'work', 'main', 'home fax', 'work fax', 'other fax', 'pager', 'other'];
export const EMAIL_LABELS = ['home', 'work', 'other'];
export const URL_LABELS = ['homepage', 'blog', 'profile', 'work', 'other'];
export const RELATION_LABELS = ['spouse', 'partner', 'child', 'parent', 'mother', 'father', 'brother', 'sister', 'friend', 'assistant', 'manager', 'relative'];
export const ADDRESS_LABELS = ['home', 'work', 'other'];
export const IM_LABELS = ['Signal', 'Telegram', 'WhatsApp', 'Matrix', 'XMPP', 'Skype', 'Discord', 'Slack', 'iMessage', 'Threema', 'Session'];
export const SOCIAL_LABELS = ['mastodon', 'twitter', 'linkedin', 'github', 'instagram', 'facebook', 'bluesky', 'youtube', 'tiktok', 'reddit'];
