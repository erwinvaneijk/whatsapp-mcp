package main

import (
	"context"
	"database/sql"
	"os"
	"testing"
	"time"

	"go.mau.fi/whatsmeow"
	waProto "go.mau.fi/whatsmeow/binary/proto"
	"go.mau.fi/whatsmeow/types"
	"google.golang.org/protobuf/proto"
)

func newTestStore(t *testing.T) *MessageStore {
	t.Helper()
	t.Chdir(t.TempDir())
	if err := os.Mkdir("store", 0o700); err != nil {
		t.Fatal(err)
	}
	s, err := NewMessageStore()
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { s.Close() })
	return s
}

type storedRow struct {
	chat, sender, content, mediaType, filename string
	fromMe                                     bool
	quoted                                     sql.NullString
}

func readRow(t *testing.T, s *MessageStore, id string) storedRow {
	t.Helper()
	var r storedRow
	var mediaType, filename sql.NullString
	err := s.db.QueryRow(`SELECT chat_jid, sender, content, is_from_me, media_type, filename, quoted_message_id
		FROM messages WHERE id = ?`, id).Scan(&r.chat, &r.sender, &r.content, &r.fromMe, &mediaType, &filename, &r.quoted)
	if err != nil {
		t.Fatalf("message %s not stored: %v", id, err)
	}
	r.mediaType, r.filename = mediaType.String, filename.String
	return r
}

func TestOutgoingChatJID(t *testing.T) {
	s := newTestStore(t)
	ctx := context.Background()
	pn := types.NewJID("31612345678", types.DefaultUserServer)
	lid := types.NewJID("123456789012345", types.HiddenUserServer)
	lids := fakeLIDs{pnForLID: map[string]string{lid.User: pn.User}}
	lidsBoth := bothWayLIDs{fakeLIDs: lids, lidForPN: map[string]string{pn.User: lid.User}}

	// No chat yet: the recipient as given.
	if got := outgoingChatJID(ctx, s, lidsBoth, pn); got != pn.String() {
		t.Errorf("new chat: got %s", got)
	}
	// Received messages are stored under the LID: use that chat.
	s.StoreChat(lid.String(), "Maria", time.Now())
	if got := outgoingChatJID(ctx, s, lidsBoth, pn); got != lid.String() {
		t.Errorf("LID chat exists: got %s, want %s", got, lid)
	}
	// And the other way round.
	s.StoreChat(pn.String(), "Maria", time.Now())
	if got := outgoingChatJID(ctx, s, lidsBoth, lid); got != lid.String() {
		t.Errorf("own chat exists: got %s, want %s", got, lid)
	}
	// Groups are used as they are.
	g := types.NewJID("120363000000000000", types.GroupServer)
	if got := outgoingChatJID(ctx, s, lidsBoth, g); got != g.String() {
		t.Errorf("group: got %s", got)
	}
}

// bothWayLIDs also maps phone numbers to LIDs.
type bothWayLIDs struct {
	fakeLIDs
	lidForPN map[string]string
}

func (b bothWayLIDs) GetLIDForPN(_ context.Context, pn types.JID) (types.JID, error) {
	if l, ok := b.lidForPN[pn.User]; ok {
		return types.NewJID(l, types.HiddenUserServer), nil
	}
	return types.EmptyJID, nil
}

func TestSaveSentMessage(t *testing.T) {
	s := newTestStore(t)
	ctx := context.Background()
	group := types.NewJID("120363000000000000", types.GroupServer)
	s.StoreChat(group.String(), "Book club", time.Now().Add(-time.Hour))
	now := time.Now()

	// A reply with a mention, as sendWhatsAppMessage builds it.
	text := &waProto.Message{Conversation: proto.String("Ja @31612345678")}
	attachContextInfo(text, "Ja @31612345678", buildContextInfo("ORIG1", "1@lid", []string{"31612345678"}))
	saveSentMessage(ctx, s, nil, "31600000000", group, text, whatsmeow.SendResponse{ID: "OUT1", Timestamp: now})

	r := readRow(t, s, "OUT1")
	if r.chat != group.String() || r.sender != "31600000000" || !r.fromMe || r.content != "Ja @31612345678" || r.quoted.String != "ORIG1" {
		t.Errorf("text reply stored as %+v", r)
	}

	// A document with a caption.
	doc := &waProto.Message{DocumentMessage: &waProto.DocumentMessage{
		FileName: proto.String("menu.pdf"), Caption: proto.String("het menu"), URL: proto.String("https://mmg.whatsapp.net/x"),
	}}
	saveSentMessage(ctx, s, nil, "31600000000", group, doc, whatsmeow.SendResponse{ID: "OUT2", Timestamp: now})
	if r := readRow(t, s, "OUT2"); r.mediaType != "document" || r.filename != "menu.pdf" || r.content != "het menu" {
		t.Errorf("document stored as %+v", r)
	}

	// A reaction, stored like a received one.
	react := &waProto.Message{ReactionMessage: &waProto.ReactionMessage{Text: proto.String("👍")}}
	saveSentMessage(ctx, s, nil, "31600000000", group, react, whatsmeow.SendResponse{ID: "OUT3", Timestamp: now})
	if r := readRow(t, s, "OUT3"); r.content != "[reaction: 👍]" {
		t.Errorf("reaction stored as %+v", r)
	}

	// Storing the chat without a name keeps the known name.
	var name string
	s.db.QueryRow(`SELECT name FROM chats WHERE jid = ?`, group.String()).Scan(&name)
	if name != "Book club" {
		t.Errorf("chat name became %q, want Book club", name)
	}
}
