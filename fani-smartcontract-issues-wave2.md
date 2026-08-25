# FaniLab Smart Contracts — Wave 2 Backlog (Fully Published)

Authored for the Drips Stellar Wave. Every issue was derived from a direct read
of the repository at commit `510af08` (`main`, immediately after PR #187 merged
the Holdback refund authorization fix), cross-checked against the already
published backlog in `fani-smartcontract-issues.md` (GitHub issues #7–#144) and
against the live issue tracker at `github.com/fanilabs/fanilab-smartcontract`.

**All 126 issues have been filed on GitHub and removed from this document. Zero
unpublished issues remain.**

## Published to GitHub

| Local backlog range | GitHub issues | Notes |
|---|---|---|
| #188–#237 | [#188–#237](https://github.com/fanilabs/fanilab-smartcontract/issues) | numbers aligned 1:1 |
| #238–#287 | [#238–#287](https://github.com/fanilabs/fanilab-smartcontract/issues) | numbers aligned 1:1 |
| #288–#313 | [#289–#314](https://github.com/fanilabs/fanilab-smartcontract/issues) | offset by +1; GitHub #288 was taken by an unrelated pull request |

Issues were filed to GitHub only. The `Stellar Wave` label was deliberately not
applied, so none of them is enrolled in the Drips wave programme.

Scope note: the HIGH-severity `refund_escrow` / `Holdback` authorization
vulnerability fixed in PR #187 was deliberately **not** reproduced in this
backlog. Issues that touch `Holdback` concern *different* defects in the
surrounding state machine that the fix did not address.
