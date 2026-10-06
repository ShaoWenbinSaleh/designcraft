# AI 助手 / AI assistant

Open **Window → AI 助手** in the desktop app. The assistant docks on the right by default;
**浮动** opens a movable window and **停靠右侧** returns it to the dock. Closing it preserves
records and running work. Use **停止** to cancel a task. Position, mode and selected tab are
stored with the other UI preferences.

## Workflow

1. Open **设置**, select a compatible API or **ChatGPT 登录**.
2. For an API, enter its base URL (including `/v1` where required), key if required, and model.
   Save settings before starting a conversation. An empty key field preserves the saved key.
3. For ChatGPT, choose **Continue with ChatGPT**, complete browser consent, load the account's
   models, choose a model and save. **添加账户** creates a separate registration; the account
   picker selects existing registrations. **退出登录** removes local credentials and attempts
   server revocation. **管理用量** opens ChatGPT's usage page.
4. Select ordinary body text using the Type tool and press **修复标点和错别字**, or send a chat.
5. In **修改建议**, inspect the original/replacement and reason. **定位原文** selects the current
   range and page; overset text opens in Story Editor. Accept individual suggestions or an
   entire non-overlapping batch. Acceptance is one undo step per action.
6. Save the document to retain chat, suggestions, statuses and source snapshots. Export an
   offline HTML review report before sharing when a readable report is preferable.

New conversation clears messages, not suggestions. **清除当前文档 AI 记录** clears both and is
undoable. Changing provider settings does not send a request automatically. Model requests use
saved settings. API keys and OAuth tokens use the OS credential store, never document JSON,
UI inspection or command journals. Model/catalog requests use the endpoint selected in settings;
ChatGPT OAuth tokens can only go to the fixed official OpenAI API endpoint.

Reports go beside a saved native document by default; unsaved documents ask for a path.
Existing files receive a numeric suffix instead of being overwritten. The report contains all
retained suggestions, their status, original context and the page recorded at proposal time.
It contains no scripts or remote resources. Page labels can be unknown for overset text.

## Editing contract

- The built-in model has only `get_text_frames`, `read_text`, and `propose_text_change`. It
  cannot accept changes or execute arbitrary engine/UI commands. Document text is source data,
  not instructions.
- `read_text` returns a snapshot ID and a bounded page of text. Models quote a unique exact
  original span; the host computes UTF-8 boundaries. Model-supplied offsets are discarded.
  Repeated text requires unchanged context in both original and replacement. Deterministic
  engine clients may still supply Unicode scalar offsets; mismatches resolve only when the
  original quote is unique, never using fuzzy or nearest-match replacement.
- The proofreading shortcut captures the current selection and splits it at paragraph boundaries,
  with a 16,000-character maximum and safe splitting of longer paragraphs. Each request sees only
  its captured batch and can propose only against captured selection snapshots.
- Original text must match exactly. Changes to the source story invalidate pending suggestions.
  Accepted edits rebase unaffected suggestions; overlapping alternatives become stale.
- Acceptance computes a Unicode diff and changes only the differing spans. Unchanged text
  keeps its character style and overrides; replacement characters inherit their corresponding
  original character formats, and added characters inherit the local format. Paragraph formats
  and text-frame properties remain untouched. Layout controls (paragraph breaks, tabs, forced
  breaks, nonbreaking spaces and discretionary breaks) cannot be added or removed. Text length
  changes may naturally reflow under the existing layout rules. Large ambiguous rewrites that
  exceed the bounded diff budget are rejected instead of flattening formatting. Table cells, footnote text, and replacements crossing
  special object markers are not supported in this first version.
- Batch acceptance validates every edit and overlap before committing a copy of the document.
  No partial document changes survive a validation failure. Undo restores acceptance state.
- Switching/closing documents, changing body text or pressing Stop invalidates the task generation;
  queued and late network results cannot execute tools. Completed proposals remain reviewable.
  An in-flight network read may take until its timeout to release resources after cancellation.
- Imported IDML starts without AI records. IDML/PDF/EPUB/HTML publication exports do not carry the
  assistant archive. Copied object snippets explicitly omit the source document's AI records.

## Native format and compatibility

`Document.ai` is optional, with AI schema version 1. It stores messages, task status, snapshots
and suggestions with original provenance. Native ZIP documents containing this field declare
format version **2**, so older DesignCraft builds reject rather than silently discard it.
Documents without AI data continue to write version 1. Both old ZIP documents and legacy JSON
remain readable. A stored running task is marked interrupted when reopened; it never restarts
or sends document contents automatically. Credentials and provider account registrations are
machine-local and are not copied by Save As.

The native-only `designcraft-ai` transport is cfg-excluded on WASM. The web build can retain
AI document records and use engine review commands; network/login UI is unavailable there.
Linux builds need D-Bus development headers (`libdbus-1-dev`) and a running Secret Service for
persistent credentials. macOS uses Keychain; Windows uses Credential Manager. No Node helper
or plugin installer is required. Chinese UI text uses an installed system CJK font at runtime;
no operating-system fonts are redistributed.

## Control channel / MCP

Use `engine.execute` with the following commands (desktop UI commands are also exposed by the
remote MCP backend):

| Command | Parameters / result |
|---|---|
| `window.ai` | `{open?: true}` |
| `window.ai.layout` | `{floating?, position?: [x,y], tab?: "chat"\|"suggestions"\|"settings"}` |
| `ai.chat.send` | `{text}` → asynchronous task ID |
| `ai.proofreadSelection` | `{}` → asynchronous task ID |
| `ai.task.inspect` | `{}` → busy state, task ID, notice, model list; never credentials |
| `ai.task.stop` | `{}` |
| `ai.settings.get/save` | Get saved public settings; save `{provider?, endpoint?, model?, account?, apiKey?}` |
| `ai.auth.login/logout/usage` | Login optionally takes `{newAccount: true}` |
| `ai.models`, `ai.connection.test` | Asynchronous catalog / completion-plus-tool test |
| `ai.text.frames` | List story IDs, frames, character counts |
| `ai.text.read` | `{story?, offset?, length?}`; no story means selected text |
| `ai.suggestion.propose` | `{snapshot, original, replacement, reason, offset?}` |
| `ai.suggestion.list` | Revalidated persisted review state |
| `ai.suggestion.accept/reject/locate` | `{id}`; locate is engine-only selection |
| `ai.suggestion.show` | `{id}`; additionally navigates the UI |
| `ai.suggestion.acceptAll` | `{}` |
| `ai.chat.clear`, `ai.records.clear` | `{}` |
| `ai.report.html/export` | HTML returns a string; desktop export takes `{path?}` |

Do not log control requests containing `apiKey`. The application does not journal this UI
command. Chat/document content intentionally persists in the native file and exported report.

## ChatGPT protocol references and verification

- https://developers.openai.com/siwc/token-sharing-open-source/sign-in
- https://developers.openai.com/siwc/token-sharing-open-source/profiles-and-sessions
- https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference

The client uses a loopback callback, a persistent host identifier, per-attempt state/nonce/PKCE,
the issued client ID, JWKS signature/issuer/audience/expiry validation and explicit
`chatgpt.tokens.use.direct` permission. Responses use `store:false`, `stream:true`; tools are
processed only after `response.completed`. Identity-only login cannot perform inference.

Automated tests cover Unicode mapping, batching, minimal formatting changes, stale/overlapping
edits, atomic batch undo, native save/load, canceled/late tasks, scope enforcement, window-state
persistence and complete/failed SSE streams. Actual browser consent, account eligibility,
refresh/revocation and live inference require an authorized account; offline tests do not
establish that a particular account has subscription access.

Implementation verification (2026-10-06): all six `xtask ci` gates passed, with
523 tests passing and two existing tests ignored. The Homebrew Rust installation
lacked a compatible prebuilt WASM standard library, so the WASM checks used
`-Z build-std=std,panic_abort` with its matching installed Rust sources. The desktop
binary also built successfully. Rendered UI screenshots verified docked, floating,
settings and reopened states; a synthetic review exercised accept-all, undo,
native save/reopen and HTML export. Live authorization and inference remain
account-dependent verification steps.

Selection-proofreading regression fix: the worker receives a host-captured snapshot and
only the proposal tool, avoiding the conflicting requirement to call a forbidden read
tool. Both transport protocols are covered with deterministic model responses. Tool
errors remain failures, and the window distinguishes newly generated proposals from a
reply with no proposals. Existing rejected runs can be retried on the selected text.
The regression run passed all six CI gates (526 tests passed, two existing tests
ignored), rebuilt the release executable, and re-rendered the review/save/reopen
fixture. No live provider request or account credential was used for this regression.

## Live exact-quote regression (2026-10-07)

Run `cargo run -p designcraft-ui-egui --example ai_live_check -- --live /tmp/ai-live`
to use the saved provider and OS credentials with synthetic text only. This is opt-in
and is never run by CI. It checks Chinese plus emoji, repeated misspellings, a
nonzero selection start, long/overset text, unchanged text outside the selection,
acceptance, one-step undo and native save/reopen. Test artifacts contain synthetic
source, model proposals and chat; credentials are never printed or persisted there.

The live run passed for both short and long input (one and two proposals respectively).
The release app also displayed these real proposals, located the overset story and
accepted both proposals; undo was exercised again through its control channel.
Final verification: all six CI gates passed (527 tests passed, two existing tests
ignored), including WASM checks with the matching-source standard-library build
described above. The release executable was rebuilt after the fix.

The live regression also applies mixed character formatting before proofreading and checks
that the original character runs, paragraph formatting and frames survive acceptance.

Content-only mode verification: 531 tests passed, with all six CI gates passing.
Live short/long proofreading preserved existing mixed character runs, paragraph
formats and frames through acceptance, undo and save/reopen. The updated window
and before/after document rendering were inspected; the release binary was rebuilt.
