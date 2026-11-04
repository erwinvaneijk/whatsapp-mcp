# Upstream issues and this fork

[lharries/whatsapp-mcp](https://github.com/lharries/whatsapp-mcp) has had no code changes since April 2025, while WhatsApp kept changing. Out of the box it no longer connects ("Client outdated (405)"), and many fixes sit in unmerged pull requests.

This fork, [erwinvaneijk/whatsapp-mcp](https://github.com/erwinvaneijk/whatsapp-mcp), collects those fixes on the [`dev-main`](https://github.com/erwinvaneijk/whatsapp-mcp/tree/dev-main) branch: 32 upstream pull requests so far, reviewed, adapted where needed, tested, and checked by CI, plus fixes made in the fork itself (see [below](#fixes-made-in-this-fork)). Setup is described in the [README](https://github.com/erwinvaneijk/whatsapp-mcp/tree/dev-main#installation).

This page lists every open upstream issue and whether `dev-main` fixes it. Each row links to the upstream issue; to link to a row, add `#issue-<number>` to this page's address.

Status as of 2026-11-04, for 96 open issues:

| Status | Issues |
|---|---|
| [Fixed](#fixed) | 31 |
| [Partly fixed or not checked](#partly-fixed-or-not-checked) | 13 |
| [Not fixed yet](#not-fixed-yet) | 22 |
| [Setup and support questions](#setup-and-support-questions) | 16 |
| [Not about the code](#not-about-the-code) | 14 |

[Fixes made in this fork](#fixes-made-in-this-fork) lists changes that are not upstream pull requests.

## Using the fork

```bash
git clone -b dev-main https://github.com/erwinvaneijk/whatsapp-mcp.git
```

Then follow the [installation steps](https://github.com/erwinvaneijk/whatsapp-mcp/tree/dev-main#installation). Two things differ from upstream: the bridge needs a shared token (`WHATSAPP_BRIDGE_TOKEN`), and files can only be sent from one folder (`WHATSAPP_MEDIA_DIR`). See [Bridge token and media folder](https://github.com/erwinvaneijk/whatsapp-mcp/tree/dev-main#bridge-token-and-media-folder).

## Fixed

| Issue | Title | Status in `dev-main` |
|---|---|---|
| <a id="issue-382"></a>[#382](https://github.com/lharries/whatsapp-mcp/issues/382) | Tracking: verified versions for the 405 and media-403 fixes (merge order) | All fixes this issue tracks are merged: whatsmeow update ([#381](https://github.com/lharries/whatsapp-mcp/pull/381), [#392](https://github.com/lharries/whatsapp-mcp/pull/392)) and the media 403 fix ([#361](https://github.com/lharries/whatsapp-mcp/pull/361)). |
| <a id="issue-344"></a>[#344](https://github.com/lharries/whatsapp-mcp/issues/344) | /api/download returns 403 for all media, even though the stored media URL works fine via direct HTTP — restarting the bridge does not fix it | The signed query string is kept in the download path ([#361](https://github.com/lharries/whatsapp-mcp/pull/361), tests from [#367](https://github.com/lharries/whatsapp-mcp/pull/367)). |
| <a id="issue-327"></a>[#327](https://github.com/lharries/whatsapp-mcp/issues/327) | Documents sent via send_file arrive as unnamed .bin files (missing mime type + FileName) | Documents are sent with their file type and name ([#390](https://github.com/lharries/whatsapp-mcp/pull/390)). |
| <a id="issue-282"></a>[#282](https://github.com/lharries/whatsapp-mcp/issues/282) | correctness+perf: last-message JOIN uses timestamp equality; can duplicate or drop chat rows | Last message selected by row, not by timestamp ([#283](https://github.com/lharries/whatsapp-mcp/pull/283)). |
| <a id="issue-280"></a>[#280](https://github.com/lharries/whatsapp-mcp/issues/280) | perf: content search (LIKE %q%) does a full table scan; add FTS5 | FTS5 full-text search ([#281](https://github.com/lharries/whatsapp-mcp/pull/281)); needs the bridge built with `-tags sqlite_fts5`. |
| <a id="issue-278"></a>[#278](https://github.com/lharries/whatsapp-mcp/issues/278) | perf: message/chat queries do full-table scans; add SQLite indexes | Indexes added ([#279](https://github.com/lharries/whatsapp-mcp/pull/279)). |
| <a id="issue-241"></a>[#241](https://github.com/lharries/whatsapp-mcp/issues/241) | CWE-22 Path Traversal Vulnerability in `/api/send` Endpoint Allows Arbitrary File Read | Files can only be sent from `WHATSAPP_MEDIA_DIR` ([#387](https://github.com/lharries/whatsapp-mcp/pull/387)). |
| <a id="issue-237"></a>[#237](https://github.com/lharries/whatsapp-mcp/issues/237) | [Client ERROR] Client outdated (405) connect failure (client version: 2.3000.1021018791) | Updated whatsmeow ([#381](https://github.com/lharries/whatsapp-mcp/pull/381), [#392](https://github.com/lharries/whatsapp-mcp/pull/392)). |
| <a id="issue-225"></a>[#225](https://github.com/lharries/whatsapp-mcp/issues/225) | Last message issue list_messages | Messages and reactions sent through the bridge are stored ([#383](https://github.com/lharries/whatsapp-mcp/pull/383)). |
| <a id="issue-222"></a>[#222](https://github.com/lharries/whatsapp-mcp/issues/222) | WhatsApp MCP cannot download older sent PDF attachments and returns 403 | Download path fixed ([#361](https://github.com/lharries/whatsapp-mcp/pull/361)); media that has expired on WhatsApp's servers is requested again from the sender's phone ([#273](https://github.com/lharries/whatsapp-mcp/pull/273)). |
| <a id="issue-216"></a>[#216](https://github.com/lharries/whatsapp-mcp/issues/216) | Client outdated (405) error on startup — whatsmeow dependency needs update | Updated whatsmeow ([#381](https://github.com/lharries/whatsapp-mcp/pull/381), [#392](https://github.com/lharries/whatsapp-mcp/pull/392)). |
| <a id="issue-215"></a>[#215](https://github.com/lharries/whatsapp-mcp/issues/215) | [Security] Bridge REST API binds to 0.0.0.0 by default — LAN-exposed unauthenticated /api/send and /api/download | The API listens on `127.0.0.1` only and requires a shared token ([#387](https://github.com/lharries/whatsapp-mcp/pull/387)). |
| <a id="issue-192"></a>[#192](https://github.com/lharries/whatsapp-mcp/issues/192) | receiving error  and not loading the qr code from whatsmeow. giving this error  Client outdated (405) connect failure (client version: 2.3000.1029399661) | Updated whatsmeow ([#381](https://github.com/lharries/whatsapp-mcp/pull/381), [#392](https://github.com/lharries/whatsapp-mcp/pull/392)). |
| <a id="issue-170"></a>[#170](https://github.com/lharries/whatsapp-mcp/issues/170) | whatsmeow breaking API change: missing context.Context in several function calls | Updated whatsmeow with the `context.Context` API changes ([#381](https://github.com/lharries/whatsapp-mcp/pull/381)). |
| <a id="issue-164"></a>[#164](https://github.com/lharries/whatsapp-mcp/issues/164) | 10+ PRs fixing the 405 error are sitting unmerged since September | Updated whatsmeow ([#381](https://github.com/lharries/whatsapp-mcp/pull/381), [#392](https://github.com/lharries/whatsapp-mcp/pull/392)). |
| <a id="issue-154"></a>[#154](https://github.com/lharries/whatsapp-mcp/issues/154) | GetGroupInfo API Breaking Change | Updated whatsmeow with the `context.Context` API changes ([#381](https://github.com/lharries/whatsapp-mcp/pull/381)). |
| <a id="issue-153"></a>[#153](https://github.com/lharries/whatsapp-mcp/issues/153) | Client outdated error (405) and GetGroupInfo API breaking change after whatsmeow update | Updated whatsmeow with the `context.Context` API changes ([#381](https://github.com/lharries/whatsapp-mcp/pull/381)), including `GetGroupInfo`. |
| <a id="issue-149"></a>[#149](https://github.com/lharries/whatsapp-mcp/issues/149) | feat: Add GitHub Actions CI for automated testing | GitHub Actions builds and tests the bridge and the MCP server ([#150](https://github.com/lharries/whatsapp-mcp/pull/150)). |
| <a id="issue-147"></a>[#147](https://github.com/lharries/whatsapp-mcp/issues/147) | feat: Add message reaction support | New `react_to_message` tool ([#148](https://github.com/lharries/whatsapp-mcp/pull/148)). |
| <a id="issue-136"></a>[#136](https://github.com/lharries/whatsapp-mcp/issues/136) | Client version Outdated | Updated whatsmeow ([#381](https://github.com/lharries/whatsapp-mcp/pull/381), [#392](https://github.com/lharries/whatsapp-mcp/pull/392)). |
| <a id="issue-124"></a>[#124](https://github.com/lharries/whatsapp-mcp/issues/124) | I'm facing this error | Updated whatsmeow ([#381](https://github.com/lharries/whatsapp-mcp/pull/381), [#392](https://github.com/lharries/whatsapp-mcp/pull/392)). |
| <a id="issue-121"></a>[#121](https://github.com/lharries/whatsapp-mcp/issues/121) | How to add support for giving reply to specific message ? | `send_message` can reply to a message ([#277](https://github.com/lharries/whatsapp-mcp/pull/277)); received replies show which message they quote ([#325](https://github.com/lharries/whatsapp-mcp/pull/325)). |
| <a id="issue-109"></a>[#109](https://github.com/lharries/whatsapp-mcp/issues/109) | API Breaking Changes - Missing context.Context Parameters After Dependency Update | Updated whatsmeow with the `context.Context` API changes ([#381](https://github.com/lharries/whatsapp-mcp/pull/381)). |
| <a id="issue-97"></a>[#97](https://github.com/lharries/whatsapp-mcp/issues/97) | Messages not syncing with messages.db after sent by MCP Client | Messages and reactions sent through the bridge are stored ([#383](https://github.com/lharries/whatsapp-mcp/pull/383)). |
| <a id="issue-94"></a>[#94](https://github.com/lharries/whatsapp-mcp/issues/94) | Client outdated (405) connect failure | Updated whatsmeow ([#381](https://github.com/lharries/whatsapp-mcp/pull/381), [#392](https://github.com/lharries/whatsapp-mcp/pull/392)). |
| <a id="issue-83"></a>[#83](https://github.com/lharries/whatsapp-mcp/issues/83) | Upgrade mcp package to newer version | The MCP server uses mcp 2.x ([#378](https://github.com/lharries/whatsapp-mcp/pull/378)), with a fork fix so every tool works under it. |
| <a id="issue-80"></a>[#80](https://github.com/lharries/whatsapp-mcp/issues/80) | "success": false, "message": "Error: HTTP 500 | The updated whatsmeow sends to `@lid` addresses ([#381](https://github.com/lharries/whatsapp-mcp/pull/381), [#392](https://github.com/lharries/whatsapp-mcp/pull/392)). |
| <a id="issue-66"></a>[#66](https://github.com/lharries/whatsapp-mcp/issues/66) | Group Members | Group senders are shown by name ([#375](https://github.com/lharries/whatsapp-mcp/pull/375)). |
| <a id="issue-62"></a>[#62](https://github.com/lharries/whatsapp-mcp/issues/62) | issue with group messages , Message sent false Error sending message: failed to get device list: unknown user server 'lid' | The updated whatsmeow sends to `@lid` addresses ([#381](https://github.com/lharries/whatsapp-mcp/pull/381), [#392](https://github.com/lharries/whatsapp-mcp/pull/392)). |
| <a id="issue-48"></a>[#48](https://github.com/lharries/whatsapp-mcp/issues/48) | Get name from chat GetContact | Names come from WhatsApp's contact store ([#375](https://github.com/lharries/whatsapp-mcp/pull/375)). |
| <a id="issue-47"></a>[#47](https://github.com/lharries/whatsapp-mcp/issues/47) | Get metadata in media messages | Captions are stored as the message text ([#350](https://github.com/lharries/whatsapp-mcp/pull/350)); replies show the ID of the message they quote ([#325](https://github.com/lharries/whatsapp-mcp/pull/325)). |

## Partly fixed or not checked

| Issue | Title | Status in `dev-main` |
|---|---|---|
| <a id="issue-266"></a>[#266](https://github.com/lharries/whatsapp-mcp/issues/266) | LID chats display raw JID instead of contact name | LID chats show the contact name ([#375](https://github.com/lharries/whatsapp-mcp/pull/375)). Chats that were already split are not merged. |
| <a id="issue-247"></a>[#247](https://github.com/lharries/whatsapp-mcp/issues/247) | Security scan results for whatsapp-mcp — MCPSafe AIVSS 67/100 (Grade D) | Of the 25 findings, the one real risk was prompt injection through message content: message text from others is now marked as data, and the model is told never to follow instructions in it ([fork #3](https://github.com/erwinvaneijk/whatsapp-mcp/pull/3)). Timeouts on bridge calls are fixed ([#373](https://github.com/lharries/whatsapp-mcp/pull/373)). Most other findings assume a multi-user server and do not apply to a local one. Error texts are still returned to the (local) caller. Message content and media keys are kept out of the bridge's logs ([fork #1](https://github.com/erwinvaneijk/whatsapp-mcp/pull/1), [fork #2](https://github.com/erwinvaneijk/whatsapp-mcp/pull/2)). |
| <a id="issue-246"></a>[#246](https://github.com/lharries/whatsapp-mcp/issues/246) | MCPSafe deep scan: 13 high-severity findings, score 67/100 — badge available | Same scan as #247. The main risks (open API, any file path, data permissions) are addressed ([#387](https://github.com/lharries/whatsapp-mcp/pull/387), [#389](https://github.com/lharries/whatsapp-mcp/pull/389), [#388](https://github.com/lharries/whatsapp-mcp/pull/388)). Against prompt injection, message text from others is marked as data and the model is told never to follow instructions in it ([fork #3](https://github.com/erwinvaneijk/whatsapp-mcp/pull/3)). Message content and media keys are kept out of the bridge's logs ([fork #1](https://github.com/erwinvaneijk/whatsapp-mcp/pull/1), [fork #2](https://github.com/erwinvaneijk/whatsapp-mcp/pull/2)). |
| <a id="issue-218"></a>[#218](https://github.com/lharries/whatsapp-mcp/issues/218) | Add policy enforcement for message sending | Optional read-only mode hides the send tools ([#388](https://github.com/lharries/whatsapp-mcp/pull/388)), send tools are marked as destructive so clients ask first ([#142](https://github.com/lharries/whatsapp-mcp/pull/142)), and the model is told to send only when the user asks, never because a message asks for it ([fork #3](https://github.com/erwinvaneijk/whatsapp-mcp/pull/3)). There is no per-recipient allowlist. |
| <a id="issue-198"></a>[#198](https://github.com/lharries/whatsapp-mcp/issues/198) | Contact identity confusion: whatsmeow's LID mappings and contact store are never synced to messages.db | Names now resolve for `@lid` contacts ([#375](https://github.com/lharries/whatsapp-mcp/pull/375)), but one person can still appear as two chats (once under the phone number, once under the LID). |
| <a id="issue-146"></a>[#146](https://github.com/lharries/whatsapp-mcp/issues/146) | Timestamp timezone ambiguity - all messages/chats lack timezone context | Timestamps are stored with their time zone, but `list_messages` shows them without one, and the `after`/`before` filters ignore zones. |
| <a id="issue-144"></a>[#144](https://github.com/lharries/whatsapp-mcp/issues/144) | feat: add sender_name field to MCP list_messages and list_chats output | `list_messages` shows sender names ([#375](https://github.com/lharries/whatsapp-mcp/pull/375)); `list_chats` still returns `last_sender` as a number. |
| <a id="issue-140"></a>[#140](https://github.com/lharries/whatsapp-mcp/issues/140) | Adding a url for QR-code pairing | Alternative: link with a pairing code instead of a QR code, with `WHATSAPP_PAIR_PHONE` ([#273](https://github.com/lharries/whatsapp-mcp/pull/273)). |
| <a id="issue-129"></a>[#129](https://github.com/lharries/whatsapp-mcp/issues/129) | Suspected security issue — please provide security contact (whatsapp-mcp) | Details were never published. The bridge is hardened ([#387](https://github.com/lharries/whatsapp-mcp/pull/387), [#389](https://github.com/lharries/whatsapp-mcp/pull/389)), but this report cannot be checked. Message content and media keys are kept out of the bridge's logs ([fork #1](https://github.com/erwinvaneijk/whatsapp-mcp/pull/1), [fork #2](https://github.com/erwinvaneijk/whatsapp-mcp/pull/2)). |
| <a id="issue-57"></a>[#57](https://github.com/lharries/whatsapp-mcp/issues/57) | Potential Security Issue | Details were never published. The bridge is hardened ([#387](https://github.com/lharries/whatsapp-mcp/pull/387), [#389](https://github.com/lharries/whatsapp-mcp/pull/389)), but this report cannot be checked. Message content and media keys are kept out of the bridge's logs ([fork #1](https://github.com/erwinvaneijk/whatsapp-mcp/pull/1), [fork #2](https://github.com/erwinvaneijk/whatsapp-mcp/pull/2)). |
| <a id="issue-43"></a>[#43](https://github.com/lharries/whatsapp-mcp/issues/43) | send_file is not working | By design the bridge only sends files from `WHATSAPP_MEDIA_DIR`; a file uploaded to Claude has to be saved there first. Documented in the README. |
| <a id="issue-18"></a>[#18](https://github.com/lharries/whatsapp-mcp/issues/18) | Unable to filter messages by chat_jid in Windows + Cursor | Not checked. The tool schemas changed with mcp 2.x; this may or may not still happen in Cursor. |
| <a id="issue-9"></a>[#9](https://github.com/lharries/whatsapp-mcp/issues/9) | Claude cannot find the contact by their name | Search finds contacts by name for everyone you have a chat with ([#375](https://github.com/lharries/whatsapp-mcp/pull/375)). Contacts you never messaged are not found: the search does not cover the whole address book. |

## Not fixed yet

| Issue | Title | Status in `dev-main` |
|---|---|---|
| <a id="issue-297"></a>[#297](https://github.com/lharries/whatsapp-mcp/issues/297) | Opencode | Not tested; the MCP server runs locally over stdio, which OpenCode may support. |
| <a id="issue-290"></a>[#290](https://github.com/lharries/whatsapp-mcp/issues/290) | feat: Auto-download & transcribe voice/audio messages | Not available. |
| <a id="issue-289"></a>[#289](https://github.com/lharries/whatsapp-mcp/issues/289) | feat: Auto-download & transcribe voice/audio messages | Not available. |
| <a id="issue-288"></a>[#288](https://github.com/lharries/whatsapp-mcp/issues/288) | perf: every list_* / get_* opens a fresh sqlite3 connection; cache per thread | Each tool call still opens a new database connection. |
| <a id="issue-287"></a>[#287](https://github.com/lharries/whatsapp-mcp/issues/287) | perf: whatsapp.py opens a fresh HTTP connection per bridge call; use a module-level requests.Session | Each bridge call still opens a new HTTP connection. |
| <a id="issue-286"></a>[#286](https://github.com/lharries/whatsapp-mcp/issues/286) | perf: format_messages_list calls get_sender_name once per message (N+1 SQL + N+1 sqlite3.connect) | `list_messages` still looks up each sender's name separately. |
| <a id="issue-123"></a>[#123](https://github.com/lharries/whatsapp-mcp/issues/123) | Cannot use the MCP as connector on ChatGPT | The MCP server runs locally over stdio only. |
| <a id="issue-90"></a>[#90](https://github.com/lharries/whatsapp-mcp/issues/90) | Message Delete and leave chat Support | Not available. |
| <a id="issue-88"></a>[#88](https://github.com/lharries/whatsapp-mcp/issues/88) | Running Whatsapp bridge in the background | Not documented yet; the bridge still has to be started by hand or as your own service. |
| <a id="issue-87"></a>[#87](https://github.com/lharries/whatsapp-mcp/issues/87) | Add manifest.json for DXT | No Claude Desktop extension package yet. |
| <a id="issue-85"></a>[#85](https://github.com/lharries/whatsapp-mcp/issues/85) | Manually have to run the bridge in order for the MCP to work? | Not documented yet; the bridge still has to be started by hand or as your own service. |
| <a id="issue-84"></a>[#84](https://github.com/lharries/whatsapp-mcp/issues/84) | Transcribing Audio | Not available. |
| <a id="issue-78"></a>[#78](https://github.com/lharries/whatsapp-mcp/issues/78) | sending blank images | Likely cause: the bridge sends images without a preview thumbnail. |
| <a id="issue-73"></a>[#73](https://github.com/lharries/whatsapp-mcp/issues/73) | [Feature] - Ability to Delete Single or Multiple Chats (Including Saved and Unsaved Contacts) | Not available. |
| <a id="issue-69"></a>[#69](https://github.com/lharries/whatsapp-mcp/issues/69) | Could you please integrate the Evenlabs API with this server? | Out of scope for this project. |
| <a id="issue-64"></a>[#64](https://github.com/lharries/whatsapp-mcp/issues/64) | Simulating Real User Behavior | Not available. |
| <a id="issue-53"></a>[#53](https://github.com/lharries/whatsapp-mcp/issues/53) | Update README with Ubuntu 24 Installation Instructions | Not added yet. |
| <a id="issue-52"></a>[#52](https://github.com/lharries/whatsapp-mcp/issues/52) | Is there a way to connect this MCP server to openWeb UI | The MCP server runs locally over stdio only. |
| <a id="issue-51"></a>[#51](https://github.com/lharries/whatsapp-mcp/issues/51) | Deploy MCP server on smithery.ai | Not planned. |
| <a id="issue-45"></a>[#45](https://github.com/lharries/whatsapp-mcp/issues/45) | Invalid type for parameter 'chat_jid' when handling interactive/button messages | Button and list messages from business accounts are not handled yet. |
| <a id="issue-41"></a>[#41](https://github.com/lharries/whatsapp-mcp/issues/41) | list_messages format broken | `list_messages` still returns formatted text, not JSON. |
| <a id="issue-37"></a>[#37](https://github.com/lharries/whatsapp-mcp/issues/37) | search_contacts - parameterize LIMIT | The limit is still fixed at 50. |

## Setup and support questions

These are about installation or configuration rather than a bug in the code.

| Issue | Title | Status in `dev-main` |
|---|---|---|
| <a id="issue-86"></a>[#86](https://github.com/lharries/whatsapp-mcp/issues/86) | how to fix this | The phone unlinked the device; link the bridge again. |
| <a id="issue-70"></a>[#70](https://github.com/lharries/whatsapp-mcp/issues/70) | Cannot run the bridge on Windows | C compiler setup on Windows; see the README. |
| <a id="issue-65"></a>[#65](https://github.com/lharries/whatsapp-mcp/issues/65) | Whatsapp-MCP-Server is not connecting with Claude | Configuration file format. |
| <a id="issue-63"></a>[#63](https://github.com/lharries/whatsapp-mcp/issues/63) | Failed to connect | Network connection to WhatsApp. |
| <a id="issue-58"></a>[#58](https://github.com/lharries/whatsapp-mcp/issues/58) | Message sent false Error sending message: failed to get device list: failed to send usync query: info query timed out | Caused by an empty recipient, according to the thread. |
| <a id="issue-46"></a>[#46](https://github.com/lharries/whatsapp-mcp/issues/46) | Client Closed not able to connect to the server | Client configuration. |
| <a id="issue-44"></a>[#44](https://github.com/lharries/whatsapp-mcp/issues/44) | Failed to create client | Configuration file format. |
| <a id="issue-40"></a>[#40](https://github.com/lharries/whatsapp-mcp/issues/40) | Unable to send message due to HTTP error 404. | The bridge was not running or was an older version. |
| <a id="issue-39"></a>[#39](https://github.com/lharries/whatsapp-mcp/issues/39) | new column "media_type" in db | Database from an early version; start with a fresh `store/` folder. |
| <a id="issue-38"></a>[#38](https://github.com/lharries/whatsapp-mcp/issues/38) | Failed to connect to database: failed to upgrade database: Binary was compiled with 'CGO_ENABLED=0', go-sqlite3 | Needs CGO; see the Windows section of the README. |
| <a id="issue-36"></a>[#36](https://github.com/lharries/whatsapp-mcp/issues/36) | [whatsapp] [info] Client transport closed | Client configuration. |
| <a id="issue-35"></a>[#35](https://github.com/lharries/whatsapp-mcp/issues/35) | Configuration file for Windows and restarting of machine | Windows configuration; see the README. |
| <a id="issue-23"></a>[#23](https://github.com/lharries/whatsapp-mcp/issues/23) | unable to send messages | The bridge was not running or was an older version. |
| <a id="issue-21"></a>[#21](https://github.com/lharries/whatsapp-mcp/issues/21) | Issue with searching and getting messages using Cursor | Client configuration. |
| <a id="issue-20"></a>[#20](https://github.com/lharries/whatsapp-mcp/issues/20) | Server disconnected | Client or account limits, not the server. |
| <a id="issue-19"></a>[#19](https://github.com/lharries/whatsapp-mcp/issues/19) | Not Install the dependency when I run uv pip install e . | Install with uv as in the README. |

## Not about the code

Directory listings, empty issues, questions about maintenance, and reports about other products.

| Issue | Title | Status in `dev-main` |
|---|---|---|
| <a id="issue-377"></a>[#377](https://github.com/lharries/whatsapp-mcp/issues/377) | Added 1-click config generator and setup guide for whatsapp-mcp on MCP ServerList | Directory listing. |
| <a id="issue-374"></a>[#374](https://github.com/lharries/whatsapp-mcp/issues/374) | video | No content. |
| <a id="issue-310"></a>[#310](https://github.com/lharries/whatsapp-mcp/issues/310) | Approved native WhatsApp event does not appear and may degrade to mismatched plain text | About another product. |
| <a id="issue-309"></a>[#309](https://github.com/lharries/whatsapp-mcp/issues/309) | Your MCP server is listed on DReview | Directory listing. |
| <a id="issue-285"></a>[#285](https://github.com/lharries/whatsapp-mcp/issues/285) | lebanon | No content. |
| <a id="issue-263"></a>[#263](https://github.com/lharries/whatsapp-mcp/issues/263) | Is this repo dead? | Question about maintenance: this fork is maintained. |
| <a id="issue-238"></a>[#238](https://github.com/lharries/whatsapp-mcp/issues/238) | Bot messages not appearing in real-time in Audit Thread panel | About another product. |
| <a id="issue-220"></a>[#220](https://github.com/lharries/whatsapp-mcp/issues/220) | Request: Add maintainers or transfer repo — 20+ PRs waiting, community eager to help | Question about maintenance: this fork is maintained. |
| <a id="issue-214"></a>[#214](https://github.com/lharries/whatsapp-mcp/issues/214) | Your MCP server is now indexed on Joy Trust Network | Directory listing. |
| <a id="issue-204"></a>[#204](https://github.com/lharries/whatsapp-mcp/issues/204) | List WhatsApp MCP on MCPHosting marketplace — 80% creator revenue share | Marketplace offer. |
| <a id="issue-187"></a>[#187](https://github.com/lharries/whatsapp-mcp/issues/187) | Community Extension: Added Polls, Reactions, and Events support 🚀 | Announcement of a companion server. Reactions are now built in ([#148](https://github.com/lharries/whatsapp-mcp/pull/148)). |
| <a id="issue-151"></a>[#151](https://github.com/lharries/whatsapp-mcp/issues/151) | Is this now abandonware or it's still worth giving it a shot? | Question about maintenance: this fork is maintained. |
| <a id="issue-126"></a>[#126](https://github.com/lharries/whatsapp-mcp/issues/126) | This project seems dead and broken (according to comments). Which one is the most active fork? | Question about maintenance: this fork is maintained. |
| <a id="issue-108"></a>[#108](https://github.com/lharries/whatsapp-mcp/issues/108) | واتساب | No content. |

## Fixes made in this fork

Changes that are not upstream pull requests:

- **MCP tools work with mcp 2.x.** After the move to mcp 2.x ([#378](https://github.com/lharries/whatsapp-mcp/pull/378)), every read tool failed with "Error executing tool", because the tools declared different return types than they returned. Fixed, with a test that calls every tool through the MCP server.
- **Full-text search does not break message storage.** With [#281](https://github.com/lharries/whatsapp-mcp/pull/281) as submitted, a bridge built once with full-text search and later without it stopped storing messages. Fixed, with tests for both builds.
- **Message content stays out of logs.** By default the bridge no longer logs message text, senders, captions, file names, reaction emoji or the status text of sends, and it never logs media decryption keys. Set `WHATSAPP_LOG_CONTENT=1` to log content while debugging. Contributed by BadPirate in [fork #1](https://github.com/erwinvaneijk/whatsapp-mcp/pull/1), completed in [fork #2](https://github.com/erwinvaneijk/whatsapp-mcp/pull/2).
- **Messages from others are marked as data.** Message text is wrapped in `<message_content>` tags (a message cannot break out of them), names are kept on one line, and every read tool and the server's instructions tell the model never to follow instructions in this content. This limits prompt injection by anyone who can message you ([fork #3](https://github.com/erwinvaneijk/whatsapp-mcp/pull/3), prompted by [#247](https://github.com/lharries/whatsapp-mcp/issues/247)).
- **Fixes in adapted upstream PRs.** Several upstream PRs were corrected while merging, for example reactions and media retry in group chats and with LID contacts ([#148](https://github.com/lharries/whatsapp-mcp/pull/148), [#273](https://github.com/lharries/whatsapp-mcp/pull/273)), and the pairing code request ([#273](https://github.com/lharries/whatsapp-mcp/pull/273)). The merge commits on `dev-main` describe each change.
