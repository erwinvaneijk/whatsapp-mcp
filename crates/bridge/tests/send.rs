//! Sending and media, without a connection. Ported from the Go bridge's
//! send_context_test.go, outgoing_test.go, reaction_test.go (stored
//! senders), media_retry_test.go (the message to ask again for),
//! media_url_test.go and media_download_test.go; the Ogg analysis, media
//! messages and download names are new.

use std::path::Path;

use chrono::{DateTime, FixedOffset, Local, TimeZone};
use whatsapp_mcp_bridge::Jid;
use whatsapp_mcp_bridge::send::{
    Kind, SenderError, Uploaded, Voice, analyze_ogg_opus, attach_context_info, context_info,
    direct_path_from_url, download_name, media_kind, media_message, mention_jids, original_sender,
    outgoing_chat_jid, placeholder_waveform, safe_chat_dir, safe_name, sent_message, stored_sender,
};
use whatsapp_mcp_store::{NewMessage, Writer};
use whatsapp_rust::prelude::wa;
use whatsapp_rust::prelude::wa::message::{DocumentMessage, ImageMessage, ReactionMessage};

macro_rules! make {
    ($t:ty { $($f:ident : $v:expr),* $(,)? }) => {{
        let mut value = <$t>::default();
        $( value.$f = $v; )*
        value
    }};
}

fn s(text: &str) -> Option<String> {
    Some(text.to_string())
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|i| i.to_string()).collect()
}

fn now() -> DateTime<Local> {
    Local.with_ymd_and_hms(2026, 11, 4, 22, 0, 52).unwrap()
}

fn earlier() -> DateTime<FixedOffset> {
    (now() - chrono::Duration::hours(1)).fixed_offset()
}

fn store() -> (tempfile::TempDir, Writer) {
    let dir = tempfile::tempdir().unwrap();
    let writer = Writer::open(dir.path()).unwrap();
    (dir, writer)
}

/// One stored row: chat, sender, content, from me, media type, file name,
/// quoted ID.
fn row(
    writer: &Writer,
    id: &str,
) -> (String, String, String, bool, String, String, Option<String>) {
    writer
        .connection()
        .query_row(
            "SELECT chat_jid, sender, content, is_from_me, media_type, filename, quoted_message_id FROM messages WHERE id = ?",
            [id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get::<_, Option<String>>(4)?.unwrap_or_default(),
                    r.get::<_, Option<String>>(5)?.unwrap_or_default(),
                    r.get(6)?,
                ))
            },
        )
        .unwrap()
}

fn store_sent(writer: &Writer, id: &str, chat: &Jid, msg: &wa::Message) {
    let chat_jid = outgoing_chat_jid(writer, chat).unwrap();
    writer
        .store_chat(&chat_jid, "", now().fixed_offset())
        .unwrap();
    writer
        .store_message(&sent_message(id, &chat_jid, "31600000000", msg, now()))
        .unwrap();
}

// send_context_test.go

#[test]
fn mentions_become_jids() {
    assert_eq!(
        mention_jids(&strings(&[
            "31612345678",
            " @31687654321 ",
            "",
            "123@lid",
            "456@s.whatsapp.net"
        ])),
        strings(&[
            "31612345678@s.whatsapp.net",
            "31687654321@s.whatsapp.net",
            "123@lid",
            "456@s.whatsapp.net"
        ])
    );
}

#[test]
fn context_for_replies_and_mentions() {
    assert_eq!(context_info("", "", &[]), None);
    assert_eq!(context_info("", "", &strings(&[" ", "@"])), None);

    let c = context_info("MSG1", "111@lid", &strings(&["222"])).unwrap();
    assert_eq!(c.stanza_id.as_deref(), Some("MSG1"));
    assert_eq!(c.participant.as_deref(), Some("111@lid"));
    assert_eq!(c.mentioned_jid, strings(&["222@s.whatsapp.net"]));

    // A mention alone must not look like a reply.
    let c = context_info("", "", &strings(&["222"])).unwrap();
    assert_eq!((c.stanza_id, c.participant), (None, None));
}

#[test]
fn context_goes_on_the_message() {
    let ctx = context_info("MSG1", "", &strings(&["222"]));

    let mut text = make!(wa::Message {
        conversation: s("hi @222")
    });
    attach_context_info(&mut text, "hi @222", ctx.clone());
    assert_eq!(text.conversation, None);
    let extended = text.extended_text_message.as_option().unwrap();
    assert_eq!(extended.text.as_deref(), Some("hi @222"));
    assert_eq!(extended.context_info.as_option(), ctx.as_ref());

    let mut image = make!(wa::Message {
        image_message: make!(ImageMessage {
            caption: s("look @222")
        })
        .into()
    });
    attach_context_info(&mut image, "look @222", ctx.clone());
    assert_eq!(
        image
            .image_message
            .as_option()
            .unwrap()
            .context_info
            .as_option(),
        ctx.as_ref()
    );
    assert!(image.extended_text_message.as_option().is_none());

    let mut plain = make!(wa::Message {
        conversation: s("hi")
    });
    attach_context_info(&mut plain, "hi", None);
    assert_eq!(plain.conversation.as_deref(), Some("hi"));
    assert!(plain.extended_text_message.as_option().is_none());
}

// outgoing_test.go

#[test]
fn sent_messages_go_into_the_existing_chat() {
    let (_dir, writer) = store();
    writer
        .store_lid_mapping("123456789012345", "31612345678")
        .unwrap();
    let pn = Jid::parse("31612345678@s.whatsapp.net").unwrap();
    let lid = Jid::parse("123456789012345@lid").unwrap();

    // No chat yet: the recipient as given.
    assert_eq!(outgoing_chat_jid(&writer, &pn).unwrap(), pn.to_string());
    // Received messages are stored under the LID: use that chat.
    writer
        .store_chat(&lid.to_string(), "Maria", earlier())
        .unwrap();
    assert_eq!(outgoing_chat_jid(&writer, &pn).unwrap(), lid.to_string());
    // And its own chat when that exists.
    writer
        .store_chat(&pn.to_string(), "Maria", earlier())
        .unwrap();
    assert_eq!(outgoing_chat_jid(&writer, &lid).unwrap(), lid.to_string());
    // A device JID is stored under the user.
    let device = Jid::parse("31612345678:3@s.whatsapp.net").unwrap();
    assert_eq!(outgoing_chat_jid(&writer, &device).unwrap(), pn.to_string());
    // Groups are used as they are.
    let group = Jid::parse("120363000000000000@g.us").unwrap();
    assert_eq!(
        outgoing_chat_jid(&writer, &group).unwrap(),
        group.to_string()
    );
}

#[test]
fn sent_messages_are_stored_like_received_ones() {
    let (_dir, writer) = store();
    let group = Jid::parse("120363000000000000@g.us").unwrap();
    writer
        .store_chat(&group.to_string(), "Book club", earlier())
        .unwrap();

    // A reply with a mention, as the bridge builds it.
    let mut text = make!(wa::Message {
        conversation: s("Ja @31612345678")
    });
    attach_context_info(
        &mut text,
        "Ja @31612345678",
        context_info("ORIG1", "1@lid", &strings(&["31612345678"])),
    );
    store_sent(&writer, "OUT1", &group, &text);
    assert_eq!(
        row(&writer, "OUT1"),
        (
            group.to_string(),
            "31600000000".into(),
            "Ja @31612345678".into(),
            true,
            String::new(),
            String::new(),
            Some("ORIG1".into())
        )
    );

    // A document with a caption.
    let doc = make!(wa::Message {
        document_message: make!(DocumentMessage {
            file_name: s("menu.pdf"),
            caption: s("the menu"),
            url: s("https://mmg.whatsapp.net/x"),
        })
        .into()
    });
    store_sent(&writer, "OUT2", &group, &doc);
    let (_, _, content, _, media_type, filename, _) = row(&writer, "OUT2");
    assert_eq!(
        (content.as_str(), media_type.as_str(), filename.as_str()),
        ("the menu", "document", "menu.pdf")
    );

    // A reaction, stored like a received one.
    let react = make!(wa::Message {
        reaction_message: make!(ReactionMessage { text: s("👍") }).into()
    });
    store_sent(&writer, "OUT3", &group, &react);
    assert_eq!(row(&writer, "OUT3").2, "[reaction: 👍]");

    // Storing the chat without a name keeps the known name.
    assert_eq!(
        writer.chat_name(&group.to_string()).unwrap().as_deref(),
        Some("Book club")
    );
}

// reaction_test.go and media_retry_test.go

#[test]
fn stored_senders_become_jids() {
    let (_dir, writer) = store();
    writer
        .store_lid_mapping("123456789012345", "31612345678")
        .unwrap();
    for (stored, want) in [
        ("31612345678", "31612345678@s.whatsapp.net"), // bare phone number
        ("123456789012345", "123456789012345@lid"),    // bare LID in lid_map
        ("999@lid", "999@lid"),                        // full JID from history sync
        ("31687654321@s.whatsapp.net", "31687654321@s.whatsapp.net"),
        (" 31600000000 ", "31600000000@s.whatsapp.net"),
    ] {
        assert_eq!(
            stored_sender(&writer, stored).unwrap().to_string(),
            want,
            "{stored:?}"
        );
    }
    assert!(stored_sender(&writer, "").is_err());
}

#[test]
fn the_original_sender_of_a_stored_message() {
    let (_dir, writer) = store();
    writer
        .store_lid_mapping("123456789012345", "31612345678")
        .unwrap();
    let group = "120363000000000000@g.us";
    let dm = "31612345678@s.whatsapp.net";
    let photo = |id: &str, chat: &str, sender: &str, from_me: bool| NewMessage {
        id: id.into(),
        chat_jid: chat.into(),
        sender: sender.into(),
        timestamp: Some(now().fixed_offset()),
        is_from_me: from_me,
        media_type: "image".into(),
        filename: "a.jpg".into(),
        ..Default::default()
    };
    writer.store_chat(group, "Book club", earlier()).unwrap();
    writer.store_chat(dm, "Maria", earlier()).unwrap();
    writer
        .store_message(&photo("G1", group, "123456789012345", false))
        .unwrap();
    writer
        .store_message(&photo("D1", dm, "31600000000", true))
        .unwrap();

    assert_eq!(
        original_sender(&writer, "G1", group).unwrap(),
        Some(Jid::parse("123456789012345@lid").unwrap())
    );
    assert_eq!(original_sender(&writer, "D1", dm).unwrap(), None, "mine");
    assert_eq!(
        original_sender(&writer, "nope", dm),
        Err(SenderError::NotFound)
    );
    assert_eq!(
        original_sender(&writer, "G1", dm),
        Err(SenderError::NotFound),
        "another chat"
    );
}

// media_url_test.go and media_download_test.go

#[test]
fn direct_paths_keep_the_query_byte_for_byte() {
    for (url, want) in [
        (
            "https://mmg.whatsapp.net/v/t62/file.enc?ccb=11-4&oh=test-signature&oe=12345678",
            "/v/t62/file.enc?ccb=11-4&oh=test-signature&oe=12345678",
        ),
        (
            "https://media.whatsapp.net/v/file.enc?oh=a%2Fb%2Bc&oe=12345678&token=a+b&token=second",
            "/v/file.enc?oh=a%2Fb%2Bc&oe=12345678&token=a+b&token=second",
        ),
        (
            "https://mmg.whatsapp.net/v/t62.7117-24/100000000_1000000000000000_1000000000000000000_n.enc?ccb=11-4&oh=01_SyntheticFixtureOhTokenNotARealSignatureAAAA&oe=00000000&_nc_sid=000000&mms3=true",
            "/v/t62.7117-24/100000000_1000000000000000_1000000000000000000_n.enc?ccb=11-4&oh=01_SyntheticFixtureOhTokenNotARealSignatureAAAA&oe=00000000&_nc_sid=000000&mms3=true",
        ),
        (
            "https://mmg.whatsapp.net/o1/v/t24/f2/m235/BBBBBBBB?ccb=9-4&oh=01_SyntheticFixtureOhTokenNotARealSignatureBBBB&oe=00000000&_nc_sid=000000&mms3=true",
            "/o1/v/t24/f2/m235/BBBBBBBB?ccb=9-4&oh=01_SyntheticFixtureOhTokenNotARealSignatureBBBB&oe=00000000&_nc_sid=000000&mms3=true",
        ),
        (
            "https://mmg.whatsapp.net/v/t62.7117-24/foo.enc?ccb=11-4&oh=ZZZ&oe=00000000&_nc_sid=000000&mms3=true",
            "/v/t62.7117-24/foo.enc?ccb=11-4&oh=ZZZ&oe=00000000&_nc_sid=000000&mms3=true",
        ),
        ("https://mmg.whatsapp.net/v/file.enc", "/v/file.enc"),
        // Already a direct path.
        (
            "/v/file.enc?oh=test-signature&oe=12345678",
            "/v/file.enc?oh=test-signature&oe=12345678",
        ),
        // Any host, not only *.whatsapp.net.
        (
            "https://example.com/file.enc?oh=test-signature",
            "/file.enc?oh=test-signature",
        ),
        // Not a URL: returned as it is.
        ("://bad url", "://bad url"),
        ("://not a url at all", "://not a url at all"),
    ] {
        assert_eq!(direct_path_from_url(url), want, "{url}");
    }
}

// New: media messages, voice notes, download names.

#[test]
fn media_kinds_from_the_extension() {
    for (path, kind, mime) in [
        ("/m/photo.JPG", Kind::Image, "image/jpeg"),
        ("/m/a.png", Kind::Image, "image/png"),
        ("/m/a.gif", Kind::Image, "image/gif"),
        ("/m/a.webp", Kind::Image, "image/webp"),
        ("/m/voice.ogg", Kind::Audio, "audio/ogg; codecs=opus"),
        ("/m/a.mp4", Kind::Video, "video/mp4"),
        ("/m/a.avi", Kind::Video, "video/avi"),
        ("/m/a.mov", Kind::Video, "video/quicktime"),
        ("/m/menu.pdf", Kind::Document, "application/pdf"),
        ("/m/notes.txt", Kind::Document, "text/plain"),
        (
            "/m/no-extension",
            Kind::Document,
            "application/octet-stream",
        ),
        (
            "/m/a.unknownext",
            Kind::Document,
            "application/octet-stream",
        ),
    ] {
        assert_eq!(
            media_kind(Path::new(path)),
            (kind, mime.to_string()),
            "{path}"
        );
    }
}

fn uploaded() -> Uploaded {
    Uploaded {
        url: "https://mmg.whatsapp.net/v/x.enc?oh=1".into(),
        direct_path: "/v/x.enc?oh=1".into(),
        media_key: vec![1; 32],
        file_sha256: vec![2; 32],
        file_enc_sha256: vec![3; 32],
        file_length: 1234,
        media_key_timestamp: 1_793_829_652,
        streaming_sidecar: Some(vec![4; 10]),
    }
}

#[test]
fn media_messages_carry_the_upload() {
    let image = media_message(Kind::Image, "image/png", &uploaded(), "look", "a.png", None);
    let part = image.image_message.as_option().unwrap();
    assert_eq!(part.caption.as_deref(), Some("look"));
    assert_eq!(part.mimetype.as_deref(), Some("image/png"));
    assert_eq!(
        part.url.as_deref(),
        Some("https://mmg.whatsapp.net/v/x.enc?oh=1")
    );
    assert_eq!(part.direct_path.as_deref(), Some("/v/x.enc?oh=1"));
    assert_eq!(part.media_key.as_deref(), Some(&[1u8; 32][..]));
    assert_eq!(part.file_sha256.as_deref(), Some(&[2u8; 32][..]));
    assert_eq!(part.file_enc_sha256.as_deref(), Some(&[3u8; 32][..]));
    assert_eq!(part.file_length, Some(1234));

    let doc = media_message(
        Kind::Document,
        "application/pdf",
        &uploaded(),
        "the menu",
        "menu.pdf",
        None,
    );
    let part = doc.document_message.as_option().unwrap();
    assert_eq!(part.file_name.as_deref(), Some("menu.pdf"));
    assert_eq!(part.title.as_deref(), Some("menu.pdf"));
    assert_eq!(part.caption.as_deref(), Some("the menu"));
    assert_eq!(part.mimetype.as_deref(), Some("application/pdf"));

    let voice = Voice {
        seconds: 7,
        waveform: placeholder_waveform(7),
    };
    let audio = media_message(
        Kind::Audio,
        "audio/ogg; codecs=opus",
        &uploaded(),
        "",
        "voice.ogg",
        Some(voice.clone()),
    );
    let part = audio.audio_message.as_option().unwrap();
    assert_eq!(part.ptt, Some(true));
    assert_eq!(part.seconds, Some(7));
    assert_eq!(part.waveform.as_deref(), Some(voice.waveform.as_slice()));
    assert_eq!(part.streaming_sidecar.as_deref(), Some(&[4u8; 10][..]));

    let video = media_message(Kind::Video, "video/mp4", &uploaded(), "clip", "a.mp4", None);
    assert_eq!(
        video.video_message.as_option().unwrap().caption.as_deref(),
        Some("clip")
    );

    // Stored like a received message with the same media.
    let stored = sent_message("OUT", "c@g.us", "31600000000", &doc, now());
    assert_eq!(
        (
            stored.media_type.as_str(),
            stored.filename.as_str(),
            stored.file_length
        ),
        ("document", "menu.pdf", 1234)
    );
    assert_eq!(stored.url, "https://mmg.whatsapp.net/v/x.enc?oh=1");
}

/// One Ogg page: header, segment table, packet.
fn ogg_page(sequence: u32, granule: u64, packet: &[u8]) -> Vec<u8> {
    let mut page = b"OggS".to_vec();
    page.push(0); // version
    page.push(0); // header type
    page.extend(granule.to_le_bytes());
    page.extend(1u32.to_le_bytes()); // serial
    page.extend(sequence.to_le_bytes());
    page.extend(0u32.to_le_bytes()); // checksum, not checked
    let segments: Vec<u8> = packet
        .chunks(255)
        .map(|c| u8::try_from(c.len()).unwrap())
        .collect();
    page.push(u8::try_from(segments.len()).unwrap());
    page.extend(&segments);
    page.extend(packet);
    page
}

/// An Ogg Opus file whose last granule position is `granule`.
fn ogg_opus(pre_skip: u16, granules: &[u64]) -> Vec<u8> {
    let mut head = b"OpusHead".to_vec();
    head.push(1); // version
    head.push(1); // channels
    head.extend(pre_skip.to_le_bytes());
    head.extend(16_000u32.to_le_bytes()); // input rate: informational only
    head.extend(0u16.to_le_bytes()); // gain
    head.push(0); // mapping family
    let mut file = ogg_page(0, 0, &head);
    file.extend(ogg_page(1, 0, b"OpusTags\0\0\0\0\0\0\0\0"));
    for (i, &granule) in granules.iter().enumerate() {
        file.extend(ogg_page(2 + i as u32, granule, &[0xAB; 40]));
    }
    file
}

#[test]
fn voice_note_length_from_the_ogg_file() {
    // 5 s at 48 kHz after a pre-skip of 312 samples; the 16 kHz input rate
    // in the header does not count.
    let five = analyze_ogg_opus(&ogg_opus(312, &[48_000 + 312, 5 * 48_000 + 312])).unwrap();
    assert_eq!(five.seconds, 5);
    assert_eq!(five.waveform, placeholder_waveform(5));
    // Part of a second counts as a second.
    let more = analyze_ogg_opus(&ogg_opus(312, &[5 * 48_000 + 313])).unwrap();
    assert_eq!(more.seconds, 6);
    // At least 1 and at most 300 seconds.
    assert_eq!(analyze_ogg_opus(&ogg_opus(312, &[100])).unwrap().seconds, 1);
    assert_eq!(
        analyze_ogg_opus(&ogg_opus(0, &[3600 * 48_000]))
            .unwrap()
            .seconds,
        300
    );
    // Without a granule position, an estimate from the size.
    assert_eq!(analyze_ogg_opus(&ogg_opus(0, &[])).unwrap().seconds, 1);
    // A page claiming more than the file holds ends the scan, no panic.
    let mut cut = ogg_opus(312, &[2 * 48_000 + 312]);
    cut.truncate(cut.len() - 5);
    assert_eq!(analyze_ogg_opus(&cut).unwrap().seconds, 1);

    assert_eq!(
        analyze_ogg_opus(b"RIFF....WAVE"),
        Err("not a valid Ogg file (missing OggS signature)".to_string())
    );
    assert!(analyze_ogg_opus(b"Og").is_err());
}

#[test]
fn waveforms_are_64_values_up_to_100_and_repeatable() {
    for seconds in [1, 5, 60, 300] {
        let waveform = placeholder_waveform(seconds);
        assert_eq!(waveform.len(), 64);
        assert!(waveform.iter().all(|&v| v <= 100));
        assert_eq!(waveform, placeholder_waveform(seconds));
    }
    assert_ne!(placeholder_waveform(5), placeholder_waveform(6));
}

#[test]
fn download_names_are_safe_and_distinct() {
    assert_eq!(safe_name("photo.jpg"), "photo.jpg");
    assert_eq!(safe_name("../../etc/passwd"), "passwd");
    assert_eq!(safe_name("..\\..\\x.exe"), "x.exe");
    assert_eq!(safe_name("dir/"), "dir");
    assert_eq!(safe_name("a\u{1b}[31m.txt"), "a[31m.txt");
    for bad in ["", ".", "..", "/", "\u{7}"] {
        assert_eq!(safe_name(bad), "file", "{bad:?}");
    }

    assert_eq!(
        safe_chat_dir("31612345678@s.whatsapp.net"),
        "31612345678@s.whatsapp.net"
    );
    assert_eq!(safe_chat_dir("../x/y@g.us"), ".._x_y@g.us");
    assert_eq!(safe_chat_dir(".."), "_");
    assert_eq!(safe_chat_dir(""), "_");

    let (_dir, writer) = store();
    let chat = "120363000000000000@g.us";
    writer.store_chat(chat, "Book club", earlier()).unwrap();
    let photo = |id: &str| NewMessage {
        id: id.into(),
        chat_jid: chat.into(),
        sender: "1".into(),
        timestamp: Some(now().fixed_offset()),
        media_type: "image".into(),
        filename: "image_20261104_220052.jpg".into(),
        ..Default::default()
    };
    writer.store_message(&photo("A")).unwrap();
    assert_eq!(
        download_name(&writer, chat, "image_20261104_220052.jpg", "A").unwrap(),
        "image_20261104_220052.jpg"
    );
    // Two photos received in the same second: each gets its own file.
    writer.store_message(&photo("B")).unwrap();
    assert_eq!(
        download_name(&writer, chat, "image_20261104_220052.jpg", "B").unwrap(),
        "image_20261104_220052_B.jpg"
    );
    // The message ID comes from the sender too.
    writer
        .store_message(&NewMessage {
            filename: "notes".into(),
            ..photo("../C")
        })
        .unwrap();
    writer
        .store_message(&NewMessage {
            filename: "notes".into(),
            ..photo("D")
        })
        .unwrap();
    assert_eq!(download_name(&writer, chat, "notes", "../C").unwrap(), "C");
    assert_eq!(
        download_name(&writer, chat, "notes", "D").unwrap(),
        "notes_D"
    );
}
