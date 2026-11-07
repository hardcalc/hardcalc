# Security policy

## Supported versions

hardcalc is young; security fixes go into the latest release only.

| Version | Supported |
|---------|-----------|
| latest release | ✓ |
| older releases | – |

## Reporting a vulnerability

Please do **not** open a public issue for security problems. Report them
privately instead:

- through GitHub: *Security → Report a vulnerability* in
  [hardcalc/hardcalc](https://github.com/hardcalc/hardcalc/security/advisories/new), or
- by email to **security@hardcalc.org**.

Please describe the problem, the affected version and, if possible, a file or
the steps that reproduce it. You will get an answer within a week. Once a fix
is released, the advisory is published and you are credited, unless you
prefer otherwise.

## Scope

hardcalc and hardconf read files that may come from others: Markdown
documents, spreadsheets (xlsx, xls, ods, csv), TextBundles and TOML
configuration files. Problems such as crashes, hangs or excessive memory use
on crafted files, writing outside the intended folder, or running commands
are in scope. So is the LibreOffice extension.
