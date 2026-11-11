# Microsoft Store package (MSIX)

The `msix` job in `.github/workflows/release.yml` packs the same `ekos.exe` that goes into the
Windows zip (built with a static C runtime, so it needs no `vcruntime140.dll`) into an unsigned
`ekos-X.Y.Z.0-x64.msix`. Partner Center signs it during certification. It is uploaded as the
workflow artifact `store-msix-x64` and is **not** attached to the GitHub Release: an unsigned
MSIX cannot be installed outside the Store.

## Identity

The manifest carries the identity from Partner Center → EKOS → Product identity
(`AlexeyBanaev.EKOS.DEV`, publisher `CN=77359C36-EF40-49A6-8F61-6727869C79BA`, display name
`Alexey Banaev`, reserved app name `EKOS.DEV` as the display name); they must stay equal character for character. Package family name
`AlexeyBanaev.EKOS.DEV_hbn1emg27zd0m`, Store ID `9NX411QFSW9C`,
<https://apps.microsoft.com/detail/9NX411QFSW9C>.

If a value is ever reset to a `REPLACE_WITH…` placeholder, a manual run of the Release workflow
fails with that message and a tag run only warns and builds no MSIX.

## Each submission

1. Actions → Release → Run workflow (or push a `v*` tag). The Store version is the tag's
   `X.Y.Z` plus `.0` (a manual run uses `ekos/Cargo.toml`'s version); it must grow with every
   submission.
2. Download `store-msix-x64` and upload the `.msix` under Packages.
3. Store listing logo: `ekos-store-300x300.png` in this directory.
4. Notes for certification: *EKOS is a command-line tool. Run `ekos --help` in a terminal (the
   package registers the `ekos` alias). Started from the Start menu it prints its help and the
   window closes at once — that is expected, not a crash.*

## Scope

Only the CLI/MCP binary is packaged. The full package (standalone Python, the web console) is not;
it would make the MSIX much larger.

`makeappx` has not been run locally (there is no Windows machine here); the first workflow run
is the test.
