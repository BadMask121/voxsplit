# voxsplit.triplejmusic.art

The download page, the install script and the installer downloads, as one
Cloudflare Worker (`voxsplit-site`) on the `triplejmusic.art` zone.

- `public/` static page, styles, `install.template.sh`, images. Plain HTML/CSS/JS.
- `src/worker.js` runs first on `/`, `/install.sh` and `/download/*`: it fills the
  current release from `release.json` into the page and the install script, and
  serves `VoxSplit-<version>.pkg` from the R2 bucket `voxsplit-downloads` (range
  requests supported, so downloads resume). `/download/latest` redirects to it.
- `release.json` is written by `../scripts/release.sh`, which also uploads the
  package and deploys. wrangler uploads at most 300 MiB per object.

```bash
npm install
npm test             # worker unit tests
npx wrangler dev     # local preview (downloads 404: no local R2 object)
```
