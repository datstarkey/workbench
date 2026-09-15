# Landing page

The public website lives in `site/`: plain HTML and CSS, with no build step or runtime dependencies. The terminal illustration is an illustrative workspace, not a screenshot. The icon is copied from `apps/desktop/src-tauri/icons/icon-source.svg`.

## Preview and edit

Serve `site/` with any static HTTP server. For example, from the repo root:

```sh
bunx serve site
```

Edit `site/index.html` for content and `site/style.css` for styling. Keep asset links relative (`./style.css`) so they work under GitHub Pages’ `/workbench/` path. Download buttons point to the latest GitHub release so new releases need no website update.

## Deployment

URL: <https://datstarkey.github.io/workbench/>

The repository’s **Settings → Pages → Build and deployment → Source** must be **GitHub Actions**. `.github/workflows/pages.yml` publishes only `site/` when its files or the workflow change on `main`; it can also be run manually from Actions on `main`. No desktop build or credentials are needed. The `github-pages` environment limits deployment to the main branch.

The workflow follows [GitHub’s custom Pages workflow](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages). If adding a custom domain later, configure it in Pages settings and update the canonical and Open Graph URLs in `site/index.html`.
