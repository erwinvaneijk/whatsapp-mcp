package main

import (
	"context"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"go.mau.fi/whatsmeow/types"
)

func TestParseSendAllowlist(t *testing.T) {
	for _, empty := range []string{"", "  ", " , ;"} {
		if a, err := parseSendAllowlist(empty); a != nil || err != nil {
			t.Errorf("%q: want no limit, got %v, %v", empty, a, err)
		}
	}

	a, err := parseSendAllowlist("+31 6-1234 5678, 120363000000000000@g.us;123456789012345@lid\n31687654321:3@s.whatsapp.net")
	if err != nil {
		t.Fatal(err)
	}
	want := "120363000000000000@g.us, 123456789012345@lid, 31612345678@s.whatsapp.net, 31687654321@s.whatsapp.net"
	if a.String() != want {
		t.Errorf("got %s\nwant %s", a, want)
	}

	// A typo stops the bridge instead of silently allowing or blocking chats.
	for _, bad := range []string{"Sien", "0612abc", "status@broadcast", "@s.whatsapp.net", "31612345678, oops"} {
		if _, err := parseSendAllowlist(bad); err == nil {
			t.Errorf("%q: want an error", bad)
		}
	}
}

func TestSendAllowlistAllows(t *testing.T) {
	ctx := context.Background()
	pn := types.NewJID("31612345678", types.DefaultUserServer)
	lid := types.NewJID("123456789012345", types.HiddenUserServer)
	group := types.NewJID("120363000000000000", types.GroupServer)
	other := types.NewJID("31699999999", types.DefaultUserServer)
	lids := bothWayLIDs{fakeLIDs: fakeLIDs{pnForLID: map[string]string{lid.User: pn.User}}, lidForPN: map[string]string{pn.User: lid.User}}

	var none *sendAllowlist
	if !none.allows(ctx, lids, other) {
		t.Error("no allowlist must allow everything")
	}

	byNumber, _ := parseSendAllowlist("31612345678, 120363000000000000@g.us")
	device := pn
	device.Device = 2
	for _, jid := range []types.JID{pn, device, lid, group} {
		if !byNumber.allows(ctx, lids, jid) {
			t.Errorf("listed by phone number: %s must be allowed", jid)
		}
	}
	if byNumber.allows(ctx, lids, other) {
		t.Error("an unlisted number must be refused")
	}
	if byNumber.allows(ctx, nil, lid) {
		t.Error("without a LID mapping, the LID cannot be matched to the listed number")
	}

	byLID, _ := parseSendAllowlist("123456789012345@lid")
	if !byLID.allows(ctx, lids, pn) {
		t.Error("listed by LID: the phone number must be allowed too")
	}
}

// The bridge refuses before anything is sent, also for direct API calls.
func TestAPIRefusesUnlistedChats(t *testing.T) {
	oldToken, oldAllow := bridgeToken, sendAllow
	defer func() { bridgeToken, sendAllow = oldToken, oldAllow }()
	bridgeToken = strings.Repeat("t", 32)
	sendAllow, _ = parseSendAllowlist("31612345678")

	// No WhatsApp client: a request that got past the allowlist would crash
	// on it, so a clean 403 shows the check comes first.
	api := newAPIHandler(nil, nil)
	post := func(path, body string) *httptest.ResponseRecorder {
		req := httptest.NewRequest(http.MethodPost, path, strings.NewReader(body))
		req.Header.Set("Content-Type", "application/json")
		req.Header.Set("X-Bridge-Token", bridgeToken)
		rec := httptest.NewRecorder()
		api.ServeHTTP(rec, req)
		return rec
	}

	for _, tc := range []struct{ path, body string }{
		{"/api/send", `{"recipient":"31699999999","message":"hi"}`},
		{"/api/send", `{"recipient":"120363000000000000@g.us","message":"hi"}`},
		{"/api/react", `{"message_id":"M1","chat_jid":"31699999999@s.whatsapp.net","emoji":"👍"}`},
	} {
		rec := post(tc.path, tc.body)
		if rec.Code != http.StatusForbidden || !strings.Contains(rec.Body.String(), "WHATSAPP_SEND_ALLOWLIST") {
			t.Errorf("%s %s: got HTTP %d %s, want 403", tc.path, tc.body, rec.Code, rec.Body)
		}
	}
}
