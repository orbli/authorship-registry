import { randomUUID } from 'node:crypto';
import { Hono } from 'hono';
import { deleteCookie, getCookie, setCookie } from 'hono/cookie';
import type { Config } from './config.js';
import { one, type Db } from './db/index.js';
import type { Mailer } from './mail.js';
import { ROLES, SESSION_COOKIE, consumeToken, createSession, destroySession, issueToken, loadUser, requireRole, type AuthEnv, type Role, type User } from './auth.js';
import { looksLikeDoi, nameKey, nameScore, normalizeDoi } from './names.js';
import { page } from './views/layout.js';
import { AdminUsersPage, AuditPage, HomePage, LoginPage, MessagePage, RecordPage, ReviewDetailPage, ReviewListPage, SearchPage, type ReportFormValues, type ReportRow, type SearchHit, type AuditRow } from './views/pages.js';

export interface Deps {
  db: Db;
  mailer: Mailer;
  cfg: Config;
}

const EMAIL_RE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

export function createApp({ db, mailer, cfg }: Deps) {
  const app = new Hono<AuthEnv>();

  const audit = (p: { actor?: User; action: string; reportId?: string | null; detail?: string }) =>
    db.query(`INSERT INTO audit_log (id, at, actor_user_id, actor_email, action, report_id, detail) VALUES ($1, $2, $3, $4, $5, $6, $7)`, [
      randomUUID(), new Date(), p.actor?.id ?? null, p.actor?.email ?? null, p.action, p.reportId ?? null, p.detail ?? null,
    ]);

  /** Origin for links in emails: configured BASE_URL, else the request's own origin. */
  const baseUrl = (c: { req: { url: string } }) => cfg.baseUrl || new URL(c.req.url).origin;
  const secureCookie = (c: { req: { url: string } }) => baseUrl(c).startsWith('https://');

  app.use('*', async (c, next) => {
    c.set('db', db);
    await next();
  });
  app.use('*', loadUser);

  app.get('/healthz', async (c) => {
    await db.query('SELECT 1');
    return c.text('ok');
  });

  // ---------- public: reporting ----------

  app.get('/', (c) => c.html(page(<HomePage user={c.var.user} />)));

  app.post('/report', async (c) => {
    const f = await c.req.parseBody();
    const v: ReportFormValues = {};
    for (const k of ['reporter_name', 'reporter_email', 'reporter_affiliation', 'subject_name', 'subject_affiliation', 'subject_email', 'paper_title', 'paper_doi', 'paper_venue', 'description'] as const) {
      const raw = f[k];
      v[k] = typeof raw === 'string' ? raw.trim() : '';
    }
    const missing = (['reporter_name', 'reporter_email', 'reporter_affiliation', 'subject_name', 'paper_title', 'description'] as const).filter((k) => !v[k]);
    const fail = (error: string) => c.html(page(<HomePage user={c.var.user} values={v} error={error} />), 400);
    if (missing.length) return fail(`Please fill in: ${missing.join(', ').replace(/_/g, ' ')}.`);
    if (!EMAIL_RE.test(v.reporter_email!)) return fail('That email address does not look valid.');
    const email = v.reporter_email!.toLowerCase();
    const id = randomUUID();
    await db.query(
      `INSERT INTO reports (id, status, reporter_name, reporter_email, reporter_affiliation, subject_name, subject_name_key, subject_affiliation, subject_email, paper_title, paper_doi, paper_venue, description, created_at)
       VALUES ($1, 'pending_email', $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13)`,
      [id, v.reporter_name, email, v.reporter_affiliation, v.subject_name, nameKey(v.subject_name!), v.subject_affiliation || null, v.subject_email || null, v.paper_title, v.paper_doi ? normalizeDoi(v.paper_doi) : null, v.paper_venue || null, v.description, new Date()],
    );
    const token = await issueToken(db, { purpose: 'verify_report', email, reportId: id, ttlMinutes: cfg.tokenTtlMinutes * 4 });
    await mailer.send({
      to: email,
      subject: 'Confirm your report to the Authorship Registry',
      text: `Hello ${v.reporter_name},\n\nSomeone used this email address to file a report about "${v.paper_title}".\n\nIf that was you, confirm it here (the link works once and expires in ${cfg.tokenTtlMinutes * 4} minutes):\n\n${baseUrl(c)}/verify/${token}\n\nIf it was not you, ignore this message and nothing will be filed.\n`,
    });
    return c.redirect('/report/sent');
  });

  app.get('/report/sent', (c) =>
    c.html(page(<MessagePage user={c.var.user} title="Check your email" body="We sent you a confirmation link. Your report is not filed until you click it." kind="ok" />)),
  );

  app.get('/verify/:token', async (c) => {
    const t = await consumeToken(db, c.req.param('token'), 'verify_report');
    if (!t?.report_id) return c.html(page(<MessagePage user={c.var.user} title="Link not valid" body="This confirmation link is invalid, expired or already used." kind="err" />), 400);
    await db.query(`UPDATE reports SET status = 'submitted', verified_at = $1 WHERE id = $2 AND status = 'pending_email'`, [new Date(), t.report_id]);
    await audit({ action: 'report_verified', reportId: t.report_id, detail: t.email });
    return c.html(page(<MessagePage user={c.var.user} title="Report filed" body="Thank you. Your report is now with the review team. You will not be contacted unless they need clarification." kind="ok" />));
  });

  // ---------- staff auth ----------

  app.get('/login', (c) => c.html(page(<LoginPage next={c.req.query('next')} sent={c.req.query('sent') === '1'} />)));

  app.post('/login', async (c) => {
    const f = await c.req.parseBody();
    const email = String(f.email ?? '').trim().toLowerCase();
    const next = String(f.next ?? '');
    const user = await one<User>(db, `SELECT id, email, name, role, disabled FROM users WHERE email = $1`, [email]);
    if (user && !user.disabled) {
      const token = await issueToken(db, { purpose: 'login', email, ttlMinutes: cfg.tokenTtlMinutes });
      const url = `${baseUrl(c)}/auth/${token}${next ? `?next=${encodeURIComponent(next)}` : ''}`;
      await mailer.send({ to: email, subject: 'Your Authorship Registry sign-in link', text: `Sign in here (expires in ${cfg.tokenTtlMinutes} minutes, works once):\n\n${url}\n` });
    }
    // Same response whether or not the address is registered: no account enumeration.
    return c.redirect('/login?sent=1');
  });

  app.get('/auth/:token', async (c) => {
    const t = await consumeToken(db, c.req.param('token'), 'login');
    const user = t && (await one<User>(db, `SELECT id, email, name, role, disabled FROM users WHERE email = $1`, [t.email]));
    if (!user || user.disabled) return c.html(page(<MessagePage title="Link not valid" body="This sign-in link is invalid, expired or already used." kind="err" />), 400);
    const sid = await createSession(db, user.id, cfg.sessionTtlHours);
    setCookie(c, SESSION_COOKIE, sid, { httpOnly: true, secure: secureCookie(c), sameSite: 'Lax', path: '/', maxAge: cfg.sessionTtlHours * 3600 });
    await audit({ actor: user, action: 'login' });
    const next = c.req.query('next');
    const home = user.role === 'editor' ? '/search' : '/review';
    return c.redirect(next && next.startsWith('/') && !next.startsWith('//') ? next : home);
  });

  app.post('/logout', async (c) => {
    const sid = getCookie(c, SESSION_COOKIE);
    if (sid) await destroySession(db, sid);
    deleteCookie(c, SESSION_COOKIE, { path: '/' });
    return c.redirect('/');
  });

  // ---------- reviewers ----------

  const REPORT_COLS = `id, status, reporter_name, reporter_email, reporter_affiliation, subject_name, subject_name_key, subject_affiliation, subject_email, paper_title, paper_doi, paper_venue, description, summary, decision_note, decided_by, created_at, verified_at, decided_at`;

  app.get('/review', requireRole('reviewer'), async (c) => {
    const status = ['submitted', 'accepted', 'rejected'].includes(c.req.query('status') ?? '') ? c.req.query('status')! : 'submitted';
    const reports = await db.query<ReportRow>(`SELECT ${REPORT_COLS} FROM reports WHERE status = $1 ORDER BY verified_at DESC LIMIT 200`, [status]);
    return c.html(page(<ReviewListPage user={c.var.user!} reports={reports} status={status} />));
  });

  app.get('/review/:id', requireRole('reviewer'), async (c) => {
    const r = await one<ReportRow>(db, `SELECT ${REPORT_COLS} FROM reports WHERE id = $1 AND status <> 'pending_email'`, [c.req.param('id')]);
    if (!r) return c.notFound();
    await audit({ actor: c.var.user, action: 'review_view', reportId: r.id });
    return c.html(page(<ReviewDetailPage user={c.var.user!} report={r} />));
  });

  app.post('/review/:id/decision', requireRole('reviewer'), async (c) => {
    const r = await one<ReportRow>(db, `SELECT ${REPORT_COLS} FROM reports WHERE id = $1 AND status <> 'pending_email'`, [c.req.param('id')]);
    if (!r) return c.notFound();
    const f = await c.req.parseBody();
    const status = String(f.status ?? '');
    const summary = String(f.summary ?? '').trim();
    const note = String(f.decision_note ?? '').trim();
    if (!['accepted', 'rejected'].includes(status)) return c.text('Bad status', 400);
    if (status === 'accepted' && !summary) return c.html(page(<ReviewDetailPage user={c.var.user!} report={r} error="An editor-facing summary is required to accept a report." />), 400);
    await db.query(`UPDATE reports SET status = $1, summary = $2, decision_note = $3, decided_by = $4, decided_at = $5 WHERE id = $6`, [
      status, summary || null, note || null, c.var.user!.id, new Date(), r.id,
    ]);
    await audit({ actor: c.var.user, action: `decision_${status}`, reportId: r.id, detail: r.status !== 'submitted' ? `was ${r.status}` : undefined });
    return c.redirect(`/review/${r.id}`);
  });

  // ---------- editors ----------

  interface AcceptedRow { id: string; subject_name: string; subject_name_key: string; subject_affiliation: string | null; paper_title: string; paper_doi: string | null; paper_venue: string | null; decided_at: Date | null }

  async function search(q: string): Promise<SearchHit[]> {
    const cols = `id, subject_name, subject_name_key, subject_affiliation, paper_title, paper_doi, paper_venue, decided_at`;
    if (looksLikeDoi(q)) {
      const rows = await db.query<AcceptedRow>(`SELECT ${cols} FROM reports WHERE status = 'accepted' AND paper_doi = $1`, [normalizeDoi(q)]);
      return rows.map((r) => ({ ...r, score: 1 }));
    }
    const rows = await db.query<AcceptedRow>(`SELECT ${cols} FROM reports WHERE status = 'accepted' LIMIT 5000`);
    const ql = q.toLowerCase();
    return rows
      .map((r) => {
        const byName = nameScore(q, r.subject_name_key);
        const byTitle = ql.length >= 4 && r.paper_title.toLowerCase().includes(ql) ? 0.8 : 0;
        return { ...r, score: Math.max(byName, byTitle) };
      })
      .filter((h) => h.score >= 0.35)
      .sort((a, b) => b.score - a.score || (b.decided_at?.valueOf() ?? 0) - (a.decided_at?.valueOf() ?? 0))
      .slice(0, 50);
  }

  app.get('/search', requireRole('editor'), async (c) => {
    const q = (c.req.query('q') ?? '').trim();
    if (!q) return c.html(page(<SearchPage user={c.var.user!} q="" />));
    const hits = await search(q);
    await audit({ actor: c.var.user, action: 'search', detail: `${q} → ${hits.length}` });
    return c.html(page(<SearchPage user={c.var.user!} q={q} hits={hits} />));
  });

  app.get('/record/:id', requireRole('editor'), async (c) => {
    const r = await one<ReportRow>(db, `SELECT ${REPORT_COLS} FROM reports WHERE id = $1 AND status = 'accepted'`, [c.req.param('id')]);
    if (!r) return c.notFound();
    const key = (r as unknown as { subject_name_key: string }).subject_name_key;
    const related = await one<{ n: string | number }>(db, `SELECT COUNT(*) AS n FROM reports WHERE status = 'accepted' AND subject_name_key = $1 AND id <> $2`, [key, r.id]);
    await audit({ actor: c.var.user, action: 'record_view', reportId: r.id });
    return c.html(page(<RecordPage user={c.var.user!} report={r} related={Number(related?.n ?? 0)} />));
  });

  // ---------- admin ----------

  const listUsers = () => db.query<User>(`SELECT id, email, name, role, disabled FROM users ORDER BY email`);

  app.get('/admin/users', requireRole(), async (c) => c.html(page(<AdminUsersPage user={c.var.user!} users={await listUsers()} />)));

  app.post('/admin/users', requireRole(), async (c) => {
    const f = await c.req.parseBody();
    const email = String(f.email ?? '').trim().toLowerCase();
    const name = String(f.name ?? '').trim();
    const role = String(f.role ?? '') as Role;
    const fail = async (error: string) => c.html(page(<AdminUsersPage user={c.var.user!} users={await listUsers()} error={error} />), 400);
    if (!EMAIL_RE.test(email) || !name || !ROLES.includes(role)) return fail('Email, name and a valid role are required.');
    if (await one(db, `SELECT id FROM users WHERE email = $1`, [email])) return fail('That email is already registered.');
    const id = randomUUID();
    await db.query(`INSERT INTO users (id, email, name, role, disabled, created_at) VALUES ($1, $2, $3, $4, false, $5)`, [id, email, name, role, new Date()]);
    await audit({ actor: c.var.user, action: 'user_added', detail: `${email} as ${role}` });
    return c.redirect('/admin/users');
  });

  app.post('/admin/users/:id/:op{enable|disable}', requireRole(), async (c) => {
    const id = c.req.param('id');
    if (id === c.var.user!.id) return c.text('Cannot change your own account', 400);
    const disabled = c.req.param('op') === 'disable';
    const rows = await db.query<{ email: string }>(`UPDATE users SET disabled = $1 WHERE id = $2 RETURNING email`, [disabled, id]);
    if (!rows.length) return c.notFound();
    if (disabled) await db.query(`DELETE FROM sessions WHERE user_id = $1`, [id]);
    await audit({ actor: c.var.user, action: disabled ? 'user_disabled' : 'user_enabled', detail: rows[0].email });
    return c.redirect('/admin/users');
  });

  app.get('/admin/audit', requireRole(), async (c) => {
    const rows = await db.query<AuditRow>(`SELECT at, actor_email, action, report_id, detail FROM audit_log ORDER BY at DESC LIMIT 200`);
    return c.html(page(<AuditPage user={c.var.user!} rows={rows} />));
  });

  app.notFound((c) => c.html(page(<MessagePage user={c.var.user} title="Not found" body="There is nothing at this address." kind="err" />), 404));

  return app;
}
