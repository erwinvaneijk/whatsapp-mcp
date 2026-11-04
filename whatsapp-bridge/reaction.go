package main

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"strings"

	"go.mau.fi/whatsmeow"
	"go.mau.fi/whatsmeow/store"
	"go.mau.fi/whatsmeow/types"
)

// ReactMessageRequest is the body of POST /api/react. An empty emoji removes
// your reaction.
type ReactMessageRequest struct {
	MessageID string `json:"message_id"`
	ChatJID   string `json:"chat_jid"`
	Emoji     string `json:"emoji"`
}

// resolveStoredSender turns a sender from messages.db into a JID. History
// sync stores the full JID; live messages store only the user part, which can
// be a phone number or a LID. A bare value counts as a LID when whatsmeow
// knows a phone number for it.
func resolveStoredSender(ctx context.Context, lids store.LIDStore, sender string) (types.JID, error) {
	sender = strings.TrimSpace(sender)
	if sender == "" {
		return types.EmptyJID, errors.New("message has no sender")
	}
	if strings.Contains(sender, "@") {
		return types.ParseJID(sender)
	}
	lid := types.NewJID(sender, types.HiddenUserServer)
	if lids != nil {
		if pn, err := lids.GetPNForLID(ctx, lid); err == nil && !pn.IsEmpty() {
			return lid, nil
		}
	}
	return types.NewJID(sender, types.DefaultUserServer), nil
}

// sendReaction reacts to a stored message. The message must be in
// messages.db, because the reaction has to name the original sender.
func sendReaction(ctx context.Context, client *whatsmeow.Client, messageStore *MessageStore, messageID, chatJID, emoji string) (bool, string) {
	if !client.IsConnected() {
		return false, "Not connected to WhatsApp"
	}
	chat, err := types.ParseJID(chatJID)
	if err != nil {
		return false, fmt.Sprintf("Error parsing chat JID: %v", err)
	}

	var sender string
	var isFromMe bool
	err = messageStore.db.QueryRow(
		`SELECT sender, is_from_me FROM messages WHERE id = ? AND chat_jid = ?`,
		messageID, chatJID,
	).Scan(&sender, &isFromMe)
	if errors.Is(err, sql.ErrNoRows) {
		return false, "Message not found in this chat"
	} else if err != nil {
		return false, fmt.Sprintf("Error finding message: %v", err)
	}

	// BuildReaction marks the key as our own message for an empty sender or
	// one matching our phone number or LID, and only adds the participant in
	// group chats.
	senderJID := types.EmptyJID
	if !isFromMe {
		if senderJID, err = resolveStoredSender(ctx, client.Store.LIDs, sender); err != nil {
			return false, fmt.Sprintf("Error parsing sender: %v", err)
		}
	}

	reaction := client.BuildReaction(chat, senderJID, messageID, emoji)
	sent, err := client.SendMessage(ctx, chat, reaction)
	if err != nil {
		return false, fmt.Sprintf("Error sending reaction: %v", err)
	}
	// Stored like a received reaction, as "[reaction: 👍]".
	storeOutgoingMessage(ctx, client, messageStore, chat, reaction, sent)
	if emoji == "" {
		return true, "Reaction removed"
	}
	return true, fmt.Sprintf("Reaction %s sent", emoji)
}

// registerReactHandler adds POST /api/react to the bridge API. It is served
// behind the same token check as the other endpoints.
func registerReactHandler(mux *http.ServeMux, client *whatsmeow.Client, messageStore *MessageStore) {
	mux.HandleFunc("/api/react", func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodPost {
			http.Error(w, "Method not allowed", http.StatusMethodNotAllowed)
			return
		}
		var req ReactMessageRequest
		if err := json.NewDecoder(r.Body).Decode(&req); err != nil {
			http.Error(w, "Invalid request format", http.StatusBadRequest)
			return
		}
		if req.MessageID == "" || req.ChatJID == "" {
			http.Error(w, "message_id and chat_jid are required", http.StatusBadRequest)
			return
		}

		if logContent {
			fmt.Println("Received request to react to message", cleanForLog(req.MessageID), cleanForLog(req.Emoji))
		} else {
			fmt.Println("Received request to react to message", cleanForLog(req.MessageID))
		}
		chat, err := types.ParseJID(req.ChatJID)
		if err != nil {
			http.Error(w, "Invalid chat_jid", http.StatusBadRequest)
			return
		}
		if refuseIfNotAllowed(w, r, client, chat) {
			return
		}
		success, message := sendReaction(r.Context(), client, messageStore, req.MessageID, req.ChatJID, req.Emoji)
		logResult("Reaction sent", success, message)

		w.Header().Set("Content-Type", "application/json")
		if !success {
			w.WriteHeader(http.StatusInternalServerError)
		}
		json.NewEncoder(w).Encode(SendMessageResponse{Success: success, Message: message})
	})
}
