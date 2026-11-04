package main

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"testing"
	"time"

	"go.mau.fi/whatsmeow"
	"go.mau.fi/whatsmeow/types"
	"go.mau.fi/whatsmeow/types/events"
)

func TestIsExpiredMediaError(t *testing.T) {
	httpErr := func(code int) error {
		return fmt.Errorf("download: %w", whatsmeow.DownloadHTTPError{Response: &http.Response{StatusCode: code}})
	}
	for code, want := range map[int]bool{403: true, 404: true, 410: true, 500: false, 401: false} {
		if got := isExpiredMediaError(httpErr(code)); got != want {
			t.Errorf("HTTP %d: got %v want %v", code, got, want)
		}
	}
	if isExpiredMediaError(errors.New("hash mismatch")) {
		t.Error("a non-HTTP error must not trigger a retry")
	}
}

func TestMediaRetryMessageInfo(t *testing.T) {
	s := newTestStore(t)
	ctx := context.Background()
	group := "120363000000000000@g.us"
	dm := "31612345678@s.whatsapp.net"
	now := time.Now()
	s.StoreChat(group, "Book club", now)
	s.StoreChat(dm, "Maria", now)
	s.StoreMessage("G1", group, "123456789012345", "", now, false, "image", "a.jpg", "u", []byte{1}, nil, nil, 1, "")
	s.StoreMessage("D1", dm, "31600000000", "", now, true, "image", "b.jpg", "u", []byte{1}, nil, nil, 1, "")
	own := types.NewJID("31600000000", types.DefaultUserServer)
	own.Device = 3
	lids := fakeLIDs{pnForLID: map[string]string{"123456789012345": "31612345678"}}

	info, err := mediaRetryMessageInfo(ctx, s, lids, own, "G1", group)
	if err != nil {
		t.Fatal(err)
	}
	if !info.IsGroup || info.IsFromMe || info.Sender.String() != "123456789012345@lid" || info.ID != "G1" {
		t.Errorf("group message: %+v", info.MessageSource)
	}

	info, err = mediaRetryMessageInfo(ctx, s, lids, own, "D1", dm)
	if err != nil {
		t.Fatal(err)
	}
	if info.IsGroup || !info.IsFromMe || info.Sender.String() != "31600000000@s.whatsapp.net" {
		t.Errorf("own direct message: %+v", info.MessageSource)
	}

	if _, err := mediaRetryMessageInfo(ctx, s, lids, own, "nope", dm); err == nil {
		t.Error("unknown message: want an error")
	}
}

func TestHandleMediaRetryRoutesToWaiter(t *testing.T) {
	chat := types.NewJID("120363000000000000", types.GroupServer)
	key := mediaRetryKey(chat.String(), "M1")
	ch := make(chan *events.MediaRetry, 1)
	mediaRetryWaiters.Lock()
	mediaRetryWaiters.m[key] = ch
	mediaRetryWaiters.Unlock()
	defer func() {
		mediaRetryWaiters.Lock()
		delete(mediaRetryWaiters.m, key)
		mediaRetryWaiters.Unlock()
	}()

	handleMediaRetry(&events.MediaRetry{ChatID: chat, MessageID: "OTHER"}) // not waited for: ignored
	handleMediaRetry(&events.MediaRetry{ChatID: chat, MessageID: "M1"})
	select {
	case evt := <-ch:
		if evt.MessageID != "M1" {
			t.Errorf("got %s", evt.MessageID)
		}
	default:
		t.Fatal("event not delivered")
	}
}
