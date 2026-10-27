import type { FC } from 'hono/jsx';
import type { User } from '../auth.js';
import { Layout, fmtDate } from './layout.js';

export interface ReportRow {
  id: string;
  status: string;
  reporter_name: string;
  reporter_email: string;
  reporter_affiliation: string;
  subject_name: string;
  subject_affiliation: string | null;
  subject_email: string | null;
  paper_title: string;
  paper_doi: string | null;
  paper_venue: string | null;
  description: string;
  summary: string | null;
  decision_note: string | null;
  created_at: Date;
  verified_at: Date | null;
  decided_at: Date | null;
}

export type ReportFormValues = Partial<Record<'reporter_name' | 'reporter_email' | 'reporter_affiliation' | 'subject_name' | 'subject_affiliation' | 'subject_email' | 'paper_title' | 'paper_doi' | 'paper_venue' | 'description', string>>;

const Err: FC<{ error?: string }> = ({ error }) => (error ? <div class="notice err">{error}</div> : null);

export const ReportPage: FC<{ user?: User; values?: ReportFormValues; error?: string }> = ({ user, values = {}, error }) => (
  <Layout title="Report" user={user}>
    <h1>Report undisclosed AI-generated authorship</h1>
    <p class="lead">
      Use this form if you have first-hand knowledge that a manuscript or published paper was substantially produced by AI without
      disclosure. Reports are reviewed by a human team before anything becomes visible to journal editors. You must give your real
      name and a working email address; we will send you a confirmation link and nothing is filed until you click it. Your identity is
      never shown to editors.
    </p>
    <Err error={error} />
    <form method="post" action="/report">
      <h2>About you</h2>
      <label><span class="req">Your full name</span><input name="reporter_name" required maxlength={200} value={values.reporter_name ?? ''} /></label>
      <label><span class="req">Your email</span><input name="reporter_email" type="email" required maxlength={200} value={values.reporter_email ?? ''} /></label>
      <label><span class="req">Your affiliation</span><input name="reporter_affiliation" required maxlength={200} value={values.reporter_affiliation ?? ''} /></label>
      <h2>About the author being reported</h2>
      <label><span class="req">Author name</span><input name="subject_name" required maxlength={200} value={values.subject_name ?? ''} /></label>
      <label><span>Author affiliation</span><input name="subject_affiliation" maxlength={200} value={values.subject_affiliation ?? ''} /></label>
      <label><span>Author email, if known</span><input name="subject_email" maxlength={200} value={values.subject_email ?? ''} /></label>
      <h2>About the paper</h2>
      <label><span class="req">Paper title</span><input name="paper_title" required maxlength={500} value={values.paper_title ?? ''} /></label>
      <label><span>DOI or preprint identifier</span><input name="paper_doi" maxlength={200} placeholder="10.1234/abcd.5678" value={values.paper_doi ?? ''} /></label>
      <label><span>Journal, conference or venue</span><input name="paper_venue" maxlength={200} value={values.paper_venue ?? ''} /></label>
      <label><span class="req">What you observed, and how you know</span><textarea name="description" required maxlength={8000}>{values.description ?? ''}</textarea></label>
      <button>Send confirmation email</button>
    </form>
  </Layout>
);

export const MessagePage: FC<{ user?: User; title: string; body: string; kind?: 'ok' | 'err' }> = ({ user, title, body, kind }) => (
  <Layout title={title} user={user}>
    <h1>{title}</h1>
    <div class={`notice ${kind ?? ''}`}>{body}</div>
  </Layout>
);

export const LoginPage: FC<{ next?: string; sent?: boolean }> = ({ next, sent }) => (
  <Layout title="Staff sign in">
    <h1>Staff sign in</h1>
    {sent ? (
      <div class="notice ok">If that address belongs to a registered reviewer or editor, a sign-in link is on its way. It expires shortly.</div>
    ) : (
      <form method="post" action="/login">
        <input type="hidden" name="next" value={next ?? ''} />
        <label><span class="req">Email</span><input name="email" type="email" required autofocus /></label>
        <button>Email me a sign-in link</button>
      </form>
    )}
  </Layout>
);

const StatusBadge: FC<{ status: string }> = ({ status }) => <span class={`badge ${status}`}>{status.replace('_', ' ')}</span>;

export const ReviewListPage: FC<{ user: User; reports: ReportRow[]; status: string }> = ({ user, reports, status }) => (
  <Layout title="Review queue" user={user}>
    <h1>Review queue</h1>
    <p class="lead">
      Showing <strong>{status}</strong> reports.{' '}
      {['submitted', 'accepted', 'rejected'].filter((s) => s !== status).map((s) => (
        <> · <a href={`/review?status=${s}`}>{s}</a></>
      ))}
    </p>
    {reports.length === 0 ? <div class="notice">Nothing here.</div> : null}
    {reports.map((r) => (
      <div class="card">
        <h3><a href={`/review/${r.id}`}>{r.subject_name}</a> <StatusBadge status={r.status} /></h3>
        <div>{r.paper_title}</div>
        <div class="meta">
          {r.paper_venue ?? ''}{r.paper_doi ? ` · ${r.paper_doi}` : ''} · reported {fmtDate(r.verified_at ?? r.created_at)} by {r.reporter_name} ({r.reporter_affiliation})
        </div>
      </div>
    ))}
  </Layout>
);

export const ReviewDetailPage: FC<{ user: User; report: ReportRow; error?: string }> = ({ user, report: r, error }) => (
  <Layout title="Review report" user={user}>
    <h1>{r.subject_name} <StatusBadge status={r.status} /></h1>
    <Err error={error} />
    <div class="card">
      <h3>Paper</h3>
      <dl>
        <dt>Title</dt><dd>{r.paper_title}</dd>
        <dt>DOI</dt><dd>{r.paper_doi ?? '—'}</dd>
        <dt>Venue</dt><dd>{r.paper_venue ?? '—'}</dd>
        <dt>Author affiliation</dt><dd>{r.subject_affiliation ?? '—'}</dd>
        <dt>Author email</dt><dd>{r.subject_email ?? '—'}</dd>
      </dl>
    </div>
    <div class="card">
      <h3>Reporter (confidential)</h3>
      <dl>
        <dt>Name</dt><dd>{r.reporter_name}</dd>
        <dt>Email</dt><dd>{r.reporter_email}</dd>
        <dt>Affiliation</dt><dd>{r.reporter_affiliation}</dd>
        <dt>Email verified</dt><dd>{fmtDate(r.verified_at) || 'not yet'}</dd>
      </dl>
    </div>
    <div class="card">
      <h3>Report</h3>
      <dd>{r.description}</dd>
    </div>
    {r.status === 'accepted' || r.status === 'rejected' ? (
      <div class="card">
        <h3>Decision · {fmtDate(r.decided_at)}</h3>
        <dl>
          <dt>Editor-facing summary</dt><dd>{r.summary ?? '—'}</dd>
          <dt>Internal note</dt><dd>{r.decision_note ?? '—'}</dd>
        </dl>
      </div>
    ) : null}
    <h2>{r.status === 'submitted' ? 'Decide' : 'Revise decision'}</h2>
    <form method="post" action={`/review/${r.id}/decision`}>
      <label><span class="req">Decision</span>
        <select name="status">
          <option value="accepted" selected={r.status === 'accepted'}>Accept: visible to editors</option>
          <option value="rejected" selected={r.status === 'rejected'}>Reject: never shown</option>
        </select>
      </label>
      <label><span>Summary shown to editors (required when accepting; never include the reporter)</span>
        <textarea name="summary" maxlength={4000}>{r.summary ?? ''}</textarea>
      </label>
      <label><span>Internal note (review team only)</span><textarea name="decision_note" maxlength={4000}>{r.decision_note ?? ''}</textarea></label>
      <button>Save decision</button>
    </form>
  </Layout>
);

export interface SearchHit {
  id: string;
  subject_name: string;
  subject_affiliation: string | null;
  paper_title: string;
  paper_doi: string | null;
  paper_venue: string | null;
  summary: string | null;
  decided_at: Date | null;
  score: number;
}

const snippet = (s: string | null, n = 180) => (s ? (s.length > n ? s.slice(0, n - 1).trimEnd() + '…' : s) : '');

export const IncidentsPage: FC<{ user?: User; q: string; hits: SearchHit[] }> = ({ user, q, hits }) => (
  <Layout title={q ? `Search: ${q}` : 'Reported incidents'} user={user}>
    <h1>Reported incidents</h1>
    <p class="lead">
      Accepted reports of undisclosed AI-generated authorship, vetted by the review team. Search by author name, DOI or paper title.
      Name matching is fuzzy and ignores word order, so "Wong Ka Ming" also finds "K. M. Wong". Reporter identities are never shown.
    </p>
    <form method="get" action="/" class="row">
      <label><span>Author name, DOI or paper title</span><input name="q" value={q} autofocus placeholder="e.g. K. M. Wong, 10.1234/abcd.5678" /></label>
      <button>Search</button>
      {q ? <a href="/" style="align-self:center">Clear</a> : null}
    </form>
    {q && hits.length === 0 ? (
      <div class="notice ok" style="margin-top:16px">No accepted reports match "{q}".</div>
    ) : null}
    {!q && hits.length === 0 ? (
      <div class="notice" style="margin-top:16px">No accepted reports yet.</div>
    ) : null}
    {hits.length > 0 ? (
      <>
        <h2>{q ? `${hits.length} match${hits.length === 1 ? '' : 'es'}` : `Latest ${hits.length}`}</h2>
        <table>
          <thead><tr><th>Author</th><th>Paper</th><th>Venue</th><th>Accepted</th>{q ? <th>Match</th> : null}</tr></thead>
          <tbody>
            {hits.map((h) => (
              <tr>
                <td><a href={`/record/${h.id}`}>{h.subject_name}</a><div class="meta">{h.subject_affiliation ?? ''}</div></td>
                <td>{h.paper_title}<div class="meta">{h.paper_doi ?? ''}</div><div class="meta">{snippet(h.summary)}</div></td>
                <td>{h.paper_venue ?? ''}</td>
                <td>{fmtDate(h.decided_at)}</td>
                {q ? <td>{Math.round(h.score * 100)}%</td> : null}
              </tr>
            ))}
          </tbody>
        </table>
      </>
    ) : null}
  </Layout>
);

export const RecordPage: FC<{ user?: User; report: ReportRow; related: number }> = ({ user, report: r, related }) => (
  <Layout title="Record" user={user}>
    <h1>{r.subject_name}</h1>
    <p class="meta">Accepted {fmtDate(r.decided_at)}{related > 0 ? ` · ${related} other accepted report${related > 1 ? 's' : ''} about an author with this name` : ''}</p>
    <div class="card">
      <dl>
        <dt>Affiliation</dt><dd>{r.subject_affiliation ?? '—'}</dd>
        <dt>Paper</dt><dd>{r.paper_title}</dd>
        <dt>DOI</dt><dd>{r.paper_doi ?? '—'}</dd>
        <dt>Venue</dt><dd>{r.paper_venue ?? '—'}</dd>
      </dl>
    </div>
    <div class="card">
      <h3>Summary from the review team</h3>
      <dd>{r.summary}</dd>
    </div>
    {user ? <p class="meta">This view has been recorded in the audit log.</p> : null}
    <p class="meta"><a href="/">Back to incidents</a> · <a href="/report">File a report</a></p>
  </Layout>
);

export const AdminUsersPage: FC<{ user: User; users: User[]; error?: string }> = ({ user, users, error }) => (
  <Layout title="Users" user={user}>
    <h1>Staff accounts</h1>
    <Err error={error} />
    <table>
      <thead><tr><th>Email</th><th>Name</th><th>Role</th><th>Status</th><th></th></tr></thead>
      <tbody>
        {users.map((u) => (
          <tr>
            <td>{u.email}</td><td>{u.name}</td><td>{u.role}</td><td>{u.disabled ? 'disabled' : 'active'}</td>
            <td>
              {u.id !== user.id ? (
                <form method="post" action={`/admin/users/${u.id}/${u.disabled ? 'enable' : 'disable'}`}><button class="secondary inline">{u.disabled ? 'Enable' : 'Disable'}</button></form>
              ) : null}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
    <h2>Add staff</h2>
    <form method="post" action="/admin/users">
      <label><span class="req">Email</span><input name="email" type="email" required /></label>
      <label><span class="req">Name</span><input name="name" required maxlength={200} /></label>
      <label><span class="req">Role</span>
        <select name="role"><option value="editor">editor: search accepted records</option><option value="reviewer">reviewer: vet reports, see reporters</option><option value="admin">admin: everything</option></select>
      </label>
      <button>Add</button>
    </form>
  </Layout>
);

export interface AuditRow { at: Date; actor_email: string | null; action: string; report_id: string | null; detail: string | null }

export const AuditPage: FC<{ user: User; rows: AuditRow[] }> = ({ user, rows }) => (
  <Layout title="Audit log" user={user}>
    <h1>Audit log</h1>
    <table>
      <thead><tr><th>When (UTC)</th><th>Who</th><th>Action</th><th>Report</th><th>Detail</th></tr></thead>
      <tbody>
        {rows.map((r) => (
          <tr><td>{new Date(r.at).toISOString().replace('T', ' ').slice(0, 19)}</td><td>{r.actor_email ?? ''}</td><td>{r.action}</td><td>{r.report_id ? <a href={`/review/${r.report_id}`}>{r.report_id.slice(0, 8)}</a> : ''}</td><td>{r.detail ?? ''}</td></tr>
        ))}
      </tbody>
    </table>
  </Layout>
);
