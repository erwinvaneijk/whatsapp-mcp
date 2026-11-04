package main

import (
	"context"
	"crypto/subtle"
	"database/sql"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"math"
	"math/rand"
	"mime"
	"net/http"
	"net/url"
	"os"
	"os/signal"
	"path/filepath"
	"reflect"
	"strings"
	"syscall"
	"time"
	"unicode"

	_ "github.com/mattn/go-sqlite3"
	"github.com/mdp/qrterminal"

	"bytes"

	"go.mau.fi/whatsmeow"
	waProto "go.mau.fi/whatsmeow/binary/proto"
	"go.mau.fi/whatsmeow/store/sqlstore"
	"go.mau.fi/whatsmeow/types"
	"go.mau.fi/whatsmeow/types/events"
	waLog "go.mau.fi/whatsmeow/util/log"
	"google.golang.org/protobuf/proto"
	"google.golang.org/protobuf/reflect/protoreflect"
)

// These settings are read in main, after the configuration file (config.go)
// has been applied to the environment.

// bridgeToken is the shared secret every REST request must present in X-Bridge-Token.
var bridgeToken string

// mediaDir is the only directory media may be sent from. Empty disables sending media.
var mediaDir string

// logContent controls whether message text, captions and file names are written
// to the logs. Off by default so chat content does not pile up in plain text;
// set WHATSAPP_LOG_CONTENT=1 (or true/yes/on) to turn it on for debugging.
var logContent bool

// logResult logs the outcome of a send. The status text can contain content
// (a reaction's emoji, a file path in an error), so it is only logged with
// WHATSAPP_LOG_CONTENT; the caller always gets the full text in the response.
func logResult(what string, success bool, status string) {
	fmt.Println(resultLogLine(what, success, status, logContent))
}

func resultLogLine(what string, success bool, status string, withContent bool) string {
	if withContent {
		return fmt.Sprintf("%s %v %s", what, success, cleanForLog(status))
	}
	if success {
		return what + " true"
	}
	return what + " false (set WHATSAPP_LOG_CONTENT=1 to log the error)"
}

// envFlag reports whether an environment value means "on" (1, true, yes, on).
func envFlag(value string) bool {
	switch strings.ToLower(strings.TrimSpace(value)) {
	case "1", "true", "yes", "on":
		return true
	}
	return false
}

// cleanForLog strips control characters so message text cannot inject terminal escape codes.
func cleanForLog(s string) string {
	return strings.Map(func(r rune) rune {
		if unicode.IsControl(r) {
			return -1
		}
		return r
	}, s)
}

// safeName reduces a sender-supplied filename to a plain base name.
func safeName(name string) string {
	name = cleanForLog(filepath.Base(strings.ReplaceAll(name, "\\", "/")))
	if name == "" || name == "." || name == ".." || name == "/" {
		return "file"
	}
	return name
}

// safeChatDir turns a chat JID into a single safe directory name.
func safeChatDir(jid string) string {
	dir := strings.Map(func(r rune) rune {
		switch {
		case r >= 'a' && r <= 'z', r >= 'A' && r <= 'Z', r >= '0' && r <= '9', r == '@', r == '.', r == '-', r == '_':
			return r
		}
		return '_'
	}, jid)
	if dir == "" || dir == "." || dir == ".." {
		return "_"
	}
	return dir
}

// allowedMediaPath resolves p and returns it only if it lies inside mediaDir.
func allowedMediaPath(p string) (string, error) {
	if mediaDir == "" {
		return "", fmt.Errorf("media sending is disabled: WHATSAPP_MEDIA_DIR is not set")
	}
	root, err := filepath.EvalSymlinks(mediaDir)
	if err != nil {
		return "", fmt.Errorf("cannot resolve WHATSAPP_MEDIA_DIR: %v", err)
	}
	real, err := filepath.EvalSymlinks(p)
	if err != nil {
		return "", fmt.Errorf("cannot resolve media path: %v", err)
	}
	rel, err := filepath.Rel(root, real)
	if err != nil || rel == ".." || strings.HasPrefix(rel, ".."+string(filepath.Separator)) {
		return "", fmt.Errorf("media path is outside WHATSAPP_MEDIA_DIR")
	}
	return real, nil
}

// requireToken rejects requests without the shared secret or without a JSON content type.
// The custom header also forces a CORS preflight, which browsers fail against this server.
func requireToken(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if subtle.ConstantTimeCompare([]byte(r.Header.Get("X-Bridge-Token")), []byte(bridgeToken)) != 1 {
			http.Error(w, "Unauthorized", http.StatusUnauthorized)
			return
		}
		if !strings.HasPrefix(r.Header.Get("Content-Type"), "application/json") {
			http.Error(w, "Content-Type must be application/json", http.StatusUnsupportedMediaType)
			return
		}
		r.Body = http.MaxBytesReader(w, r.Body, 1<<20)
		next.ServeHTTP(w, r)
	})
}

// Message represents a chat message for our client
type Message struct {
	Time      time.Time
	Sender    string
	Content   string
	IsFromMe  bool
	MediaType string
	Filename  string
}

// Database handler for storing message history
type MessageStore struct {
	db *sql.DB
}

// Initialize message store
func NewMessageStore() (*MessageStore, error) {
	// Create directory for database if it doesn't exist, readable by the owner only
	if err := ensurePrivateDir("store"); err != nil {
		return nil, fmt.Errorf("failed to create store directory: %v", err)
	}
	if err := ensurePrivateFile("store/messages.db"); err != nil {
		return nil, fmt.Errorf("failed to create message database: %v", err)
	}

	// Open SQLite database for messages
	db, err := sql.Open("sqlite3", "file:store/messages.db?_foreign_keys=on&_journal_mode=WAL&_synchronous=NORMAL&_busy_timeout=5000")
	if err != nil {
		return nil, fmt.Errorf("failed to open message database: %v", err)
	}

	// Create tables if they don't exist
	_, err = db.Exec(`
		CREATE TABLE IF NOT EXISTS chats (
			jid TEXT PRIMARY KEY,
			name TEXT,
			last_message_time TIMESTAMP
		);
		
		CREATE TABLE IF NOT EXISTS messages (
			id TEXT,
			chat_jid TEXT,
			sender TEXT,
			content TEXT,
			timestamp TIMESTAMP,
			is_from_me BOOLEAN,
			media_type TEXT,
			filename TEXT,
			url TEXT,
			media_key BLOB,
			file_sha256 BLOB,
			file_enc_sha256 BLOB,
			file_length INTEGER,
			quoted_message_id TEXT,
			PRIMARY KEY (id, chat_jid),
			FOREIGN KEY (chat_jid) REFERENCES chats(jid)
		);

		-- #278: without these indexes every list_messages / list_chats /
		-- get_message_context / get_last_interaction call is a full-table
		-- scan. Latency then scales linearly with history size. The three
		-- messages indexes cover the hot filter+sort predicates; the chats
		-- index covers the last-active ORDER BY in list_chats. All are
		-- IF NOT EXISTS so an existing store gets them for free on first
		-- start after the upgrade.
		CREATE INDEX IF NOT EXISTS idx_messages_chat_time
			ON messages(chat_jid, timestamp DESC);
		CREATE INDEX IF NOT EXISTS idx_messages_sender_time
			ON messages(sender, timestamp DESC);
		CREATE INDEX IF NOT EXISTS idx_messages_timestamp
			ON messages(timestamp DESC);
		CREATE INDEX IF NOT EXISTS idx_chats_last_message
			ON chats(last_message_time DESC);
	`)
	if err != nil {
		db.Close()
		return nil, fmt.Errorf("failed to create tables: %v", err)
	}

	// Migrate older databases that predate quoted_message_id (#325).
	if _, err := db.Exec(`ALTER TABLE messages ADD COLUMN quoted_message_id TEXT`); err != nil &&
		!strings.Contains(err.Error(), "duplicate column") {
		db.Close()
		return nil, fmt.Errorf("failed to migrate messages table: %v", err)
	}

	// #280: FTS5-backed content search. go-sqlite3 omits FTS5 by default; if
	// the binary was NOT built with `-tags sqlite_fts5`, the CREATE VIRTUAL
	// TABLE will fail with "no such module: fts5". We log a helpful message
	// and continue - the MCP server checks for the FTS table and falls back
	// to LIKE-based search when it isn't present, so this is fully optional.
	_, err = db.Exec(`
		CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
			content,
			content='messages',
			content_rowid='rowid',
			tokenize='unicode61 remove_diacritics 2'
		);
		CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages
			WHEN new.content IS NOT NULL AND new.content != ''
		BEGIN
			INSERT INTO messages_fts(rowid, content) VALUES (new.rowid, new.content);
		END;
		CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages
			WHEN old.content IS NOT NULL AND old.content != ''
		BEGIN
			INSERT INTO messages_fts(messages_fts, rowid, content)
				VALUES('delete', old.rowid, old.content);
		END;
		CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE ON messages
		BEGIN
			INSERT INTO messages_fts(messages_fts, rowid, content)
				SELECT 'delete', old.rowid, old.content
				WHERE old.content IS NOT NULL AND old.content != '';
			INSERT INTO messages_fts(rowid, content)
				SELECT new.rowid, new.content
				WHERE new.content IS NOT NULL AND new.content != '';
		END;
	`)
	if err != nil {
		// A bridge built with the tag may have created these triggers. Without
		// FTS5 they would make every message insert fail, so drop them. The MCP
		// server only searches the index while messages_ai exists, and the next
		// start with FTS5 recreates the triggers and rebuilds the index.
		for _, trigger := range []string{"messages_ai", "messages_ad", "messages_au"} {
			if _, dropErr := db.Exec("DROP TRIGGER IF EXISTS " + trigger); dropErr != nil {
				db.Close()
				return nil, fmt.Errorf("failed to drop FTS trigger %s: %v", trigger, dropErr)
			}
		}
		fmt.Printf(
			"FTS5 unavailable (%v); content search will fall back to LIKE. "+
				"To enable full-text search, run with:  go run -tags sqlite_fts5 .\n",
			err,
		)
	} else {
		// Backfill pre-existing rows into the FTS index. `('rebuild')` is the
		// documented way to (re)populate an external-content FTS5 table;
		// idempotent, so it's safe to run every start.
		if _, rebuildErr := db.Exec(`INSERT INTO messages_fts(messages_fts) VALUES ('rebuild')`); rebuildErr != nil {
			fmt.Printf("FTS5 backfill failed (non-fatal): %v\n", rebuildErr)
		}
	}

	return &MessageStore{db: db}, nil
}

// Close the database connection
func (store *MessageStore) Close() error {
	return store.db.Close()
}

// Store a chat in the database
func (store *MessageStore) StoreChat(jid, name string, lastMessageTime time.Time) error {
	_, err := store.db.Exec(
		`INSERT INTO chats (jid, name, last_message_time) VALUES (?, ?, ?)
		 ON CONFLICT(jid) DO UPDATE SET
		   name = COALESCE(NULLIF(excluded.name, ''), chats.name),
		   last_message_time = excluded.last_message_time`,
		jid, name, lastMessageTime,
	)
	return err
}

// Store a message in the database
func (store *MessageStore) StoreMessage(id, chatJID, sender, content string, timestamp time.Time, isFromMe bool,
	mediaType, filename, url string, mediaKey, fileSHA256, fileEncSHA256 []byte, fileLength uint64, quotedMessageID string) error {
	// Only store if there's actual content or media
	if content == "" && mediaType == "" {
		return nil
	}

	_, err := store.db.Exec(
		`INSERT OR REPLACE INTO messages 
		(id, chat_jid, sender, content, timestamp, is_from_me, media_type, filename, url, media_key, file_sha256, file_enc_sha256, file_length, quoted_message_id)
		VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`,
		id, chatJID, sender, content, timestamp, isFromMe, mediaType, filename, url, mediaKey, fileSHA256, fileEncSHA256, fileLength, nullIfEmpty(quotedMessageID),
	)
	return err
}

// Get messages from a chat
func (store *MessageStore) GetMessages(chatJID string, limit int) ([]Message, error) {
	rows, err := store.db.Query(
		"SELECT sender, content, timestamp, is_from_me, media_type, filename FROM messages WHERE chat_jid = ? ORDER BY timestamp DESC LIMIT ?",
		chatJID, limit,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var messages []Message
	for rows.Next() {
		var msg Message
		var timestamp time.Time
		err := rows.Scan(&msg.Sender, &msg.Content, &timestamp, &msg.IsFromMe, &msg.MediaType, &msg.Filename)
		if err != nil {
			return nil, err
		}
		msg.Time = timestamp
		messages = append(messages, msg)
	}

	return messages, nil
}

// Get all chats
func (store *MessageStore) GetChats() (map[string]time.Time, error) {
	rows, err := store.db.Query("SELECT jid, last_message_time FROM chats ORDER BY last_message_time DESC")
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	chats := make(map[string]time.Time)
	for rows.Next() {
		var jid string
		var lastMessageTime time.Time
		err := rows.Scan(&jid, &lastMessageTime)
		if err != nil {
			return nil, err
		}
		chats[jid] = lastMessageTime
	}

	return chats, nil
}

// unwrapMessage returns the message that actually carries the content.
//
// View-once, ephemeral and document-with-caption messages are envelopes: the
// real Message sits one level down. Live events are already unwrapped by
// whatsmeow, but history sync payloads are not, so without this a captioned
// image in a disappearing-messages chat is stored as if it were empty.
func unwrapMessage(msg *waProto.Message) *waProto.Message {
	// Envelopes do nest (ephemeral wrapping view-once), but the chain is short.
	// The bound is only there so a malformed payload cannot loop forever.
	for i := 0; i < 4 && msg != nil; i++ {
		switch {
		case msg.GetEphemeralMessage().GetMessage() != nil:
			msg = msg.GetEphemeralMessage().GetMessage()
		case msg.GetViewOnceMessage().GetMessage() != nil:
			msg = msg.GetViewOnceMessage().GetMessage()
		case msg.GetViewOnceMessageV2().GetMessage() != nil:
			msg = msg.GetViewOnceMessageV2().GetMessage()
		case msg.GetViewOnceMessageV2Extension().GetMessage() != nil:
			msg = msg.GetViewOnceMessageV2Extension().GetMessage()
		case msg.GetDocumentWithCaptionMessage().GetMessage() != nil:
			msg = msg.GetDocumentWithCaptionMessage().GetMessage()
		default:
			return msg
		}
	}
	return msg
}

// Extract text content from a message
//
// A caption is text, not decoration: when someone sends a screenshot and types
// what to do with it underneath, the caption IS the message. While only
// Conversation and ExtendedTextMessage were read, such a message was stored
// with an empty content column — indistinguishable from an image sent with no
// words at all. Audio messages have no caption field in the protocol.
func extractTextContent(msg *waProto.Message) string {
	msg = unwrapMessage(msg)
	if msg == nil {
		return ""
	}

	// Plain text first — the common case.
	if text := msg.GetConversation(); text != "" {
		return text
	}
	if extendedText := msg.GetExtendedTextMessage(); extendedText != nil {
		if t := extendedText.GetText(); t != "" {
			return t
		}
	}

	// Media captions
	if img := msg.GetImageMessage(); img != nil {
		return img.GetCaption()
	}
	if vid := msg.GetVideoMessage(); vid != nil {
		return vid.GetCaption()
	}
	if doc := msg.GetDocumentMessage(); doc != nil {
		return doc.GetCaption()
	}

	// Non-text messages used to return "" here, which made StoreMessage drop
	// them entirely (it skips when content and mediaType are both empty).
	// StoreChat still ran first and bumped chats.last_message_time, so a chat
	// could look active while holding no rows for that activity — 35 of 102
	// recently-active chats were affected. Render a short text form instead so
	// the message is stored and readable.
	//
	// Prefixes are bracketed and stable so downstream consumers can recognise
	// them without parsing free text.
	if r := msg.GetReactionMessage(); r != nil {
		if t := r.GetText(); t != "" {
			return fmt.Sprintf("[reaction: %s]", t)
		}
		return "[reaction removed]"
	}

	// WhatsApp has migrated polls through V2..V6; a client that only reads V1
	// silently drops every modern poll. Most variants return the poll directly;
	// V4 wraps it in a FutureProofMessage. Signatures verified against the
	// whatsmeow version in go.mod — they differ between releases.
	poll := msg.GetPollCreationMessage()
	if poll == nil {
		poll = msg.GetPollCreationMessageV2()
	}
	if poll == nil {
		poll = msg.GetPollCreationMessageV3()
	}
	if poll == nil {
		if fp := msg.GetPollCreationMessageV4(); fp != nil {
			poll = fp.GetMessage().GetPollCreationMessage()
		}
	}
	if poll == nil {
		poll = msg.GetPollCreationMessageV5()
	}
	if poll == nil {
		poll = msg.GetPollCreationMessageV6()
	}
	if p := poll; p != nil {
		opts := make([]string, 0, len(p.GetOptions()))
		for _, o := range p.GetOptions() {
			if n := o.GetOptionName(); n != "" {
				opts = append(opts, n)
			}
		}
		if len(opts) > 0 {
			return fmt.Sprintf("[poll: %s — options: %s]", p.GetName(), strings.Join(opts, " | "))
		}
		return fmt.Sprintf("[poll: %s]", p.GetName())
	}

	if msg.GetPollUpdateMessage() != nil {
		// The vote itself is encrypted; record that a vote happened.
		return "[poll vote]"
	}

	if l := msg.GetLocationMessage(); l != nil {
		if n := l.GetName(); n != "" {
			return fmt.Sprintf("[location: %s (%.6f, %.6f)]", n, l.GetDegreesLatitude(), l.GetDegreesLongitude())
		}
		return fmt.Sprintf("[location: %.6f, %.6f]", l.GetDegreesLatitude(), l.GetDegreesLongitude())
	}

	if l := msg.GetLiveLocationMessage(); l != nil {
		if c := l.GetCaption(); c != "" {
			return fmt.Sprintf("[live location: %s (%.6f, %.6f)]", c, l.GetDegreesLatitude(), l.GetDegreesLongitude())
		}
		return fmt.Sprintf("[live location: %.6f, %.6f]", l.GetDegreesLatitude(), l.GetDegreesLongitude())
	}

	if c := msg.GetContactMessage(); c != nil {
		return fmt.Sprintf("[contact: %s]", c.GetDisplayName())
	}

	if ca := msg.GetContactsArrayMessage(); ca != nil {
		names := make([]string, 0, len(ca.GetContacts()))
		for _, c := range ca.GetContacts() {
			if n := c.GetDisplayName(); n != "" {
				names = append(names, n)
			}
		}
		return fmt.Sprintf("[contacts: %s]", strings.Join(names, ", "))
	}

	// An edit can arrive as a top-level EditedMessage rather than wrapped in
	// ProtocolMessage, depending on client version.
	if e := msg.GetEditedMessage(); e != nil {
		if inner := extractTextContent(e.GetMessage()); inner != "" {
			return fmt.Sprintf("[edited] %s", inner)
		}
		return "[edited message]"
	}

	// Edits and revokes arrive as ProtocolMessage. An edit carries the new
	// message, so recurse into it rather than losing the corrected text.
	if p := msg.GetProtocolMessage(); p != nil {
		switch p.GetType() {
		case waProto.ProtocolMessage_MESSAGE_EDIT:
			if inner := extractTextContent(p.GetEditedMessage()); inner != "" {
				return fmt.Sprintf("[edited] %s", inner)
			}
			return "[edited message]"
		case waProto.ProtocolMessage_REVOKE:
			return "[message deleted]"
		}
	}

	// Stickers carry no text; extractMediaInfo handles them as media, but give
	// them content too so they are never silently dropped.
	if msg.GetStickerMessage() != nil {
		return "[sticker]"
	}

	return ""
}

// SendMessageResponse represents the response for the send message API
type SendMessageResponse struct {
	Success bool   `json:"success"`
	Message string `json:"message"`
}

// SendMessageRequest represents the request body for the send message API.
// The Reply* fields (added for #121) are optional; when both are set the
// outbound message quotes the referenced original in the recipient's chat.
type SendMessageRequest struct {
	Recipient        string `json:"recipient"`
	Message          string `json:"message"`
	MediaPath        string `json:"media_path,omitempty"`
	ReplyToMessageID string `json:"reply_to_message_id,omitempty"`
	ReplyToSenderJID string `json:"reply_to_sender_jid,omitempty"`
	// Mentions are phone numbers or JIDs. WhatsApp only highlights a mention
	// when the text also contains "@<number>" for it.
	Mentions []string `json:"mentions,omitempty"`
}

// normalizeMentionJIDs turns phone numbers or JID strings into full JIDs.
func normalizeMentionJIDs(mentions []string) []string {
	normalized := make([]string, 0, len(mentions))
	for _, mention := range mentions {
		mention = strings.TrimSpace(strings.TrimPrefix(strings.TrimSpace(mention), "@"))
		if mention == "" {
			continue
		}
		if !strings.Contains(mention, "@") {
			mention = mention + "@s.whatsapp.net"
		}
		normalized = append(normalized, mention)
	}
	return normalized
}

// buildContextInfo returns the reply and mention metadata for an outgoing
// message, or nil when there is none.
//
// #121: a WhatsApp reply carries ContextInfo{StanzaID, Participant} pointing
// at the original. For group replies Participant is required and must be the
// original sender's JID; for direct chats it can be omitted.
func buildContextInfo(replyToMessageID, replyToSenderJID string, mentions []string) *waProto.ContextInfo {
	mentionJIDs := normalizeMentionJIDs(mentions)
	if replyToMessageID == "" && len(mentionJIDs) == 0 {
		return nil
	}
	ctxInfo := &waProto.ContextInfo{}
	if replyToMessageID != "" {
		ctxInfo.StanzaID = proto.String(replyToMessageID)
		if replyToSenderJID != "" {
			ctxInfo.Participant = proto.String(replyToSenderJID)
		}
	}
	if len(mentionJIDs) > 0 {
		ctxInfo.MentionedJID = mentionJIDs
	}
	return ctxInfo
}

// attachContextInfo puts ctxInfo on the media message, or turns plain text
// into an ExtendedTextMessage, because Conversation cannot carry ContextInfo.
func attachContextInfo(msg *waProto.Message, text string, ctxInfo *waProto.ContextInfo) {
	if ctxInfo == nil {
		return
	}
	switch {
	case msg.ExtendedTextMessage != nil:
		msg.ExtendedTextMessage.ContextInfo = ctxInfo
	case msg.ImageMessage != nil:
		msg.ImageMessage.ContextInfo = ctxInfo
	case msg.VideoMessage != nil:
		msg.VideoMessage.ContextInfo = ctxInfo
	case msg.AudioMessage != nil:
		msg.AudioMessage.ContextInfo = ctxInfo
	case msg.DocumentMessage != nil:
		msg.DocumentMessage.ContextInfo = ctxInfo
	default:
		msg.ExtendedTextMessage = &waProto.ExtendedTextMessage{
			Text:        proto.String(text),
			ContextInfo: ctxInfo,
		}
		msg.Conversation = nil
	}
}

// Function to send a WhatsApp message
func sendWhatsAppMessage(client *whatsmeow.Client, messageStore *MessageStore, recipient string, message string, mediaPath string, replyToMessageID string, replyToSenderJID string, mentions []string) (bool, string) {
	if !client.IsConnected() {
		return false, "Not connected to WhatsApp"
	}

	// A phone number or a JID (person or group)
	recipientJID, err := parseRecipient(recipient)
	if err != nil {
		return false, fmt.Sprintf("Error parsing JID: %v", err)
	}

	msg := &waProto.Message{}

	// Check if we have media to send
	if mediaPath != "" {
		safePath, err := allowedMediaPath(mediaPath)
		if err != nil {
			return false, fmt.Sprintf("Media path rejected: %v", err)
		}
		mediaPath = safePath

		// Read media file
		mediaData, err := os.ReadFile(mediaPath)
		if err != nil {
			return false, fmt.Sprintf("Error reading media file: %v", err)
		}

		// Determine media type and mime type based on file extension
		fileExt := strings.ToLower(mediaPath[strings.LastIndex(mediaPath, ".")+1:])
		var mediaType whatsmeow.MediaType
		var mimeType string

		// Handle different media types
		switch fileExt {
		// Image types
		case "jpg", "jpeg":
			mediaType = whatsmeow.MediaImage
			mimeType = "image/jpeg"
		case "png":
			mediaType = whatsmeow.MediaImage
			mimeType = "image/png"
		case "gif":
			mediaType = whatsmeow.MediaImage
			mimeType = "image/gif"
		case "webp":
			mediaType = whatsmeow.MediaImage
			mimeType = "image/webp"

		// Audio types
		case "ogg":
			mediaType = whatsmeow.MediaAudio
			mimeType = "audio/ogg; codecs=opus"

		// Video types
		case "mp4":
			mediaType = whatsmeow.MediaVideo
			mimeType = "video/mp4"
		case "avi":
			mediaType = whatsmeow.MediaVideo
			mimeType = "video/avi"
		case "mov":
			mediaType = whatsmeow.MediaVideo
			mimeType = "video/quicktime"

		// Document types (for any other file type)
		default:
			mediaType = whatsmeow.MediaDocument
			// Without a real mime type WhatsApp mobile renders the document as .bin
			mimeType = mime.TypeByExtension("." + fileExt)
			if mimeType == "" {
				mimeType = "application/octet-stream"
			}
		}

		// Upload media to WhatsApp servers
		resp, err := client.Upload(context.Background(), mediaData, mediaType)
		if err != nil {
			return false, fmt.Sprintf("Error uploading media: %v", err)
		}

		// Never log resp itself: it holds the media URL and its decryption key,
		// which together let anyone with the logs download and open the file.
		fmt.Printf("Media uploaded (%d bytes)\n", resp.FileLength)

		// Create the appropriate message type based on media type
		switch mediaType {
		case whatsmeow.MediaImage:
			msg.ImageMessage = &waProto.ImageMessage{
				Caption:       proto.String(message),
				Mimetype:      proto.String(mimeType),
				URL:           &resp.URL,
				DirectPath:    &resp.DirectPath,
				MediaKey:      resp.MediaKey,
				FileEncSHA256: resp.FileEncSHA256,
				FileSHA256:    resp.FileSHA256,
				FileLength:    &resp.FileLength,
			}
		case whatsmeow.MediaAudio:
			// Handle ogg audio files
			var seconds uint32 = 30 // Default fallback
			var waveform []byte = nil

			// Try to analyze the ogg file
			if strings.Contains(mimeType, "ogg") {
				analyzedSeconds, analyzedWaveform, err := analyzeOggOpus(mediaData)
				if err == nil {
					seconds = analyzedSeconds
					waveform = analyzedWaveform
				} else {
					return false, fmt.Sprintf("Failed to analyze Ogg Opus file: %v", err)
				}
			} else {
				fmt.Printf("Not an Ogg Opus file: %s\n", mimeType)
			}

			msg.AudioMessage = &waProto.AudioMessage{
				Mimetype:      proto.String(mimeType),
				URL:           &resp.URL,
				DirectPath:    &resp.DirectPath,
				MediaKey:      resp.MediaKey,
				FileEncSHA256: resp.FileEncSHA256,
				FileSHA256:    resp.FileSHA256,
				FileLength:    &resp.FileLength,
				Seconds:       proto.Uint32(seconds),
				PTT:           proto.Bool(true),
				Waveform:      waveform,
			}
		case whatsmeow.MediaVideo:
			msg.VideoMessage = &waProto.VideoMessage{
				Caption:       proto.String(message),
				Mimetype:      proto.String(mimeType),
				URL:           &resp.URL,
				DirectPath:    &resp.DirectPath,
				MediaKey:      resp.MediaKey,
				FileEncSHA256: resp.FileEncSHA256,
				FileSHA256:    resp.FileSHA256,
				FileLength:    &resp.FileLength,
			}
		case whatsmeow.MediaDocument:
			msg.DocumentMessage = &waProto.DocumentMessage{
				Title:         proto.String(mediaPath[strings.LastIndex(mediaPath, "/")+1:]),
				FileName:      proto.String(mediaPath[strings.LastIndex(mediaPath, "/")+1:]),
				Caption:       proto.String(message),
				Mimetype:      proto.String(mimeType),
				URL:           &resp.URL,
				DirectPath:    &resp.DirectPath,
				MediaKey:      resp.MediaKey,
				FileEncSHA256: resp.FileEncSHA256,
				FileSHA256:    resp.FileSHA256,
				FileLength:    &resp.FileLength,
			}
		}
	} else {
		msg.Conversation = proto.String(message)
	}

	attachContextInfo(msg, message, buildContextInfo(replyToMessageID, replyToSenderJID, mentions))

	// Send message
	sent, err := client.SendMessage(context.Background(), recipientJID, msg)

	if err != nil {
		return false, fmt.Sprintf("Error sending message: %v", err)
	}
	storeOutgoingMessage(context.Background(), client, messageStore, recipientJID, msg, sent)

	return true, fmt.Sprintf("Message sent to %s", recipient)
}

// Extract media info from a message
// extractQuotedMessageID returns the ID of the message this one replies to
// (quotes), or "". WhatsApp puts it in the ContextInfo of whichever message
// type is set (text, image, sticker, location, ...), so look at all of them.
func extractQuotedMessageID(msg *waProto.Message) string {
	msg = unwrapMessage(msg)
	if msg == nil {
		return ""
	}
	id := ""
	msg.ProtoReflect().Range(func(fd protoreflect.FieldDescriptor, v protoreflect.Value) bool {
		if fd.Kind() != protoreflect.MessageKind || fd.IsList() || fd.IsMap() {
			return true
		}
		inner := v.Message()
		ctxField := inner.Descriptor().Fields().ByName("contextInfo")
		if ctxField == nil || !inner.Has(ctxField) {
			return true
		}
		if ctx, ok := inner.Get(ctxField).Message().Interface().(*waProto.ContextInfo); ok && ctx.GetStanzaID() != "" {
			id = ctx.GetStanzaID()
			return false
		}
		return true
	})
	return id
}

// nullIfEmpty stores "" as NULL, so "not a reply" is NULL in the database.
func nullIfEmpty(s string) any {
	if s == "" {
		return nil
	}
	return s
}

func extractMediaInfo(msg *waProto.Message) (mediaType string, filename string, url string, mediaKey []byte, fileSHA256 []byte, fileEncSHA256 []byte, fileLength uint64) {
	// Same envelopes as in extractTextContent: a document sent with a caption
	// arrives as DocumentWithCaptionMessage, and without unwrapping it would be
	// stored as a message with no attachment at all.
	msg = unwrapMessage(msg)
	if msg == nil {
		return "", "", "", nil, nil, nil, 0
	}

	// Check for image message
	if img := msg.GetImageMessage(); img != nil {
		return "image", "image_" + time.Now().Format("20060102_150405") + ".jpg",
			img.GetURL(), img.GetMediaKey(), img.GetFileSHA256(), img.GetFileEncSHA256(), img.GetFileLength()
	}

	// Check for video message
	if vid := msg.GetVideoMessage(); vid != nil {
		return "video", "video_" + time.Now().Format("20060102_150405") + ".mp4",
			vid.GetURL(), vid.GetMediaKey(), vid.GetFileSHA256(), vid.GetFileEncSHA256(), vid.GetFileLength()
	}

	// Check for audio message
	if aud := msg.GetAudioMessage(); aud != nil {
		return "audio", "audio_" + time.Now().Format("20060102_150405") + ".ogg",
			aud.GetURL(), aud.GetMediaKey(), aud.GetFileSHA256(), aud.GetFileEncSHA256(), aud.GetFileLength()
	}

	// Check for document message
	if doc := msg.GetDocumentMessage(); doc != nil {
		filename := doc.GetFileName()
		if filename == "" {
			filename = "document_" + time.Now().Format("20060102_150405")
		}
		return "document", filename,
			doc.GetURL(), doc.GetMediaKey(), doc.GetFileSHA256(), doc.GetFileEncSHA256(), doc.GetFileLength()
	}

	return "", "", "", nil, nil, nil, 0
}

// Handle regular incoming messages with media support
func handleMessage(client *whatsmeow.Client, messageStore *MessageStore, msg *events.Message, logger waLog.Logger) {
	// Save message to database
	chatJID := msg.Info.Chat.String()
	sender := msg.Info.Sender.User

	// Get appropriate chat name (pass nil for conversation since we don't have one for regular messages)
	name := GetChatName(client, messageStore, msg.Info.Chat, chatJID, nil, sender, logger)

	// Update chat in database with the message timestamp (keeps last message time updated)
	err := messageStore.StoreChat(chatJID, name, msg.Info.Timestamp)
	if err != nil {
		logger.Warnf("Failed to store chat: %v", err)
	}

	// Extract text content
	content := extractTextContent(msg.Message)

	// Edits (and some other updates) arrive as an opaque secretEncryptedMessage
	// envelope rather than as EditedMessage or ProtocolMessage — observed live on
	// 2026-09-06, where an edit logged only "messageContextInfo,
	// secretEncryptedMessage" and was dropped. Decrypt it and re-extract from the
	// inner message. msg.IsEdit is set by whatsmeow when the payload was an edit.
	if content == "" && msg.Message.GetSecretEncryptedMessage() != nil {
		if inner, err := client.DecryptSecretEncryptedMessage(context.Background(), msg); err != nil {
			logger.Warnf("Failed to decrypt secretEncryptedMessage %s: %v", msg.Info.ID, err)
		} else if inner != nil {
			if decrypted := extractTextContent(inner); decrypted != "" {
				if msg.IsEdit {
					content = fmt.Sprintf("[edited] %s", decrypted)
				} else {
					content = decrypted
				}
			}
		}
	}

	// Extract media info
	mediaType, filename, url, mediaKey, fileSHA256, fileEncSHA256, fileLength := extractMediaInfo(msg.Message)

	// Skip if there's no content and no media.
	//
	// This gate is where unhandled message types disappear, and it used to do so
	// invisibly — the "→"/"←" log line below only prints for messages that get
	// stored, so a type extractTextContent does not understand left no trace at
	// all. Log the populated proto fields before returning, so an unrecognised
	// type is discoverable instead of being silently lost.
	if content == "" && mediaType == "" {
		if msg.Message != nil {
			var fields []string
			msg.Message.ProtoReflect().Range(func(fd protoreflect.FieldDescriptor, _ protoreflect.Value) bool {
				fields = append(fields, string(fd.Name()))
				return true
			})
			if len(fields) > 0 {
				logger.Warnf("Dropping message %s in %s: no text or media extracted. Populated fields: %s",
					msg.Info.ID, chatJID, strings.Join(fields, ", "))
			}
		}
		return
	}

	// Store message in database
	err = messageStore.StoreMessage(
		msg.Info.ID,
		chatJID,
		sender,
		content,
		msg.Info.Timestamp,
		msg.Info.IsFromMe,
		mediaType,
		filename,
		url,
		mediaKey,
		fileSHA256,
		fileEncSHA256,
		fileLength,
		extractQuotedMessageID(msg.Message),
	)

	if err != nil {
		logger.Warnf("Failed to store message: %v", err)
	} else {
		// Log message reception
		timestamp := msg.Info.Timestamp.Format("2006-01-02 15:04:05")
		direction := "←"
		if msg.Info.IsFromMe {
			direction = "→"
		}

		// Log based on message type
		if mediaType != "" {
			if logContent {
				fmt.Printf("[%s] %s %s: [%s: %s] %s\n", timestamp, direction, cleanForLog(sender), mediaType, cleanForLog(filename), cleanForLog(content))
			} else {
				fmt.Printf("[%s] %s message stored [%s]\n", timestamp, direction, mediaType)
			}
		} else if content != "" {
			if logContent {
				fmt.Printf("[%s] %s %s: %s\n", timestamp, direction, cleanForLog(sender), cleanForLog(content))
			} else {
				fmt.Printf("[%s] %s message stored\n", timestamp, direction)
			}
		}
	}
}

// DownloadMediaRequest represents the request body for the download media API
type DownloadMediaRequest struct {
	MessageID string `json:"message_id"`
	ChatJID   string `json:"chat_jid"`
}

// DownloadMediaResponse represents the response for the download media API
type DownloadMediaResponse struct {
	Success  bool   `json:"success"`
	Message  string `json:"message"`
	Filename string `json:"filename,omitempty"`
	Path     string `json:"path,omitempty"`
}

// Store additional media info in the database
func (store *MessageStore) StoreMediaInfo(id, chatJID, url string, mediaKey, fileSHA256, fileEncSHA256 []byte, fileLength uint64) error {
	_, err := store.db.Exec(
		"UPDATE messages SET url = ?, media_key = ?, file_sha256 = ?, file_enc_sha256 = ?, file_length = ? WHERE id = ? AND chat_jid = ?",
		url, mediaKey, fileSHA256, fileEncSHA256, fileLength, id, chatJID,
	)
	return err
}

// Get media info from the database
func (store *MessageStore) GetMediaInfo(id, chatJID string) (string, string, string, []byte, []byte, []byte, uint64, error) {
	var mediaType, filename, url string
	var mediaKey, fileSHA256, fileEncSHA256 []byte
	var fileLength uint64

	err := store.db.QueryRow(
		"SELECT media_type, filename, url, media_key, file_sha256, file_enc_sha256, file_length FROM messages WHERE id = ? AND chat_jid = ?",
		id, chatJID,
	).Scan(&mediaType, &filename, &url, &mediaKey, &fileSHA256, &fileEncSHA256, &fileLength)

	return mediaType, filename, url, mediaKey, fileSHA256, fileEncSHA256, fileLength, err
}

// MediaDownloader implements the whatsmeow.DownloadableMessage interface
type MediaDownloader struct {
	URL           string
	DirectPath    string
	MediaKey      []byte
	FileLength    uint64
	FileSHA256    []byte
	FileEncSHA256 []byte
	MediaType     whatsmeow.MediaType
}

// GetDirectPath implements the DownloadableMessage interface
func (d *MediaDownloader) GetDirectPath() string {
	return d.DirectPath
}

// GetURL implements the DownloadableMessage interface
func (d *MediaDownloader) GetURL() string {
	return d.URL
}

// GetMediaKey implements the DownloadableMessage interface
func (d *MediaDownloader) GetMediaKey() []byte {
	return d.MediaKey
}

// GetFileLength implements the DownloadableMessage interface
func (d *MediaDownloader) GetFileLength() uint64 {
	return d.FileLength
}

// GetFileSHA256 implements the DownloadableMessage interface
func (d *MediaDownloader) GetFileSHA256() []byte {
	return d.FileSHA256
}

// GetFileEncSHA256 implements the DownloadableMessage interface
func (d *MediaDownloader) GetFileEncSHA256() []byte {
	return d.FileEncSHA256
}

// GetMediaType implements the DownloadableMessage interface
func (d *MediaDownloader) GetMediaType() whatsmeow.MediaType {
	return d.MediaType
}

// Function to download media from a message
func downloadMedia(client *whatsmeow.Client, messageStore *MessageStore, messageID, chatJID string) (bool, string, string, string, error) {
	// Query the database for the message
	var mediaType, filename, url string
	var mediaKey, fileSHA256, fileEncSHA256 []byte
	var fileLength uint64
	var err error

	// First, check if we already have this file
	chatDir := filepath.Join("store", safeChatDir(chatJID))
	localPath := ""

	// Get media info from the database
	mediaType, filename, url, mediaKey, fileSHA256, fileEncSHA256, fileLength, err = messageStore.GetMediaInfo(messageID, chatJID)

	if err != nil {
		// Try to get basic info if extended info isn't available
		err = messageStore.db.QueryRow(
			"SELECT media_type, filename FROM messages WHERE id = ? AND chat_jid = ?",
			messageID, chatJID,
		).Scan(&mediaType, &filename)

		if err != nil {
			return false, "", "", "", fmt.Errorf("failed to find message: %v", err)
		}
	}

	// Check if this is a media message
	if mediaType == "" {
		return false, "", "", "", fmt.Errorf("not a media message")
	}

	// Create directory for the chat if it doesn't exist
	if err := ensurePrivateDir(chatDir); err != nil {
		return false, "", "", "", fmt.Errorf("failed to create chat directory: %v", err)
	}

	// Generate a local path for the file. Names come from the receive
	// time to the second (or the sender's document name), so two photos sent
	// together share one; give each such message its own file, suffixed with
	// its id, or the second download silently returns the first one's bytes.
	localName := filename
	var sameName int
	if err := messageStore.db.QueryRow(
		"SELECT COUNT(*) FROM messages WHERE chat_jid = ? AND filename = ? AND id != ?",
		chatJID, filename, messageID,
	).Scan(&sameName); err == nil && sameName > 0 {
		ext := filepath.Ext(filename)
		localName = strings.TrimSuffix(filename, ext) + "_" + messageID + ext
	}
	// The filename and message id both come from the sender, so reduce the
	// result to a single safe path component.
	localName = safeName(localName)
	localPath = filepath.Join(chatDir, localName)

	// Get absolute path
	absPath, err := filepath.Abs(localPath)
	if err != nil {
		return false, "", "", "", fmt.Errorf("failed to get absolute path: %v", err)
	}

	// Check if file already exists
	if _, err := os.Stat(localPath); err == nil {
		// File exists, return it
		return true, mediaType, localName, absPath, nil
	}

	// If we don't have all the media info we need, we can't download
	if url == "" || len(mediaKey) == 0 || len(fileSHA256) == 0 || len(fileEncSHA256) == 0 || fileLength == 0 {
		return false, "", "", "", fmt.Errorf("incomplete media information for download")
	}

	fmt.Printf("Attempting to download media for message %s in chat %s...\n", messageID, chatJID)

	// Extract direct path from URL
	directPath := extractDirectPathFromURL(url)

	// Create a downloader that implements DownloadableMessage
	var waMediaType whatsmeow.MediaType
	switch mediaType {
	case "image":
		waMediaType = whatsmeow.MediaImage
	case "video":
		waMediaType = whatsmeow.MediaVideo
	case "audio":
		waMediaType = whatsmeow.MediaAudio
	case "document":
		waMediaType = whatsmeow.MediaDocument
	default:
		return false, "", "", "", fmt.Errorf("unsupported media type: %s", mediaType)
	}

	downloader := &MediaDownloader{
		URL:           url,
		DirectPath:    directPath,
		MediaKey:      mediaKey,
		FileLength:    fileLength,
		FileSHA256:    fileSHA256,
		FileEncSHA256: fileEncSHA256,
		MediaType:     waMediaType,
	}

	// Download the media using whatsmeow client
	mediaData, err := client.Download(context.Background(), downloader)
	if err != nil && isExpiredMediaError(err) {
		// Expired on WhatsApp's servers (#273): ask the sender's phone for it.
		fmt.Printf("Media download failed (%v); asking the sender's phone to upload it again...\n", err)
		ctx, cancel := context.WithTimeout(context.Background(), mediaRetryTimeout+15*time.Second)
		mediaData, err = downloadViaMediaRetry(ctx, client, messageStore, messageID, chatJID, downloader)
		cancel()
	}
	if err != nil {
		return false, "", "", "", fmt.Errorf("failed to download media: %v", err)
	}

	// Save the downloaded media to file
	if err := os.WriteFile(localPath, mediaData, 0600); err != nil {
		return false, "", "", "", fmt.Errorf("failed to save media file: %v", err)
	}

	if logContent {
		fmt.Printf("Successfully downloaded %s media to %s (%d bytes)\n", mediaType, absPath, len(mediaData))
	} else {
		fmt.Printf("Successfully downloaded %s media (%d bytes)\n", mediaType, len(mediaData))
	}
	return true, mediaType, localName, absPath, nil
}

// Extract direct path from a WhatsApp media URL
//
// The direct path must include the URL's query string byte-for-byte:
// whatsmeow's downloader builds the actual CDN request as
// directPath + "&hash=..." + more params, and that query carries the
// request's signature (oh=/oe=/ccb=/etc). Stripping it (the previous
// behavior here) produces an unsigned request that the CDN rejects with
// a 403 -- in practice for every media message, since observed
// production URLs of both known shapes carry a query string 100% of the
// time. Do not rebuild this with u.String()/u.Query().Encode(): both can
// re-escape or reorder the query and break the signature just as
// thoroughly as dropping it outright.
func extractDirectPathFromURL(rawURL string) string {
	u, err := url.Parse(rawURL)
	if err != nil || u.Host == "" || u.EscapedPath() == "" {
		return rawURL // fall back to the original URL if parsing fails
	}

	directPath := u.EscapedPath()
	if u.RawQuery != "" {
		directPath += "?" + u.RawQuery
	}
	return directPath
}

// Start a REST API server to expose the WhatsApp client functionality
// newAPIHandler returns the bridge's REST API, behind the token check.
func newAPIHandler(client *whatsmeow.Client, messageStore *MessageStore) http.Handler {
	mux := http.NewServeMux()

	// Handler for sending messages
	mux.HandleFunc("/api/send", func(w http.ResponseWriter, r *http.Request) {
		// Only allow POST requests
		if r.Method != http.MethodPost {
			http.Error(w, "Method not allowed", http.StatusMethodNotAllowed)
			return
		}

		// Parse the request body
		var req SendMessageRequest
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			http.Error(w, "Invalid request format", http.StatusBadRequest)
			return
		}

		// Validate request
		if req.Recipient == "" {
			http.Error(w, "Recipient is required", http.StatusBadRequest)
			return
		}

		if req.Message == "" && req.MediaPath == "" {
			http.Error(w, "Message or media path is required", http.StatusBadRequest)
			return
		}

		if logContent {
			fmt.Println("Received request to send message", cleanForLog(req.Message), cleanForLog(req.MediaPath))
		} else {
			fmt.Println("Received request to send message")
		}

		recipientJID, err := parseRecipient(req.Recipient)
		if err != nil {
			http.Error(w, "Invalid recipient", http.StatusBadRequest)
			return
		}
		if refuseIfNotAllowed(w, r, client, recipientJID) {
			return
		}

		// Send the message (with optional reply-to context per #121)
		success, message := sendWhatsAppMessage(client, messageStore, req.Recipient, req.Message, req.MediaPath, req.ReplyToMessageID, req.ReplyToSenderJID, req.Mentions)
		logResult("Message sent", success, message)
		// Set response headers
		w.Header().Set("Content-Type", "application/json")

		// Set appropriate status code
		if !success {
			w.WriteHeader(http.StatusInternalServerError)
		}

		// Send response
		json.NewEncoder(w).Encode(SendMessageResponse{
			Success: success,
			Message: message,
		})
	})

	// Handler for downloading media
	registerReactHandler(mux, client, messageStore)

	mux.HandleFunc("/api/download", func(w http.ResponseWriter, r *http.Request) {
		// Only allow POST requests
		if r.Method != http.MethodPost {
			http.Error(w, "Method not allowed", http.StatusMethodNotAllowed)
			return
		}

		// Parse the request body
		var req DownloadMediaRequest
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			http.Error(w, "Invalid request format", http.StatusBadRequest)
			return
		}

		// Validate request
		if req.MessageID == "" || req.ChatJID == "" {
			http.Error(w, "Message ID and Chat JID are required", http.StatusBadRequest)
			return
		}

		// Download the media
		success, mediaType, filename, path, err := downloadMedia(client, messageStore, req.MessageID, req.ChatJID)

		// Set response headers
		w.Header().Set("Content-Type", "application/json")

		// Handle download result
		if !success || err != nil {
			errMsg := "Unknown error"
			if err != nil {
				errMsg = err.Error()
			}

			w.WriteHeader(http.StatusInternalServerError)
			json.NewEncoder(w).Encode(DownloadMediaResponse{
				Success: false,
				Message: fmt.Sprintf("Failed to download media: %s", errMsg),
			})
			return
		}

		// Send successful response
		json.NewEncoder(w).Encode(DownloadMediaResponse{
			Success:  true,
			Message:  fmt.Sprintf("Successfully downloaded %s media", mediaType),
			Filename: filename,
			Path:     path,
		})
	})

	return requireToken(mux)
}

func startRESTServer(client *whatsmeow.Client, messageStore *MessageStore, port int) {
	handler := newAPIHandler(client, messageStore)

	// Start the server
	serverAddr := fmt.Sprintf("127.0.0.1:%d", port)
	fmt.Printf("Starting REST API server on %s...\n", serverAddr)

	// Run server in a goroutine so it doesn't block
	go func() {
		if err := http.ListenAndServe(serverAddr, handler); err != nil {
			fmt.Printf("REST API server error: %v\n", err)
		}
	}()
}

func main() {
	// Set up logger
	logger := waLog.Stdout("Client", "INFO", true)
	configFile, err := loadConfig()
	if err != nil {
		logger.Errorf("%v", err)
		return
	}
	if configFile != "" {
		logger.Infof("Read settings from %s (environment variables take precedence)", configFile)
	}
	bridgeToken = os.Getenv("WHATSAPP_BRIDGE_TOKEN")
	mediaDir = os.Getenv("WHATSAPP_MEDIA_DIR")
	logContent = envFlag(os.Getenv("WHATSAPP_LOG_CONTENT"))

	if len(bridgeToken) < 32 {
		logger.Errorf("WHATSAPP_BRIDGE_TOKEN (or bridge_token in the configuration file) must be set to a random secret of at least 32 characters")
		return
	}
	allow, err := parseSendAllowlist(os.Getenv("WHATSAPP_SEND_ALLOWLIST"))
	if err != nil {
		// Refuse to start rather than guess which chats are meant.
		logger.Errorf("%v", err)
		return
	}
	sendAllow = allow
	if sendAllow != nil {
		logger.Infof("Sending is limited to %d chat(s) in WHATSAPP_SEND_ALLOWLIST", len(sendAllow.jids))
	}
	logger.Infof("Starting WhatsApp client...")

	// Create database connection for storing session data
	dbLog := waLog.Stdout("Database", "INFO", true)

	// Create directory for database if it doesn't exist, readable by the owner only
	if err := ensurePrivateDir("store"); err != nil {
		logger.Errorf("Failed to create store directory: %v", err)
		return
	}
	if err := ensurePrivateFile("store/whatsapp.db"); err != nil {
		logger.Errorf("Failed to create session database: %v", err)
		return
	}

	container, err := sqlstore.New(context.Background(), "sqlite3", "file:store/whatsapp.db?_foreign_keys=on&_journal_mode=WAL&_synchronous=NORMAL&_busy_timeout=5000", dbLog)
	if err != nil {
		logger.Errorf("Failed to connect to database: %v", err)
		return
	}

	// Get device store - This contains session information
	deviceStore, err := container.GetFirstDevice(context.Background())
	if err != nil {
		if err == sql.ErrNoRows {
			// No device exists, create one
			deviceStore = container.NewDevice()
			logger.Infof("Created new device")
		} else {
			logger.Errorf("Failed to get device: %v", err)
			return
		}
	}

	// Create client instance
	client := whatsmeow.NewClient(deviceStore, logger)
	if client == nil {
		logger.Errorf("Failed to create WhatsApp client")
		return
	}

	// Initialize message store
	messageStore, err := NewMessageStore()
	if err != nil {
		logger.Errorf("Failed to initialize message store: %v", err)
		return
	}
	defer messageStore.Close()

	// Setup event handling for messages and history sync
	client.AddEventHandler(func(evt interface{}) {
		switch v := evt.(type) {
		case *events.MediaRetry:
			handleMediaRetry(v)

		case *events.Message:
			// Process regular messages
			handleMessage(client, messageStore, v, logger)

		case *events.HistorySync:
			// Process history sync events
			handleHistorySync(client, messageStore, v, logger)

		case *events.Connected:
			logger.Infof("Connected to WhatsApp")

		case *events.LoggedOut:
			logger.Warnf("Device logged out, please scan QR code to log in again")
		}
	})

	// Create channel to track connection success
	connected := make(chan bool, 1)

	// Connect to WhatsApp
	if client.Store.ID == nil {
		// No ID stored, this is a new client, need to pair with phone
		qrChan, _ := client.GetQRChannel(context.Background())
		err = client.Connect()
		if err != nil {
			logger.Errorf("Failed to connect: %v", err)
			return
		}

		// With WHATSAPP_PAIR_PHONE set (your number with country code), link
		// with an 8-character code instead of a QR code (#273). whatsmeow wants
		// the first QR event before the code is requested.
		pairPhone := strings.Trim(strings.NewReplacer(" ", "", "-", "").Replace(os.Getenv("WHATSAPP_PAIR_PHONE")), "+")
		codeRequested := false

		// Print QR code for pairing with phone
		for evt := range qrChan {
			if evt.Event == "code" && pairPhone != "" {
				if !codeRequested {
					codeRequested = true
					code, err := client.PairPhone(context.Background(), pairPhone, true, whatsmeow.PairClientChrome, "Chrome (macOS)")
					if err != nil {
						logger.Errorf("Failed to request pairing code: %v", err)
						return
					}
					fmt.Printf("\nOn your phone open WhatsApp > Linked devices > Link a device > Link with phone number instead, and enter: %s\n", code)
				}
			} else if evt.Event == "code" {
				fmt.Println("\nScan this QR code with your WhatsApp app:")
				qrterminal.GenerateHalfBlock(evt.Code, qrterminal.L, os.Stdout)
			} else if evt.Event == "success" {
				connected <- true
				break
			}
		}

		// Wait for connection
		select {
		case <-connected:
			fmt.Println("\nSuccessfully connected and authenticated!")
		case <-time.After(3 * time.Minute):
			logger.Errorf("Timeout waiting for QR code scan")
			return
		}
	} else {
		// Already logged in, just connect
		err = client.Connect()
		if err != nil {
			logger.Errorf("Failed to connect: %v", err)
			return
		}
		connected <- true
	}

	// Wait a moment for connection to stabilize
	time.Sleep(2 * time.Second)

	if !client.IsConnected() {
		logger.Errorf("Failed to establish stable connection")
		return
	}

	fmt.Println("\n✓ Connected to WhatsApp! Type 'help' for commands.")

	// Start REST API server
	startRESTServer(client, messageStore, 8080)

	// Create a channel to keep the main goroutine alive
	exitChan := make(chan os.Signal, 1)
	signal.Notify(exitChan, syscall.SIGINT, syscall.SIGTERM)

	fmt.Println("REST server is running. Press Ctrl+C to disconnect and exit.")

	// Wait for termination signal
	<-exitChan

	fmt.Println("Disconnecting...")
	// Disconnect client
	client.Disconnect()
}

// GetChatName determines the appropriate name for a chat based on JID and other info
func GetChatName(client *whatsmeow.Client, messageStore *MessageStore, jid types.JID, chatJID string, conversation interface{}, sender string, logger waLog.Logger) string {
	// First, check if chat already exists in database with a name
	var existingName string
	err := messageStore.db.QueryRow("SELECT name FROM chats WHERE jid = ?", chatJID).Scan(&existingName)
	if err == nil && existingName != "" {
		// Chat exists with a name, use that
		logger.Infof("Using existing chat name for %s: %s", chatJID, existingName)
		return existingName
	}

	// Need to determine chat name
	var name string

	if jid.Server == "g.us" {
		// This is a group chat
		logger.Infof("Getting name for group: %s", chatJID)

		// Use conversation data if provided (from history sync)
		if conversation != nil {
			// Extract name from conversation if available
			// This uses type assertions to handle different possible types
			var displayName, convName *string
			// Try to extract the fields we care about regardless of the exact type
			v := reflect.ValueOf(conversation)
			if v.Kind() == reflect.Ptr && !v.IsNil() {
				v = v.Elem()

				// Try to find DisplayName field
				if displayNameField := v.FieldByName("DisplayName"); displayNameField.IsValid() && displayNameField.Kind() == reflect.Ptr && !displayNameField.IsNil() {
					dn := displayNameField.Elem().String()
					displayName = &dn
				}

				// Try to find Name field
				if nameField := v.FieldByName("Name"); nameField.IsValid() && nameField.Kind() == reflect.Ptr && !nameField.IsNil() {
					n := nameField.Elem().String()
					convName = &n
				}
			}

			// Use the name we found
			if displayName != nil && *displayName != "" {
				name = *displayName
			} else if convName != nil && *convName != "" {
				name = *convName
			}
		}

		// If we didn't get a name, try group info
		if name == "" {
			groupInfo, err := client.GetGroupInfo(context.Background(), jid)
			if err == nil && groupInfo.Name != "" {
				name = groupInfo.Name
			} else {
				// Fallback name for groups
				name = fmt.Sprintf("Group %s", jid.User)
			}
		}

		logger.Infof("Using group name: %s", name)
	} else {
		// This is an individual contact
		logger.Infof("Getting name for contact: %s", chatJID)

		// Just use contact info (full name)
		contact, err := client.Store.Contacts.GetContact(context.Background(), jid)
		if err == nil && contact.FullName != "" {
			name = contact.FullName
		} else if sender != "" {
			// Fallback to sender
			name = sender
		} else {
			// Last fallback to JID
			name = jid.User
		}

		logger.Infof("Using contact name: %s", name)
	}

	return name
}

// Handle history sync events
func handleHistorySync(client *whatsmeow.Client, messageStore *MessageStore, historySync *events.HistorySync, logger waLog.Logger) {
	fmt.Printf("Received history sync event with %d conversations\n", len(historySync.Data.Conversations))

	syncedCount := 0
	for _, conversation := range historySync.Data.Conversations {
		// Parse JID from the conversation
		if conversation.ID == nil {
			continue
		}

		chatJID := *conversation.ID

		// Try to parse the JID
		jid, err := types.ParseJID(chatJID)
		if err != nil {
			logger.Warnf("Failed to parse JID %s: %v", chatJID, err)
			continue
		}

		// Get appropriate chat name by passing the history sync conversation directly
		name := GetChatName(client, messageStore, jid, chatJID, conversation, "", logger)

		// Process messages
		messages := conversation.Messages
		if len(messages) > 0 {
			// Update chat with latest message timestamp
			latestMsg := messages[0]
			if latestMsg == nil || latestMsg.Message == nil {
				continue
			}

			// Get timestamp from message info
			timestamp := time.Time{}
			if ts := latestMsg.Message.GetMessageTimestamp(); ts != 0 {
				timestamp = time.Unix(int64(ts), 0)
			} else {
				continue
			}

			messageStore.StoreChat(chatJID, name, timestamp)

			// Store messages
			for _, msg := range messages {
				if msg == nil || msg.Message == nil {
					continue
				}

				// Extract text content
				// Same path as live messages. Duplicating the extraction here
				// meant captions were dropped only for history-synced messages,
				// which is the harder half of the bug to notice.
				content := extractTextContent(msg.Message.Message)

				// Extract media info
				var mediaType, filename, url string
				var mediaKey, fileSHA256, fileEncSHA256 []byte
				var fileLength uint64

				if msg.Message.Message != nil {
					mediaType, filename, url, mediaKey, fileSHA256, fileEncSHA256, fileLength = extractMediaInfo(msg.Message.Message)
				}

				// Log the message content for debugging
				if logContent {
					logger.Infof("Message content: %v, Media Type: %v", cleanForLog(content), mediaType)
				}

				// Skip messages with no content and no media
				if content == "" && mediaType == "" {
					continue
				}

				// Determine sender
				var sender string
				isFromMe := false
				if msg.Message.Key != nil {
					if msg.Message.Key.FromMe != nil {
						isFromMe = *msg.Message.Key.FromMe
					}
					if !isFromMe && msg.Message.Key.Participant != nil && *msg.Message.Key.Participant != "" {
						sender = *msg.Message.Key.Participant
					} else if isFromMe {
						sender = client.Store.ID.User
					} else {
						sender = jid.User
					}
				} else {
					sender = jid.User
				}

				// Store message
				msgID := ""
				if msg.Message.Key != nil && msg.Message.Key.ID != nil {
					msgID = *msg.Message.Key.ID
				}

				// Get message timestamp
				timestamp := time.Time{}
				if ts := msg.Message.GetMessageTimestamp(); ts != 0 {
					timestamp = time.Unix(int64(ts), 0)
				} else {
					continue
				}

				err = messageStore.StoreMessage(
					msgID,
					chatJID,
					sender,
					content,
					timestamp,
					isFromMe,
					mediaType,
					filename,
					url,
					mediaKey,
					fileSHA256,
					fileEncSHA256,
					fileLength,
					extractQuotedMessageID(msg.Message.Message),
				)
				if err != nil {
					logger.Warnf("Failed to store history message: %v", err)
				} else {
					syncedCount++
					// Log each stored message only when WHATSAPP_LOG_CONTENT is on;
					// otherwise the count is reported at the end of the sync.
					if logContent {
						if mediaType != "" {
							logger.Infof("Stored message: [%s] %s -> %s: [%s: %s] %s",
								timestamp.Format("2006-01-02 15:04:05"), cleanForLog(sender), chatJID, mediaType, cleanForLog(filename), cleanForLog(content))
						} else {
							logger.Infof("Stored message: [%s] %s -> %s: %s",
								timestamp.Format("2006-01-02 15:04:05"), cleanForLog(sender), chatJID, cleanForLog(content))
						}
					}
				}
			}
		}
	}

	fmt.Printf("History sync complete. Stored %d messages.\n", syncedCount)
}

// Request history sync from the server
func requestHistorySync(client *whatsmeow.Client) {
	if client == nil {
		fmt.Println("Client is not initialized. Cannot request history sync.")
		return
	}

	if !client.IsConnected() {
		fmt.Println("Client is not connected. Please ensure you are connected to WhatsApp first.")
		return
	}

	if client.Store.ID == nil {
		fmt.Println("Client is not logged in. Please scan the QR code first.")
		return
	}

	// Build and send a history sync request
	historyMsg := client.BuildHistorySyncRequest(nil, 100)
	if historyMsg == nil {
		fmt.Println("Failed to build history sync request.")
		return
	}

	_, err := client.SendMessage(context.Background(), types.JID{
		Server: "s.whatsapp.net",
		User:   "status",
	}, historyMsg)

	if err != nil {
		fmt.Printf("Failed to request history sync: %v\n", err)
	} else {
		fmt.Println("History sync requested. Waiting for server response...")
	}
}

// analyzeOggOpus tries to extract duration and generate a simple waveform from an Ogg Opus file
func analyzeOggOpus(data []byte) (duration uint32, waveform []byte, err error) {
	// Try to detect if this is a valid Ogg file by checking for the "OggS" signature
	// at the beginning of the file
	if len(data) < 4 || string(data[0:4]) != "OggS" {
		return 0, nil, fmt.Errorf("not a valid Ogg file (missing OggS signature)")
	}

	// Parse Ogg pages to find the last page with a valid granule position
	var lastGranule uint64
	var sampleRate uint32 = 48000 // Default Opus sample rate
	var preSkip uint16 = 0
	var foundOpusHead bool

	// Scan through the file looking for Ogg pages
	for i := 0; i < len(data); {
		// Check if we have enough data to read Ogg page header
		if i+27 >= len(data) {
			break
		}

		// Verify Ogg page signature
		if string(data[i:i+4]) != "OggS" {
			// Skip until next potential page
			i++
			continue
		}

		// Extract header fields
		granulePos := binary.LittleEndian.Uint64(data[i+6 : i+14])
		pageSeqNum := binary.LittleEndian.Uint32(data[i+18 : i+22])
		numSegments := int(data[i+26])

		// Extract segment table
		if i+27+numSegments >= len(data) {
			break
		}
		segmentTable := data[i+27 : i+27+numSegments]

		// Calculate page size
		pageSize := 27 + numSegments
		for _, segLen := range segmentTable {
			pageSize += int(segLen)
		}

		// A malformed page that claims to run past the end of the file would panic below
		if i+pageSize > len(data) {
			break
		}

		// Check if we're looking at an OpusHead packet (should be in first few pages)
		if !foundOpusHead && pageSeqNum <= 1 {
			// Look for "OpusHead" marker in this page
			pageData := data[i : i+pageSize]
			headPos := bytes.Index(pageData, []byte("OpusHead"))
			if headPos >= 0 && headPos+12 < len(pageData) {
				// Found OpusHead, extract sample rate and pre-skip
				// OpusHead format: Magic(8) + Version(1) + Channels(1) + PreSkip(2) + SampleRate(4) + ...
				headPos += 8 // Skip "OpusHead" marker
				// PreSkip is 2 bytes at offset 10
				if headPos+16 <= len(pageData) {
					preSkip = binary.LittleEndian.Uint16(pageData[headPos+10 : headPos+12])
					sampleRate = binary.LittleEndian.Uint32(pageData[headPos+12 : headPos+16])
					foundOpusHead = true
					fmt.Printf("Found OpusHead: sampleRate=%d, preSkip=%d\n", sampleRate, preSkip)
				}
			}
		}

		// Keep track of last valid granule position
		if granulePos != 0 {
			lastGranule = granulePos
		}

		// Move to next page
		i += pageSize
	}

	if !foundOpusHead {
		fmt.Println("Warning: OpusHead not found, using default values")
	}

	// Calculate duration based on granule position
	if lastGranule > 0 {
		// Formula for duration: (lastGranule - preSkip) / sampleRate
		durationSeconds := float64(lastGranule-uint64(preSkip)) / float64(sampleRate)
		duration = uint32(math.Ceil(durationSeconds))
		fmt.Printf("Calculated Opus duration from granule: %f seconds (lastGranule=%d)\n",
			durationSeconds, lastGranule)
	} else {
		// Fallback to rough estimation if granule position not found
		fmt.Println("Warning: No valid granule position found, using estimation")
		durationEstimate := float64(len(data)) / 2000.0 // Very rough approximation
		duration = uint32(durationEstimate)
	}

	// Make sure we have a reasonable duration (at least 1 second, at most 300 seconds)
	if duration < 1 {
		duration = 1
	} else if duration > 300 {
		duration = 300
	}

	// Generate waveform
	waveform = placeholderWaveform(duration)

	fmt.Printf("Ogg Opus analysis: size=%d bytes, calculated duration=%d sec, waveform=%d bytes\n",
		len(data), duration, len(waveform))

	return duration, waveform, nil
}

// min returns the smaller of x or y
func min(x, y int) int {
	if x < y {
		return x
	}
	return y
}

// placeholderWaveform generates a synthetic waveform for WhatsApp voice messages
// that appears natural with some variability based on the duration
func placeholderWaveform(duration uint32) []byte {
	// WhatsApp expects a 64-byte waveform for voice messages
	const waveformLength = 64
	waveform := make([]byte, waveformLength)

	// Seed the random number generator for consistent results with the same duration
	rand.Seed(int64(duration))

	// Create a more natural looking waveform with some patterns and variability
	// rather than completely random values

	// Base amplitude and frequency - longer messages get faster frequency
	baseAmplitude := 35.0
	frequencyFactor := float64(min(int(duration), 120)) / 30.0

	for i := range waveform {
		// Position in the waveform (normalized 0-1)
		pos := float64(i) / float64(waveformLength)

		// Create a wave pattern with some randomness
		// Use multiple sine waves of different frequencies for more natural look
		val := baseAmplitude * math.Sin(pos*math.Pi*frequencyFactor*8)
		val += (baseAmplitude / 2) * math.Sin(pos*math.Pi*frequencyFactor*16)

		// Add some randomness to make it look more natural
		val += (rand.Float64() - 0.5) * 15

		// Add some fade-in and fade-out effects
		fadeInOut := math.Sin(pos * math.Pi)
		val = val * (0.7 + 0.3*fadeInOut)

		// Center around 50 (typical voice baseline)
		val = val + 50

		// Ensure values stay within WhatsApp's expected range (0-100)
		if val < 0 {
			val = 0
		} else if val > 100 {
			val = 100
		}

		waveform[i] = byte(val)
	}

	return waveform
}
