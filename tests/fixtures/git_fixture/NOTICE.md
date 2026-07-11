# Provenance: `odoo_utm.bundle`

**Source**: https://github.com/odoo/odoo, branch `17.0`, directory `addons/utm/` (Odoo's UTM /
marketing-campaign-tracking module).

**License**: LGPL-3.0 (see `LICENSE-LGPL-3.0.txt` in this directory — vendored verbatim from
`https://github.com/odoo/odoo/blob/17.0/LICENSE`). This license applies to the contents of this
`git_fixture/` directory only; it is independent of and does not change EKOS's own license.

**What this is**: a real git repository — 39 real commits with real authors, dates, and commit
messages — path-filtered down to just the `addons/utm/` subtree using
[`git-filter-repo`](https://github.com/newren/git-filter-repo). This is **not** the full Odoo
codebase (which is many gigabytes and would be impractical to vendor); it's a small, self-contained
slice of one real module's real history, used as "near-real, open-source" test data for EKOS's
`GitObserver`/`GitAnalyzerPass` — genuine commit metadata and genuine multi-file coupling (the
module's initial commit touches 28 files together: models, views, tests, and data all landing in
one real change), rather than the throwaway synthetic 1–20 commit repos EKOS's other git tests
construct via `git init`/`git commit`.

**How it was produced** (for reproducibility):

```bash
git clone --no-checkout --depth=3000 --single-branch --branch 17.0 https://github.com/odoo/odoo.git odoo_fullblob
cd odoo_fullblob
git filter-repo --path addons/utm/ --force
git bundle create odoo_utm.bundle --all
```

**How EKOS tests use it**: never over the network. At test setup, materialize a real working repo
from the bundle:

```bash
git clone tests/fixtures/git_fixture/odoo_utm.bundle <tempdir>
```

then point `GitObserver`/`GitAnalyzerPass` at `<tempdir>` exactly as they would a live repository.
