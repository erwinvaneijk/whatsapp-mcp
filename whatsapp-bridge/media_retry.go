package main

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"sync"
	"time"

	"go.mau.fi/whatsmeow"
	"go.mau.fi/whatsmeow/proto/waMmsRetry"
	"go.mau.fi/whatsmeow/store"
	"go.mau.fi/whatsmeow/types"
	"go.mau.fi/whatsmeow/types/events"
)

// Media of older messages (history sync, forwards) expires on WhatsApp's
// servers. The media-retry protocol asks the sender's phone to upload it
// again; the phone answers with a MediaRetry event that carries a new path.
//
// Based on upstream PR #273 (commit 1cc8e86).

const mediaRetryTimeout = 45 * time.Second

var mediaRetryWaiters = struct {
	sync.Mutex
	m map[string]chan *events.MediaRetry
}{m: map[string]chan *events.MediaRetry{}}

func mediaRetryKey(chatJID, messageID string) string { return chatJID + "/" + messageID }

// handleMediaRetry passes a MediaRetry event to the download waiting for it.
func handleMediaRetry(evt *events.MediaRetry) {
	key := mediaRetryKey(evt.ChatID.String(), evt.MessageID)
	mediaRetryWaiters.Lock()
	ch, ok := mediaRetryWaiters.m[key]
	mediaRetryWaiters.Unlock()
	if ok {
		select {
		case ch <- evt:
		default:
		}
	}
}

// isExpiredMediaError reports whether a download failed because the media is
// no longer on WhatsApp's servers, which a media retry can fix.
func isExpiredMediaError(err error) bool {
	var httpErr whatsmeow.DownloadHTTPError
	if !errors.As(err, &httpErr) || httpErr.Response == nil {
		return false
	}
	switch httpErr.StatusCode {
	case http.StatusForbidden, http.StatusNotFound, http.StatusGone:
		return true
	}
	return false
}

// mediaRetryMessageInfo builds the message details the retry request needs.
// For group messages WhatsApp also needs the original sender; IsGroup must be
// set for whatsmeow to send it.
func mediaRetryMessageInfo(ctx context.Context, messageStore *MessageStore, lids store.LIDStore, ownJID types.JID,
	messageID, chatJID string) (*types.MessageInfo, error) {
	chat, err := types.ParseJID(chatJID)
	if err != nil {
		return nil, fmt.Errorf("invalid chat JID: %v", err)
	}
	var sender string
	var isFromMe bool
	if err := messageStore.db.QueryRow(
		`SELECT sender, is_from_me FROM messages WHERE id = ? AND chat_jid = ?`, messageID, chatJID,
	).Scan(&sender, &isFromMe); err != nil {
		return nil, fmt.Errorf("failed to look up message sender: %v", err)
	}
	senderJID := ownJID.ToNonAD()
	if !isFromMe {
		if senderJID, err = resolveStoredSender(ctx, lids, sender); err != nil {
			return nil, fmt.Errorf("invalid sender: %v", err)
		}
	}
	return &types.MessageInfo{
		MessageSource: types.MessageSource{
			Chat:     chat,
			Sender:   senderJID,
			IsFromMe: isFromMe,
			IsGroup:  chat.Server == types.GroupServer,
		},
		ID: messageID,
	}, nil
}

// downloadViaMediaRetry asks the sender's phone to upload the media again and
// downloads it from the new path. The phone must be online.
func downloadViaMediaRetry(ctx context.Context, client *whatsmeow.Client, messageStore *MessageStore,
	messageID, chatJID string, downloader *MediaDownloader) ([]byte, error) {
	if client.Store == nil || client.Store.ID == nil {
		return nil, errors.New("not logged in")
	}
	info, err := mediaRetryMessageInfo(ctx, messageStore, client.Store.LIDs, *client.Store.ID, messageID, chatJID)
	if err != nil {
		return nil, err
	}

	key := mediaRetryKey(info.Chat.String(), messageID)
	ch := make(chan *events.MediaRetry, 1)
	mediaRetryWaiters.Lock()
	mediaRetryWaiters.m[key] = ch
	mediaRetryWaiters.Unlock()
	defer func() {
		mediaRetryWaiters.Lock()
		delete(mediaRetryWaiters.m, key)
		mediaRetryWaiters.Unlock()
	}()

	if err := client.SendMediaRetryReceipt(ctx, info, downloader.MediaKey); err != nil {
		return nil, fmt.Errorf("failed to request media retry: %v", err)
	}

	select {
	case evt := <-ch:
		notif, err := whatsmeow.DecryptMediaRetryNotification(evt, downloader.MediaKey)
		if err != nil {
			return nil, fmt.Errorf("media retry failed: %v", err)
		}
		if notif.GetResult() != waMmsRetry.MediaRetryNotification_SUCCESS {
			return nil, fmt.Errorf("the sender's phone could not provide the media: %s", notif.GetResult())
		}
		if notif.GetDirectPath() == "" {
			return nil, errors.New("media retry returned an empty path")
		}
		retry := *downloader
		retry.URL = ""
		retry.DirectPath = notif.GetDirectPath()
		return client.Download(ctx, &retry)
	case <-time.After(mediaRetryTimeout):
		return nil, errors.New("timed out waiting for the sender's phone to upload the media again (it must be online)")
	case <-ctx.Done():
		return nil, ctx.Err()
	}
}
