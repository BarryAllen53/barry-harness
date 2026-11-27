# Security Policy

## Supported versions

Security fixes are made on `main` and shipped in the next release. Only the latest release is supported.

| Version | Supported |
| --- | --- |
| 0.1.x | Yes |

## Reporting a vulnerability

Do not open a public issue for a vulnerability, and do not post vulnerability details anywhere public.

- **Once the repository is public:** report it through GitHub private vulnerability reporting. Open the repository's **Security** tab and choose **Report a vulnerability**.
- **Until then, or if that form is not available:** contact the maintainer through the GitHub profile [BarryAllen53](https://github.com/BarryAllen53) and ask for a private channel. Share the details only once a private channel is agreed.

Please include:

- the affected version or commit,
- the component (provider crate, sign-in, credential storage, desktop shell, frontend),
- steps to reproduce or a proof of concept,
- the impact you expect.

Never include real API keys, OAuth tokens or client secrets in a report. Redact them, or create throwaway credentials for the reproduction.

You will get an acknowledgement within 7 days. Once the issue is confirmed, a fix and a coordinated disclosure date are agreed in the advisory, and you are credited in it unless you prefer otherwise.

## Scope

In scope: credential handling and storage, OAuth flows and the loopback redirect listener, leaks of keys or tokens into logs or error messages, network egress rules of the provider clients, the Tauri capability and content security policy, and the IPC commands.

Out of scope: vulnerabilities in the model providers' own services, and issues that require an attacker who already controls the user's Windows account.
