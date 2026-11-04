package main

import (
	"context"
	"fmt"

	"go.mau.fi/whatsmeow"
	waProto "go.mau.fi/whatsmeow/binary/proto"
	"go.mau.fi/whatsmeow/store"
	"go.mau.fi/whatsmeow/types"
)

// outgoingChatJID picks the chat to store a sent message in. WhatsApp knows a
// contact by phone number and by LID, and received messages may be stored
// under either; use whichever chat already exists, so the conversation is not
// split. Without one, use the recipient as given.
//
// Based on upstream PR #383, which always preferred the LID.
func outgoingChatJID(ctx context.Context, messageStore *MessageStore, lids store.LIDStore, recipient types.JID) string {
	recipient = recipient.ToNonAD()
	if messageStore.chatExists(recipient.String()) || lids == nil {
		return recipient.String()
	}
	var other types.JID
	var err error
	switch recipient.Server {
	case types.DefaultUserServer:
		other, err = lids.GetLIDForPN(ctx, recipient)
	case types.HiddenUserServer:
		other, err = lids.GetPNForLID(ctx, recipient)
	default:
		return recipient.String()
	}
	if err == nil && !other.IsEmpty() && messageStore.chatExists(other.String()) {
		return other.String()
	}
	return recipient.String()
}

func (store *MessageStore) chatExists(jid string) bool {
	var one int
	return store.db.QueryRow(`SELECT 1 FROM chats WHERE jid = ?`, jid).Scan(&one) == nil
}

// storeOutgoingMessage saves a message sent through the API. whatsmeow emits
// no event for messages this client sends, so without this they never reach
// messages.db: Claude would not see them, and could not reply or react to
// them. The text, media and reply fields come from the sent message itself,
// through the same functions used for received messages.
func storeOutgoingMessage(ctx context.Context, client *whatsmeow.Client, messageStore *MessageStore,
	recipient types.JID, msg *waProto.Message, resp whatsmeow.SendResponse) {
	if messageStore == nil || client.Store == nil || client.Store.ID == nil {
		return
	}
	saveSentMessage(ctx, messageStore, client.Store.LIDs, client.Store.ID.User, recipient, msg, resp)
}

// saveSentMessage does the work of storeOutgoingMessage; ownUser is the user
// part of our own JID, stored as the sender like in history sync.
func saveSentMessage(ctx context.Context, messageStore *MessageStore, lids store.LIDStore, ownUser string,
	recipient types.JID, msg *waProto.Message, resp whatsmeow.SendResponse) {
	chatJID := outgoingChatJID(ctx, messageStore, lids, recipient)

	content := extractTextContent(msg)
	mediaType, filename, url, mediaKey, fileSHA256, fileEncSHA256, fileLength := extractMediaInfo(msg)

	if err := messageStore.StoreChat(chatJID, "", resp.Timestamp); err != nil {
		fmt.Printf("Warning: failed to store chat for sent message: %v\n", err)
		return
	}
	if err := messageStore.StoreMessage(resp.ID, chatJID, ownUser, content, resp.Timestamp, true,
		mediaType, filename, url, mediaKey, fileSHA256, fileEncSHA256, fileLength,
		extractQuotedMessageID(msg)); err != nil {
		fmt.Printf("Warning: failed to store sent message: %v\n", err)
	}
}
