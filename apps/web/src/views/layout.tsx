import type { FC, PropsWithChildren } from 'hono/jsx';
import { html } from 'hono/html';
import type { User } from '../auth.js';

const CSS = `
:root{--bg:#fafaf9;--fg:#1c1917;--muted:#57534e;--line:#e7e5e4;--accent:#1d4ed8;--accent-fg:#fff;--warn:#b45309;--ok:#15803d;--card:#fff}
@media (prefers-color-scheme:dark){:root{--bg:#1c1917;--fg:#f5f5f4;--muted:#a8a29e;--line:#44403c;--accent:#60a5fa;--accent-fg:#1c1917;--card:#292524}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--fg);font:16px/1.5 system-ui,-apple-system,"Segoe UI",Roboto,"Noto Sans","Noto Sans CJK TC",sans-serif}
header{border-bottom:1px solid var(--line);background:var(--card)}header .in{max-width:960px;margin:0 auto;padding:12px 16px;display:flex;gap:16px;align-items:center;flex-wrap:wrap}
header a{color:inherit;text-decoration:none}header .brand{font-weight:600}header nav{display:flex;gap:14px;flex-wrap:wrap}header .me{margin-left:auto;color:var(--muted);font-size:14px;display:flex;gap:10px;align-items:center}
main{max-width:960px;margin:0 auto;padding:24px 16px}footer{max-width:960px;margin:0 auto;padding:24px 16px;color:var(--muted);font-size:13px}
h1{font-size:26px;margin:0 0 12px}h2{font-size:19px;margin:24px 0 8px}p.lead{color:var(--muted);max-width:68ch}
form{display:grid;gap:14px;max-width:640px}label{display:grid;gap:4px;font-size:14px;color:var(--muted)}label span.req::after{content:" *";color:var(--warn)}
input,textarea,select{font:inherit;padding:8px 10px;border:1px solid var(--line);border-radius:6px;background:var(--card);color:var(--fg);width:100%}textarea{min-height:140px}
button{font:inherit;padding:9px 16px;border-radius:6px;border:1px solid var(--accent);background:var(--accent);color:var(--accent-fg);cursor:pointer;justify-self:start}button.secondary{background:transparent;color:var(--fg);border-color:var(--line)}a.button{display:inline-block;padding:9px 16px;border-radius:6px;border:1px solid var(--accent);background:var(--accent);color:var(--accent-fg);text-decoration:none}button.inline{padding:4px 10px;font-size:13px}
.notice{padding:12px 14px;border-radius:8px;border:1px solid var(--line);background:var(--card)}.notice.err{border-color:var(--warn)}.notice.ok{border-color:var(--ok)}
table{width:100%;border-collapse:collapse;font-size:14px}th,td{text-align:left;padding:8px 6px;border-bottom:1px solid var(--line);vertical-align:top}th{color:var(--muted);font-weight:500}
.card{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:16px;margin:12px 0}.card h3{margin:0 0 6px;font-size:17px}.meta{color:var(--muted);font-size:13px}
.badge{display:inline-block;padding:1px 8px;border-radius:999px;font-size:12px;border:1px solid var(--line);color:var(--muted)}.badge.accepted{color:var(--ok);border-color:var(--ok)}.badge.rejected{color:var(--warn);border-color:var(--warn)}
dl{display:grid;grid-template-columns:max-content 1fr;gap:6px 16px;font-size:15px}dt{color:var(--muted)}dd{margin:0;white-space:pre-wrap}
.row{display:flex;gap:10px;flex-wrap:wrap;align-items:end}.row label{flex:1;min-width:200px}
`;

export const Layout: FC<PropsWithChildren<{ title: string; user?: User }>> = ({ title, user, children }) => (
  <html lang="en">
    <head>
      <meta charset="utf-8" />
      <meta name="viewport" content="width=device-width, initial-scale=1" />
      <meta name="robots" content="noindex" />
      <title>{title} · Authorship Registry</title>
      <style>{html`${CSS}`}</style>
    </head>
    <body>
      <header>
        <div class="in">
          <a class="brand" href="/">Authorship Registry</a>
          <nav>
            <a href="/">Incidents</a>
            <a href="/report">Report</a>
            {user && (user.role === 'reviewer' || user.role === 'admin') ? <a href="/review">Review</a> : null}
            {user?.role === 'admin' ? <a href="/admin/users">Users</a> : null}
            {user?.role === 'admin' ? <a href="/admin/audit">Audit</a> : null}
          </nav>
          <div class="me">
            {user ? (
              <>
                <span>{user.email} · {user.role}</span>
                <form method="post" action="/logout" style="display:inline"><button class="secondary inline">Sign out</button></form>
              </>
            ) : (
              <a href="/login">Staff sign in</a>
            )}
          </div>
        </div>
      </header>
      <main>{children}</main>
      <footer>Reports are confidential. Reporter identities are visible to the review team only and are never shown to editors.</footer>
    </body>
  </html>
);

export const page = (node: unknown) => html`<!doctype html>${node}`;

export const fmtDate = (d: Date | string | null | undefined) => (d ? new Date(d).toISOString().slice(0, 10) : '');
