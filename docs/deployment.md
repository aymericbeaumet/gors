# Website deployment

GitHub Pages uses **GitHub Actions** as its publishing source (`build_type:
workflow`). There is no `gh-pages` branch. Keep this setting under repository
Settings → Pages → Build and deployment → Source.

On a push to `main`, `.github/workflows/ci.yml` builds `www/dist` and uploads a
Pages artifact. After the compiler and browser gates pass, `deploy-www` verifies
that its commit still matches `main`, then deploys that artifact with
`actions/deploy-pages`. Deployment is serialized and retains the custom domain
`gors.aymericbeaumet.com`.

The Pages `source.branch` API field can retain legacy metadata even with
workflow publishing enabled; `build_type` determines the active publishing
mode. Do not recreate an output branch to deploy the site. The repository can
retain only `main` between builds and completed feature branches can be deleted.

Native release builds are separate from Pages. See [releasing](releasing.md) for
the mise tasks and six-platform matrix. Manual release workflow runs validate
artifacts without publishing a release or creating a branch.
