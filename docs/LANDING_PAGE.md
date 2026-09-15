# Landing page

The public website lives in `site/`: plain HTML and CSS, with no build step or runtime dependencies. The terminal illustration is an illustrative workspace, not a screenshot. The icon is copied from `apps/desktop/src-tauri/icons/icon-source.svg`.

## Preview and edit

Serve `site/` with any static HTTP server. For example, from the repo root:

```sh
bunx serve site
```

Edit `site/index.html` for content and `site/style.css` for styling. Keep asset links relative (`./style.css`) so they work at the custom domain root and under GitHub Pages’ `/workbench/` path. Download buttons point to the latest GitHub release so new releases need no website update.

## Deployment

URL: <https://workbench.starkeydigital.com/>

The repository’s **Settings → Pages → Build and deployment → Source** must be **GitHub Actions**. `.github/workflows/pages.yml` publishes only `site/` when its files or the workflow change on `main`; it can also be run manually from Actions on `main`. No desktop build or credentials are needed. The `github-pages` environment limits deployment to the main branch.

The workflow follows [GitHub’s custom Pages workflow](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages).

### Custom domain

GitHub Pages settings bind `workbench.starkeydigital.com` to this repository. Cloudflare DNS has a DNS-only (unproxied) CNAME from `workbench.starkeydigital.com` to `datstarkey.github.io`, with a 300-second TTL. The target must not include `/workbench/`. This explicit record takes precedence over the zone’s wildcard record.

GitHub provisions the TLS certificate; enable **Enforce HTTPS** when the certificate is ready. With an Actions publishing source, GitHub stores the custom domain in repository settings and does not require a `CNAME` file. If changing the domain, update Pages settings, DNS, the repository homepage, this document, the README, and the canonical/Open Graph URLs in `site/index.html`.
