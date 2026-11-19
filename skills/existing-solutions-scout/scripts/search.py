"""Search public developer sources for existing solutions. No agent, no ranking: raw, de-duplicated candidates,
with enough detail per candidate (activity, issues, community comments) to judge without opening the pages again.

Usage:
  python -I search.py "query" ["another query" ...] [--sources github,hn,npm,crates,mcp] [--limit 15]
                      [--details 5] [--out results.json]

Sources: github (via `gh`, needs gh auth), hn (Hacker News, Algolia API), reddit (through Arctic Shift, a community
archive: Reddit closed its own keyless API and ends RSS on 2026-11-13; search runs per subreddit, see --subreddits
and --since), npm (registry), crates (crates.io), mcp (official MCP registry).
--details N enriches the first N results of each source (in the order the source returned them, not a ranking):
  github: stars, forks, open issues, license, archived, last push, latest release, topics, the most-commented
          issues (title, state, comment count) and the start of the README;
  hn:     the top-level comments of the discussion (trimmed);
  reddit: the highest-scored comments of the thread (trimmed);
  npm:    weekly downloads and the repository;
  crates: recent downloads.
Prints a Markdown digest grouped by source; --out also writes everything as JSON. Errors are reported per item,
never fatal.
"""
import argparse
import datetime
import html
import json
import re
import subprocess
import sys
import time
import urllib.parse
import urllib.request

UA = {"User-Agent": "existing-solutions-scout/1.0 (research script)", "Accept": "application/json"}


def get(url, retries=0):
    """GET JSON. With retries > 0, waits and retries when the server says it is busy (timeouts, 422/429/5xx)."""
    for attempt in range(retries + 1):
        try:
            req = urllib.request.Request(url, headers=UA)
            with urllib.request.urlopen(req, timeout=60) as r:
                return json.loads(r.read().decode("utf-8"))
        except urllib.error.HTTPError as e:
            if attempt == retries or e.code not in (422, 429, 500, 502, 503, 504):
                raise
        except (TimeoutError, urllib.error.URLError):
            if attempt == retries:
                raise
        time.sleep(5 * (attempt + 1))


def gh(*args):
    r = subprocess.run(["gh", *args], capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=60)
    if r.returncode != 0:
        raise RuntimeError(r.stderr.strip()[:200])
    return r.stdout


def clean(text, n):
    text = html.unescape(re.sub(r"<[^>]+>", " ", text or ""))
    return re.sub(r"\s+", " ", text).strip()[:n]


# ---- search -------------------------------------------------------------------------------------------------

def s_github(q, n):
    data = json.loads(gh("search", "repos", q, "--limit", str(n), "--json", "fullName,description,stargazersCount,updatedAt,url"))
    return [{"title": x["fullName"], "url": x["url"], "desc": x.get("description") or "",
             "signal": f"{x.get('stargazersCount', 0)} stars", "date": (x.get("updatedAt") or "")[:10], "ref": x["fullName"]}
            for x in data]


def s_hn(q, n):
    d = get("https://hn.algolia.com/api/v1/search?" + urllib.parse.urlencode({"query": q, "tags": "story", "hitsPerPage": n}))
    return [{"title": h.get("title") or "", "url": h.get("url") or f"https://news.ycombinator.com/item?id={h['objectID']}",
             "desc": f"discussion: https://news.ycombinator.com/item?id={h['objectID']}",
             "signal": f"{h.get('points', 0)} points, {h.get('num_comments', 0)} comments",
             "date": (h.get("created_at") or "")[:10], "ref": h["objectID"]} for h in d.get("hits", [])]


def s_npm(q, n):
    d = get("https://registry.npmjs.org/-/v1/search?" + urllib.parse.urlencode({"text": q, "size": n}))
    out = []
    for o in d.get("objects", []):
        p = o["package"]
        links = p.get("links") or {}
        out.append({"title": p["name"], "url": links.get("repository") or links.get("npm", ""), "desc": p.get("description") or "",
                    "signal": f"v{p.get('version', '')}", "date": (p.get("date") or "")[:10], "ref": p["name"]})
    return out


def s_crates(q, n):
    d = get("https://crates.io/api/v1/crates?" + urllib.parse.urlencode({"q": q, "per_page": n}))
    return [{"title": c["name"], "url": c.get("repository") or f"https://crates.io/crates/{c['name']}",
             "desc": c.get("description") or "", "signal": f"{c.get('downloads', 0)} downloads",
             "date": (c.get("updated_at") or "")[:10], "ref": c["name"],
             "details": {"recent_downloads_90d": c.get("recent_downloads")}} for c in d.get("crates", [])]


ARCTIC = "https://arctic-shift.photon-reddit.com/api"
REDDIT = {"subreddits": ["ClaudeAI", "ClaudeCode", "LocalLLaMA", "ChatGPTCoding", "mcp", "programming"], "since": ""}


def s_reddit(q, n):
    """Reddit through Arctic Shift (a community archive of Reddit; Reddit's own keyless endpoints are closed).
    Searches post titles (the archive's body search barely filters), once per subreddit, within --since.
    Use short title words as the query (for example "reddit mcp", not a sentence)."""
    out = []
    for sub in REDDIT["subreddits"]:
        params = {"subreddit": sub, "title": q, "limit": n, "after": REDDIT["since"]}
        d = get(f"{ARCTIC}/posts/search?" + urllib.parse.urlencode(params), retries=3)
        for p in d.get("data") or []:
            date = datetime.datetime.fromtimestamp(int(p.get("created_utc", 0)), datetime.timezone.utc).strftime("%Y-%m-%d")
            link = p.get("url") if p.get("url") and "reddit.com" not in p.get("url", "") else ""
            out.append({"title": p.get("title", ""), "url": "https://www.reddit.com" + p.get("permalink", ""),
                        "desc": f"r/{p.get('subreddit', sub)}" + (f" | link: {link}" if link else "")
                                + (f" | {clean(p.get('selftext'), 220)}" if p.get("selftext") else ""),
                        "signal": f"{p.get('score', 0)} score, {p.get('num_comments', 0)} comments", "date": date,
                        "ref": p.get("id", "")})
        time.sleep(1.0)  # the archive asks clients to slow down
    return out


def d_reddit(item):
    d = get(f"{ARCTIC}/comments/search?" + urllib.parse.urlencode({"link_id": item["ref"], "limit": 25}), retries=3)
    human = [c for c in d.get("data") or [] if c.get("author") != "AutoModerator" and "I am a bot" not in (c.get("body") or "")]
    comments = sorted(human, key=lambda c: c.get("score", 0), reverse=True)
    return {"top_comments": [f"[{c.get('score', 0)}] {clean(c.get('body'), 320)}" for c in comments[:6] if c.get("body")]}


def s_mcp(q, n):
    d = get("https://registry.modelcontextprotocol.io/v0/servers?" + urllib.parse.urlencode({"search": q, "limit": n}))
    out = []
    for s in d.get("servers", []):
        s = s.get("server", s)
        out.append({"title": s.get("name", ""), "url": (s.get("repository") or {}).get("url", "") or s.get("websiteUrl", ""),
                    "desc": s.get("description") or "", "signal": f"v{s.get('version', '')}", "date": "", "ref": s.get("name", "")})
    return out


# ---- details ------------------------------------------------------------------------------------------------

def d_github(item):
    repo = item["ref"]
    meta = json.loads(gh("api", f"repos/{repo}"))
    det = {"forks": meta.get("forks_count"), "open_issues": meta.get("open_issues_count"),
           "license": (meta.get("license") or {}).get("spdx_id"), "archived": meta.get("archived"),
           "pushed": (meta.get("pushed_at") or "")[:10], "topics": meta.get("topics") or []}
    try:
        rel = json.loads(gh("api", f"repos/{repo}/releases/latest"))
        det["latest_release"] = f"{rel.get('tag_name')} ({(rel.get('published_at') or '')[:10]})"
    except Exception:
        det["latest_release"] = None
    try:
        q = urllib.parse.quote(f"repo:{repo} is:issue")
        issues = json.loads(gh("api", f"search/issues?q={q}&sort=comments&order=desc&per_page=5"))
        det["top_issues"] = [f"[{i['state']}, {i['comments']} comments] {i['title']}" for i in issues.get("items", [])]
    except Exception as e:
        det["top_issues"] = [f"(could not read issues: {str(e)[:80]})"]
    try:
        readme = gh("api", f"repos/{repo}/readme", "-H", "Accept: application/vnd.github.raw")
        det["readme_start"] = clean(re.sub(r"!\[[^\]]*\]\([^)]*\)", "", readme), 700)
    except Exception:
        det["readme_start"] = None
    return det


def d_hn(item):
    data = get(f"https://hn.algolia.com/api/v1/items/{item['ref']}")
    comments = [c for c in data.get("children", []) if c.get("text")]
    return {"top_comments": [f"{c.get('author', '?')}: {clean(c['text'], 320)}" for c in comments[:6]]}


def d_npm(item):
    det = {}
    try:
        dl = get(f"https://api.npmjs.org/downloads/point/last-week/{urllib.parse.quote(item['ref'], safe='@/')}")
        det["weekly_downloads"] = dl.get("downloads")
    except Exception:
        det["weekly_downloads"] = None
    return det


SOURCES = {"github": (s_github, d_github), "hn": (s_hn, d_hn), "reddit": (s_reddit, d_reddit), "npm": (s_npm, d_npm),
           "crates": (s_crates, None), "mcp": (s_mcp, None)}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("queries", nargs="+")
    ap.add_argument("--sources", default="github,hn,reddit,npm,crates,mcp")
    ap.add_argument("--limit", type=int, default=15)
    ap.add_argument("--details", type=int, default=5, help="enrich the first N results of each source (0 = off)")
    ap.add_argument("--subreddits", help="comma-separated subreddits for the reddit source (default: AI coding ones)")
    ap.add_argument("--since", type=int, default=365, help="reddit: only posts from the last N days (default 365)")
    ap.add_argument("--out")
    args = ap.parse_args()
    sys.stdout.reconfigure(encoding="utf-8")
    if args.subreddits:
        REDDIT["subreddits"] = [s.strip() for s in args.subreddits.split(",") if s.strip()]
    REDDIT["since"] = (datetime.date.today() - datetime.timedelta(days=args.since)).isoformat()

    results, errors, seen = {}, {}, set()
    for src in [s.strip() for s in args.sources.split(",") if s.strip()]:
        if src not in SOURCES:
            errors[src] = "unknown source"
            continue
        search, detail = SOURCES[src]
        for q in args.queries:
            try:
                for item in search(q, args.limit):
                    key = (item["url"] or item["title"]).lower().rstrip("/")
                    if key in seen:
                        continue
                    seen.add(key)
                    results.setdefault(src, []).append({**item, "query": q})
            except Exception as e:
                errors[f"{src}: {q}"] = str(e)[:200]
        if detail:
            for item in results.get(src, [])[:args.details]:
                try:
                    item["details"] = {**item.get("details", {}), **detail(item)}
                except Exception as e:
                    item["details"] = {"error": str(e)[:200]}

    for src, items in results.items():
        print(f"\n## {src} ({len(items)})")
        for i in items:
            print(f"- {i['title']} — {i['url']}  [{i['signal']}{', ' + i['date'] if i['date'] else ''}]")
            if i["desc"]:
                print(f"  {i['desc'][:220]}")
            for k, v in (i.get("details") or {}).items():
                if v in (None, [], ""):
                    continue
                if isinstance(v, list):
                    print(f"  {k}:")
                    for line in v:
                        print(f"    - {line}")
                else:
                    print(f"  {k}: {v}")
    if errors:
        print("\n## errors")
        for k, v in errors.items():
            print(f"- {k}: {v}")
    if args.out:
        with open(args.out, "w", encoding="utf-8") as f:
            json.dump({"queries": args.queries, "results": results, "errors": errors}, f, indent=2, ensure_ascii=False)
        print(f"\njson: {args.out}")


if __name__ == "__main__":
    main()

