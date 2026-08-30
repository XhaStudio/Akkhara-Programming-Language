# Getting `curl -fsSL https://your-domain/install.sh | sh` working for Akkhara

Three pieces, in order.

> **Before doing anything else:** open `scripts/install.sh` and
> `scripts/install.ps1` and replace `yourusername/akkhara` with your real
> GitHub `owner/repo`. Both scripts will fail to find a download until you do.

## 1. Publish prebuilt binaries (GitHub Releases)

Drop `.github/workflows/release.yml` into your `akkhara` repo. It builds
`akk` for Linux x86_64, macOS x86_64, and macOS arm64, and attaches each as
`akk-<target>.tar.gz` to a GitHub Release whenever you push a version tag:

```
git add .github/workflows/release.yml
git commit -m "Add release workflow"
git tag v0.1.0
git push origin main --tags
```

Check the "Actions" and then "Releases" tab on GitHub afterward -- you
should see the three `.tar.gz` files attached.

## 2. Add `install.sh`

Put `scripts/install.sh` (included) at the root of your repo, or wherever
you'll serve it from. Before using it, edit this line near the top:

```sh
REPO="${AKK_REPO:-yourusername/akkhara}"
```

to your actual `github-username/akkhara`. It downloads whichever
`akk-<target>.tar.gz` matches the user's OS/arch from your latest release,
extracts it, and installs the binary to `~/.local/bin`.

Test it locally first:
```sh
chmod +x scripts/install.sh
AKK_REPO=yourusername/akkhara ./scripts/install.sh
```

## 3. Get a URL that serves the script

You have two options, cheapest first:

### Option A -- no domain needed, works today
GitHub serves raw file contents directly:
```
curl -fsSL https://raw.githubusercontent.com/yourusername/akkhara/main/scripts/install.sh | sh
```
This just works the moment the file is pushed to `main`. Good enough for
most projects and many real tools ship exactly this.

### Option B -- branded domain (the `rux-lang.dev` look)
1. Buy a domain (Namecheap, Cloudflare Registrar, etc).
2. Enable **GitHub Pages** on the repo (Settings -> Pages), serving from a
   `docs/` folder or a `gh-pages` branch. Put `install.sh` in that folder's
   root so it's served at `/install.sh`.
3. Add a `CNAME` file in that same folder containing just your domain, e.g.:
   ```
   akkhara-lang.dev
   ```
4. At your domain registrar, add DNS records pointing at GitHub Pages:
   - `A` records for the apex domain pointing to GitHub's Pages IPs
     (185.199.108.153, .109.153, .110.153, .111.153), or
   - a `CNAME` record for a subdomain like `install.akkhara-lang.dev`
     pointing to `yourusername.github.io`.
5. Wait for DNS to propagate (minutes to a few hours), then:
   ```
   curl -fsSL https://akkhara-lang.dev/install.sh | sh
   ```

Cloudflare Pages or Netlify work the same way if you'd rather use those
instead of GitHub Pages -- point the custom domain at whichever static host
serves the file.

## 4. Windows one-liner (`irm ... | iex`)

The release workflow now also builds `akk.exe` for
`x86_64-pc-windows-msvc` and zips it as `akk-x86_64-pc-windows-msvc.zip`.

`scripts/install.ps1` (included) is the Windows counterpart to
`install.sh` -- it downloads that zip from your latest release, extracts
`akk.exe` into `%LOCALAPPDATA%\Akkhara\bin`, and adds that folder to the
user's PATH. Edit the same repo default near the top:

```powershell
$Repo = if ($env:AKK_REPO) { $env:AKK_REPO } else { "yourusername/akkhara" }
```

Test locally first:
```powershell
$env:AKK_REPO = "yourusername/akkhara"
.\scripts\install.ps1
```

Once hosted (raw GitHub URL or your custom domain, same as step 3), users run:
```powershell
irm https://raw.githubusercontent.com/yourusername/akkhara/main/scripts/install.ps1 | iex
```
or, with the branded domain:
```powershell
irm https://akkhara-lang.dev/install.ps1 | iex
```

**Important distinction:** the project's *existing* `install.ps1` /
`command.ps1` at the repo root build Akkhara from source and are meant for
people who cloned the repo to develop it. This *new* `scripts/install.ps1`
downloads a prebuilt binary and is meant for end users who just want to run
`akk` -- keep both, but only advertise the new one as the one-liner.
