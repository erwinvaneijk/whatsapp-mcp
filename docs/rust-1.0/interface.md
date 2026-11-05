# The interface to keep

What the 1.0 rewrite must keep, as of `dev-main` on 2026-11-05. Where a test or a generated file is the exact reference, this page points to it instead of repeating it. Part of the [plan](plan.md); progress is in the [score sheet](scoresheet.md).

## 1. MCP server

The exact reference is the [parity harness](../../parity/README.md):

- **Server:** [`parity/golden/default/_server.json`](../../parity/golden/default/_server.json) has the server name (`whatsapp`), the instructions, and every tool with its description, input schema, output schema and annotations.
- **Results:** the other golden files have the result of each case.

| Tool | Title | Kind | Hidden in read-only mode |
|---|---|---|---|
| `search_contacts` | Search Contacts | read | no |
| `list_messages` | List Messages | read | no |
| `list_chats` | List Chats | read | no |
| `get_chat` | Get Chat | read | no |
| `get_direct_chat_by_contact` | Get Direct Chat by Contact | read | no |
| `get_contact_chats` | Get Contact Chats | read | no |
| `get_last_interaction` | Get Last Interaction | read | no |
| `get_message_context` | Get Message Context | read | no |
| `download_media` | Download Media | asks the bridge, writes a file | no |
| `send_message` | Send Message | send | yes |
| `send_file` | Send File | send | yes |
| `send_audio_message` | Send Audio Message | send | yes |
| `react_to_message` | React to Message | send | yes |

The parts that are easy to miss:

- **Annotations:** read tools are marked read-only and closed-world. Send tools are marked destructive, not idempotent and open-world. `download_media` is not read-only, not destructive, idempotent and open-world.
- **Descriptions:** every read tool's description ends with the untrusted-content notice.
- **Untrusted content:** message text from others is wrapped in `<message_content>…</message_content>`, and a `<message_content` or `</message_content` inside it has its `<` written as `&lt;`. Chat and contact names are put on one line.
- **Times:** shown with their UTC offset (`2026-11-04 22:00:52 +01:00`). `after` and `before` take ISO-8601 dates or date-times. A time without a zone is local to the machine running the server.
- **Sender names:**
  1. the saved contact name from whatsmeow;
  2. otherwise the stored chat name;
  3. otherwise the push name or business name.

  Each lookup also goes through the LID-to-phone map, both ways.

## 2. Configuration

Shared by the bridge and the MCP server. The exact rules are in `whatsapp-bridge/config.go` and `whatsapp-mcp-server/config.py`, and their tests. The [README](../../README.md#configuration) and [`config.example.yaml`](../../config.example.yaml) describe them for users.

- **File:** `~/.config/whatsapp-mcp/config.yaml`, or the path in `WHATSAPP_MCP_CONFIG`. A file named that way must exist; the default file is optional.
- **Precedence:** an environment variable that is set wins over the file. An empty one counts as unset.
- **Values:** keep their exact text, so `0612` stays `0612`. `send_allowlist` may also be a YAML list; its entries are joined with `, `.
- **Errors:** an unknown setting, or a value of the wrong kind, is an error that gives the line number. Both programs stop.
- **Permissions:** a file that others can read or change is refused, except on Windows.

| Setting | Environment variable | Used by |
|---|---|---|
| `bridge_token` | `WHATSAPP_BRIDGE_TOKEN` | both; the bridge refuses to start without one of at least 32 characters |
| `media_dir` | `WHATSAPP_MEDIA_DIR` | both; files are only sent from here |
| `send_allowlist` | `WHATSAPP_SEND_ALLOWLIST` | bridge; a bad entry stops it |
| `read_only` | `WHATSAPP_MCP_READ_ONLY` | MCP server |
| `log_content` | `WHATSAPP_LOG_CONTENT` | bridge |
| `pair_phone` | `WHATSAPP_PAIR_PHONE` | bridge |

Yes/no settings accept `1`, `true`, `yes` and `on`, in any case; anything else means no.

## 3. Bridge API

Internal, but kept for 1.0, so the Rust MCP server can run with the Go bridge (release 0.9).

The bridge listens on `127.0.0.1:8080` only. Every request must have:

- the `X-Bridge-Token` header, equal to the token. It is compared in constant time. Otherwise the answer is `401 Unauthorized`.
- `Content-Type: application/json`. Otherwise the answer is `415`.
- a body of at most 1 MiB.

Requests other than POST get `405`. Plain-text errors end with a newline. JSON answers are compact and end with a newline.

| Endpoint | Request (JSON) | Answer |
|---|---|---|
| `POST /api/send` | `recipient` (phone number or JID), `message`, optional `media_path`, `reply_to_message_id`, `reply_to_sender_jid`, `mentions` (list) | `200 {"success":true,"message":…}`; `500` with `success: false` when sending failed |
| `POST /api/react` | `message_id`, `chat_jid`, `emoji` (empty removes the reaction) | `200 {"success":true,"message":"Reaction ❤️ sent"}` or `"Reaction removed"`; `500` with `success: false` |
| `POST /api/download` | `message_id`, `chat_jid` | `200 {"success":true,"message":"Successfully downloaded image media","filename":…,"path":…}`; `500 {"success":false,"message":"Failed to download media: …"}` |

Plain-text errors with `400`:

| Endpoint | Message |
|---|---|
| any | `Invalid request format` |
| `/api/send` | `Recipient is required`, `Message or media path is required`, `Invalid recipient` |
| `/api/react` | `message_id and chat_jid are required`, `Invalid chat_jid` |
| `/api/download` | `Message ID and Chat JID are required` |

A chat outside the send allowlist gets `403 {"success":false,"message":"This chat is not in WHATSAPP_SEND_ALLOWLIST, so the bridge will not send to it."}`.

`media_path` must resolve, symlinks included, to a file inside `media_dir`.

## 4. Files

All in `whatsapp-bridge/store/`. The folder and its files are readable by the user only: `0700` for folders, `0600` for files.

| File | Contents | Kept in 1.0 |
|---|---|---|
| `messages.db` | Chats and messages. Exact schema: [`parity/fixture/schema.sql`](../../parity/fixture/schema.sql), checked against the bridge in CI. Times are stored as text with their UTC offset. Opened in WAL mode. | Yes, used as is. |
| `whatsapp.db` | whatsmeow's session and contact store. The MCP server reads `whatsmeow_contacts` and `whatsmeow_lid_map` from it for names ([`parity/fixture/whatsmeow.sql`](../../parity/fixture/whatsmeow.sql)). | No: whatsapp-rust has its own store. The Rust bridge copies names and the LID map into `messages.db` (see the plan). |
| `<chat>/<file>` | Downloaded media. The chat folder is the chat JID with every character other than letters, digits, `@ . - _` replaced by `_`. When two messages in a chat share a file name, the file gets `_<message id>` before its extension. | Yes. |

## 5. Behaviour that improves in 1.0

The harness records today's behaviour, including the quirks below. **Decided on 2026-11-05:** the Rust server does not copy them. It behaves as in the third column, and the score sheet marks these cases `changed`.

| What | Today | In the Rust server | Golden files |
|---|---|---|---|
| Errors raised by a tool (a bad date, a negative limit) | The client gets only `Error executing tool list_messages`. mcp 2.x hides the exception text, so the model doesn't learn what was wrong. | An error result with the reason, for example `Invalid date format for 'after': 21:00. Use an ISO-8601 date, or date and time, …`. | `default/list_messages-bad-*.json`, `default/list_messages-negative-limit.json`, `default/list_chats-negative-page.json` |
| Bridge answers with an error status | The message is `Error: HTTP 500 - ` followed by the bridge's raw JSON. | The bridge's own `message` when the answer is JSON, such as `This chat is not in WHATSAPP_SEND_ALLOWLIST, …`. Otherwise `Bridge error (HTTP 400): ` followed by its text. | `default/send_message-refused.json`, `default/send_message-bridge-failure.json` |
| Bridge answers 200 with text that isn't JSON | `Request error: Expecting value: line 1 column 1 (char 0)`, Python's parser error. | A message that says the bridge's answer could not be read, without parser details. | `default/send_message-bridge-not-json.json` |
| `download_media` failures | Always `Failed to download media`; the bridge's reason is only written to the server's log. | The bridge's reason is included, for example `Failed to download media: message has no media`. | `default/download_media-failed.json`, `default/download_media-no-id.json` |

**Harness:** when the Rust server is built, these golden files get a second, Rust-specific expected result. All other golden files stay shared, and both servers must match them.
