import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { createApp } from '../src/app.js';
import { loadConfig } from '../src/config.js';
import { openDb, type Db } from '../src/db/index.js';
import { migrate } from '../src/db/schema.js';
import { consoleMailer } from '../src/mail.js';

const cfg = loadConfig({ DB_DIALECT: 'pglite', MAIL_DRIVER: 'console', BASE_URL: 'http://test.local', ADMIN_EMAIL: 'admin@test.local', TOKEN_TTL_MINUTES: '5' });
const mailer = consoleMailer();
let db: Db;
let app: ReturnType<typeof createApp>;

const form = (data: Record<string, string>, cookie?: string) =>
  new Request('http://test.local/_', {
    method: 'POST',
    headers: { 'content-type': 'application/x-www-form-urlencoded', ...(cookie ? { cookie } : {}) },
    body: new URLSearchParams(data).toString(),
  });
const post = (path: string, data: Record<string, string>, cookie?: string) => app.request(path, form(data, cookie));
const get = (path: string, cookie?: string) => app.request(path, { headers: cookie ? { cookie } : {} });
const lastLink = () => mailer.sent!.at(-1)!.text.match(/http:\/\/test\.local(\/\S+)/)![1];

async function signIn(email: string): Promise<string> {
  const r = await post('/login', { email });
  expect(r.status).toBe(302);
  const link = lastLink();
  const auth = await get(link);
  expect(auth.status).toBe(302);
  const cookie = auth.headers.get('set-cookie')!;
  expect(cookie).toMatch(/^sid=/);
  return cookie.split(';')[0];
}

beforeAll(async () => {
  db = await openDb(cfg);
  await migrate(db, { adminEmail: cfg.adminEmail });
  app = createApp({ db, mailer, cfg });
});
afterAll(() => db.close());

describe('end to end', () => {
  let reportId: string;
  let admin: string;
  let reviewer: string;
  let editor: string;

  it('serves the incident list, the report form and the health check', async () => {
    expect((await get('/healthz')).status).toBe(200);
    const home = await get('/');
    expect(home.status).toBe(200);
    expect(await home.text()).toContain('No accepted reports yet');
    const form = await get('/report');
    expect(form.status).toBe(200);
    expect(await form.text()).toContain('Report undisclosed AI-generated authorship');
  });

  it('rejects an incomplete report', async () => {
    const r = await post('/report', { reporter_name: 'x' });
    expect(r.status).toBe(400);
  });

  it('files a report only after email confirmation', async () => {
    const r = await post('/report', {
      reporter_name: 'Alice Reporter', reporter_email: 'Alice@Uni.edu', reporter_affiliation: 'Test University',
      subject_name: 'Wong, Ka-Ming', subject_affiliation: 'Other University',
      paper_title: 'On the Spontaneous Generation of Paragraphs', paper_doi: 'https://doi.org/10.1000/Spont.1', paper_venue: 'J. Test',
      description: 'The text was generated; I saw the prompt history.',
    });
    expect(r.status).toBe(302);
    const pending = await db.query<{ id: string; status: string; reporter_email: string; paper_doi: string; subject_name_key: string }>('SELECT id, status, reporter_email, paper_doi, subject_name_key FROM reports');
    expect(pending).toHaveLength(1);
    expect(pending[0].status).toBe('pending_email');
    expect(pending[0].reporter_email).toBe('alice@uni.edu');
    expect(pending[0].paper_doi).toBe('10.1000/spont.1');
    expect(pending[0].subject_name_key).toBe('ka ming wong');
    reportId = pending[0].id;

    const link = lastLink();
    expect(link).toMatch(/^\/verify\//);
    const v = await get(link);
    expect(v.status).toBe(200);
    expect(await v.text()).toContain('Report filed');
    expect((await db.query<{ status: string }>('SELECT status FROM reports'))[0].status).toBe('submitted');

    // link is single use
    expect((await get(link)).status).toBe(400);
  });

  it('admin signs in via magic link and adds staff', async () => {
    admin = await signIn('admin@test.local');
    const users = await get('/admin/users', admin);
    expect(users.status).toBe(200);
    expect((await post('/admin/users', { email: 'rev@test.local', name: 'Rhea', role: 'reviewer' }, admin)).status).toBe(302);
    expect((await post('/admin/users', { email: 'ed@test.local', name: 'Eddy', role: 'editor' }, admin)).status).toBe(302);
    expect((await post('/admin/users', { email: 'ed@test.local', name: 'Dup', role: 'editor' }, admin)).status).toBe(400);
  });

  it('unknown email gets the same login response but no mail', async () => {
    const before = mailer.sent!.length;
    expect((await post('/login', { email: 'nobody@test.local' })).status).toBe(302);
    expect(mailer.sent!.length).toBe(before);
  });

  it('editor cannot see the review queue; the old /search URL redirects to /', async () => {
    reviewer = await signIn('rev@test.local');
    editor = await signIn('ed@test.local');
    expect((await get('/review', editor)).status).toBe(403);
    expect((await get('/review')).status).toBe(302); // anonymous → login
    const old = await get('/search?q=wong');
    expect(old.status).toBe(302);
    expect(old.headers.get('location')).toBe('/?q=wong');
  });

  it('nothing is listed or found before the report is accepted', async () => {
    const r = await get('/?q=Wong%20Ka%20Ming');
    expect(r.status).toBe(200);
    expect(await r.text()).toContain('No accepted reports match');
    expect(await (await get('/')).text()).toContain('No accepted reports yet');
    expect((await get(`/record/${reportId}`)).status).toBe(404);
  });

  it('reviewer sees the reporter and must give a summary to accept', async () => {
    const detail = await get(`/review/${reportId}`, reviewer);
    expect(detail.status).toBe(200);
    expect(await detail.text()).toContain('alice@uni.edu');
    expect((await post(`/review/${reportId}/decision`, { status: 'accepted', summary: '' }, reviewer)).status).toBe(400);
    expect((await post(`/review/${reportId}/decision`, { status: 'accepted', summary: 'Confirmed by two independent sources.', decision_note: 'Reporter is credible.' }, reviewer)).status).toBe(302);
  });

  it('the accepted report is listed and found by fuzzy name, DOI and title, never the reporter', async () => {
    const list = await (await get('/')).text();
    expect(list).toContain('Wong, Ka-Ming');
    expect(list).toContain('Confirmed by two independent sources.');
    expect(list).not.toContain('alice');
    for (const q of ['K. M. Wong', 'wong ka ming', '10.1000/SPONT.1', 'spontaneous generation']) {
      const r = await get(`/?q=${encodeURIComponent(q)}`, editor);
      const body = await r.text();
      expect(body, q).toContain('Wong, Ka-Ming');
      expect(body, q).not.toContain('alice');
    }
    const miss = await get('/?q=Chan%20Tai%20Man');
    expect(await miss.text()).toContain('No accepted reports match');

    const anon = await (await get(`/record/${reportId}`)).text();
    expect(anon).toContain('Confirmed by two independent sources.');
    expect(anon).not.toContain('audit log');
    const rec = await get(`/record/${reportId}`, editor); // signed-in reads are audit-logged
    const body = await rec.text();
    expect(body).toContain('Confirmed by two independent sources.');
    expect(body).not.toContain('Alice');
    expect(body).not.toContain('Reporter is credible');
  });

  it('writes an audit trail', async () => {
    const rows = await db.query<{ action: string }>('SELECT action FROM audit_log ORDER BY at');
    const actions = rows.map((r) => r.action);
    for (const a of ['report_verified', 'login', 'user_added', 'review_view', 'decision_accepted', 'search', 'record_view']) expect(actions).toContain(a);
    const audit = await get('/admin/audit', admin);
    expect(audit.status).toBe(200);
    expect((await get('/admin/audit', editor)).status).toBe(403);
  });

  it('disabling a user kills their session', async () => {
    const id = (await db.query<{ id: string }>(`SELECT id FROM users WHERE email = 'ed@test.local'`))[0].id;
    expect((await post(`/admin/users/${id}/disable`, {}, admin)).status).toBe(302);
    expect((await get('/search?q=wong', editor)).status).toBe(302);
  });
});
