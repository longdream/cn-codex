---
name: sites-hosting
description: Manage hosting for sites, web apps, and games. Good for new projects that need hosting. Use when the user wants to deploy or manage hosting using OpenAI/Codex/ChatGPT infrastructure, or when the project contains `.openai/hosting.json`.
---

# Sites Hosting

Use this skill to create or reuse a hosting configuration, save deployable versions, deploy them to production, inspect deployment status, and manage access controls.

Use this plugin's Sites connector for all hosting operations. Use the
current connector schema for the exact tool names and arguments.

## Configuration Contract

Store hosting metadata in `.openai/hosting.json`.

- `project_id`: the remote Sites project id. It is absent until the site has been provisioned.
- `d1`: optional logical Cloudflare D1 binding name.
- `r2`: optional logical Cloudflare R2 binding name.

## Compatible Projects

Host projects that can build deployment artifacts for Cloudflare Workers.

- Prefer this plugin's bundled vinext starters for new Sites projects.
- For existing Next.js sites, use a Cloudflare Workers-compatible build path such
  as OpenNext when needed.
- For other frameworks, use the project's Cloudflare-compatible build path when
  one exists. Refer to Cloudflare's web application framework guides:
  https://developers.cloudflare.com/workers/framework-guides/web-apps/
- Do not use frameworks that cannot produce Cloudflare Worker deployment
  artifacts for hosted sites.

## Environment Variables

Runtime environment variables are managed through the Sites connector,
not `.openai/hosting.json`.

- Use the connector's get-environment operation to inspect current runtime keys
  and values.
- Use the connector's update-environment operation to create, update, or remove
  runtime env vars.
- Redeploy after environment changes so the next deployment picks up the updated
  revision.
- Keep `.env` and `.env.example` aligned with the runtime keys needed for local
  development.

## Project Resolution

Before any publish or deploy flow, read the project's `.openai/hosting.json`.

- If `project_id` is present, treat it as the provisioned project for the site and use it.
- If `project_id` is absent, provision a new project for the site.
- Avoid provisioning multiple projects for the same site, unless that is what the
  user explicitly requested.

## Creating a Project

Create a project only when `.openai/hosting.json` is missing or does not already
contain `project_id`.

1. Read `.openai/hosting.json` if it exists and confirm that `project_id` is
   absent.
2. Use the connector's create-project operation to provision a new project with
   a title, slug, and description.
3. Immediately write the returned `project_id` in `.openai/hosting.json`.
4. If create-project returns a source repository credential, keep it for the
   first source push.

## Local Validation

Before saving a version of the site:

1. If the D1 schema changed, generate and inspect the local migration output.
2. Run the site's normal build command, usually `npm run build`, to confirm it
   can cleanly build.

## Source Repository Workflow

App versions are associated with Git commits via a SHA.

- For a new project, use the source repository credential returned by project
  creation.
- For an existing project, mint a fresh source repository write credential
  before fetching or pushing source.
- Use the returned `remote_url`, `branch`, `token`, `token_expires_at`, and
  `auth_mode`.
- Keep the returned token out of URLs, config files, commits, logs, and
  user-facing summaries.
- Prefer one-shot Git authentication with an HTTP authorization header.

Push the source used for a build before saving a version. Use the pushed commit
SHA as the version's `commit_sha`.

## Artifact Preparation

Prepare the uploaded artifact from the exact source state being published.

1. For new sites and meaningful visual changes, create or refresh the canonical site preview at `public/screenshot.jpeg`.
   - Use a representative state of the working implementation, not a source mock.
   - Capture the visible browser viewport directly at `1200x750` (`16:10`), then reset any temporary viewport override. Do not stretch or distort an existing screenshot to fit. If direct capture is unavailable, preserve the original aspect ratio with a center crop or letterboxing.
   - Keep the canonical filename and location stable. Do not rename the file or store it elsewhere in the repo.
2. Run the site's deployment build command to produce up-to-date build
   artifacts, usually `npm run build`. If `package.json` defines a
   Cloudflare-specific build script for deployment, use that instead.
3. Stage the built `dist/` directory in a temporary location.
4. If `.openai/hosting.json` is not already copied, copy it into
   `dist/_appgen_meta/appgarden.json`; deployment binding discovery reads the
   staged metadata from that path.
5. If `drizzle/` exists and is not already copied, copy it into
   `dist/_appgen_meta/drizzle/`.
6. Verify the staged artifact contains:
    - `dist/server/index.js`
    - `dist/client/**` or `dist/server/public/**` when static assets exist
    - `dist/_appgen_meta/appgarden.json`
    - `dist/_appgen_meta/drizzle/**` when migrations exist
7. Create a tarball archive from the staged directory, for example:
   `tar -C "$staged_root" -czf "$archive_path" dist`.

## Version Publishing

Save a deployable version only after local validation, source push, and artifact preparation are complete.

1. Use the connector's create-project-version operation with the pushed Git
   commit SHA and matching build archive.
2. Keep the returned version id for follow-up calls.
3. In user-facing summaries, report the version number rather than the internal
   version id.

If the user asks to save a version without deploying it, stop here.

Use version listing and inspection when the user asks for history, rollback
candidates, or the currently saved versions for a project.

## Production Deployment

Deploy only saved versions.

After creating or modifying a site, deploy the newly saved version unless the
user explicitly asks not to deploy.

1. Use the connector's deploy-project-version operation to begin a deployment
   for the selected saved version.
2. Use the connector's deployment-status operation to poll the deployment until
   it reaches a terminal state or the user asks to stop.
3. When deployment succeeds, report the production URL.
4. When deployment fails, report the failure message and enough context to
   continue debugging.

Treat every Sites deployment URL as a production deployment.

## Access Control

Use access controls when the user asks to change who can access the deployed site.

- Use `admins_only` when access should be limited to workspace admins and the
  owner of the site. This should be the default for brand new sites, since it is
  the safest option until the user says otherwise. Tell the user they should
  expand access before sharing the URL.
- Use `workspace_all` when every active workspace member should have access.
- Use `custom` when access should be limited to specific users or groups.
- Before applying group allowlists, list available workspace and tenant groups
  for the project, present their names and sizes to the user, then update
  access using the ids for the groups the user asks for.
- When updating a custom allowlist, preserve any existing allowlist fields the
  user did not ask to change.
- The project owner remains allowed automatically.
