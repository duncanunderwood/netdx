# netdx analytics (Supabase)

netdx sends a small amount of anonymous, aggregate usage analytics to a Supabase project so we
can see which features get used and roughly how well speed tests perform in the wild. This is
disclosed here so it's never a surprise.

## What is sent

One row per event, via a fire-and-forget POST to the Supabase REST API (PostgREST) using a
public, insert-only `publishable`/`anon` key (safe to embed in a distributed binary — it cannot
read data back, only insert):

- `event_type` — `app_start`, `traceroute_start`, `telnet_connect`, `speedtest_complete`,
  `update_available`, `update_install`.
- `app_version`, `os`, `arch` — e.g. `0.2.0`, `windows`, `x86_64`.
- `payload` — event-specific, coarse numeric data only, e.g. `speedtest_complete` includes the
  measured `download_mbps`/`upload_mbps`/`ping_ms`/`jitter_ms`/`packet_loss_pct` and which server
  was used.

**Never sent:** hostnames, IP addresses, traceroute/telnet targets, MAC addresses, interface
names, or anything else that identifies the machine, its network, or its user.

Every send is best-effort: failures (no internet, DNS failure, Supabase unreachable, table not
yet provisioned) are silently swallowed and never surfaced to the user or logged as an error — a
network diagnostic tool is routinely run on exactly the kind of broken network that would make
this fail, and that must never itself become something netdx reports as a problem.

## Opting out

Pass `--no-analytics`, or set the environment variable `NETDX_NO_ANALYTICS=1`. Either disables
every `track()` call for that run; nothing is sent, not even a "disabled" event.

## One-time Supabase setup

The netdx binary only ever *inserts* rows — it never creates the table. Run this once against
the target Supabase project (SQL Editor, or `supabase db push` with this as a migration) before
analytics will actually land anywhere; until then every insert 404s and is silently dropped
exactly like any other network failure, so it's safe to ship the binary before running this.

```sql
create table if not exists public.netdx_events (
  id bigint generated always as identity primary key,
  created_at timestamptz not null default now(),
  event_type text not null,
  app_version text,
  os text,
  arch text,
  payload jsonb
);

alter table public.netdx_events enable row level security;

-- The publishable/anon key embedded in the binary can only insert, never read — so the
-- analytics data stays private to whoever has dashboard/service-role access to the project.
create policy "netdx anon insert" on public.netdx_events
  for insert
  to anon
  with check (true);
```

Query it from the Supabase SQL Editor or dashboard table view, e.g.:

```sql
select event_type, count(*), avg((payload->>'download_mbps')::float8) as avg_download_mbps
from public.netdx_events
where event_type = 'speedtest_complete'
group by event_type;
```
