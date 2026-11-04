package main

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"sort"
	"strings"

	"go.mau.fi/whatsmeow"
	"go.mau.fi/whatsmeow/store"
	"go.mau.fi/whatsmeow/types"
)

// WHATSAPP_SEND_ALLOWLIST limits which chats the bridge sends messages and
// reactions to: phone numbers (country code, no +) or JIDs, including groups
// (...@g.us), separated by commas, semicolons or spaces. Not set or empty
// means no limit. A message that tries to make the model send elsewhere
// (prompt injection) is then refused by the bridge itself, also for calls
// that bypass the MCP server.
var sendAllow *sendAllowlist

type sendAllowlist struct {
	jids map[string]bool // non-AD JID strings
}

// parseRecipient turns a phone number or JID into the JID to send to.
func parseRecipient(recipient string) (types.JID, error) {
	recipient = strings.TrimSpace(recipient)
	if strings.Contains(recipient, "@") {
		return types.ParseJID(recipient)
	}
	return types.NewJID(recipient, types.DefaultUserServer), nil
}

// parseAllowEntry accepts a phone number, written with or without +, spaces,
// dashes, dots or brackets, or a JID on a user or group server.
func parseAllowEntry(entry string) (types.JID, error) {
	if strings.Contains(entry, "@") {
		jid, err := types.ParseJID(entry)
		if err != nil {
			return types.EmptyJID, err
		}
		switch jid.Server {
		case types.DefaultUserServer, types.HiddenUserServer, types.GroupServer:
		default:
			return types.EmptyJID, fmt.Errorf("unsupported server %q", jid.Server)
		}
		if jid.User == "" {
			return types.EmptyJID, fmt.Errorf("missing user part")
		}
		return jid.ToNonAD(), nil
	}
	digits := strings.Map(func(r rune) rune {
		switch r {
		case '+', ' ', '-', '.', '(', ')':
			return -1
		}
		return r
	}, entry)
	if digits == "" || strings.Trim(digits, "0123456789") != "" {
		return types.EmptyJID, fmt.Errorf("not a phone number or JID")
	}
	return types.NewJID(digits, types.DefaultUserServer), nil
}

// parseSendAllowlist reads WHATSAPP_SEND_ALLOWLIST. It returns nil (no limit)
// for an empty value, and an error for any entry it cannot read, so a typo
// does not silently allow or block the wrong chats.
func parseSendAllowlist(value string) (*sendAllowlist, error) {
	fields := strings.FieldsFunc(value, func(r rune) bool { return r == ',' || r == ';' || r == '\n' || r == '\t' })
	list := &sendAllowlist{jids: map[string]bool{}}
	for _, field := range fields {
		field = strings.TrimSpace(field)
		if field == "" {
			continue
		}
		jid, err := parseAllowEntry(field)
		if err != nil {
			return nil, fmt.Errorf("WHATSAPP_SEND_ALLOWLIST entry %q: %v", field, err)
		}
		list.jids[jid.String()] = true
	}
	if len(list.jids) == 0 {
		return nil, nil
	}
	return list, nil
}

// allows reports whether the bridge may send to chat. A contact can be
// addressed by phone number or by LID; either form in the list allows both.
func (a *sendAllowlist) allows(ctx context.Context, lids store.LIDStore, chat types.JID) bool {
	if a == nil {
		return true
	}
	chat = chat.ToNonAD()
	if a.jids[chat.String()] {
		return true
	}
	if lids == nil {
		return false
	}
	var other types.JID
	var err error
	switch chat.Server {
	case types.DefaultUserServer:
		other, err = lids.GetLIDForPN(ctx, chat)
	case types.HiddenUserServer:
		other, err = lids.GetPNForLID(ctx, chat)
	default:
		return false
	}
	return err == nil && !other.IsEmpty() && a.jids[other.ToNonAD().String()]
}

func (a *sendAllowlist) String() string {
	if a == nil {
		return "no limit"
	}
	list := make([]string, 0, len(a.jids))
	for jid := range a.jids {
		list = append(list, jid)
	}
	sort.Strings(list)
	return strings.Join(list, ", ")
}

// refuseIfNotAllowed writes a 403 response and returns true when chat is not
// in WHATSAPP_SEND_ALLOWLIST. Handlers call it before sending anything.
func refuseIfNotAllowed(w http.ResponseWriter, r *http.Request, client *whatsmeow.Client, chat types.JID) bool {
	var lids store.LIDStore
	if client != nil && client.Store != nil {
		lids = client.Store.LIDs
	}
	if sendAllow.allows(r.Context(), lids, chat) {
		return false
	}
	fmt.Println("Refused to send: chat is not in WHATSAPP_SEND_ALLOWLIST")
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusForbidden)
	json.NewEncoder(w).Encode(SendMessageResponse{
		Success: false,
		Message: "This chat is not in WHATSAPP_SEND_ALLOWLIST, so the bridge will not send to it.",
	})
	return true
}
