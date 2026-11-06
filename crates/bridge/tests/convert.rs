//! Incoming messages to stored text, quoted IDs and media. Ported case for
//! case from the Go bridge's main_test.go, caption_test.go and
//! quoted_test.go, so the stored text stays the same.

use chrono::TimeZone;
use whatsapp_mcp_bridge::convert::{media, quoted_message_id, text_content};
use whatsapp_rust::prelude::wa;
use whatsapp_rust::prelude::wa::message::protocol_message::Type as ProtocolType;
use whatsapp_rust::prelude::wa::message::{
    AudioMessage, ContactMessage, ContactsArrayMessage, DocumentMessage, ExtendedTextMessage,
    FutureProofMessage, ImageMessage, LiveLocationMessage, LocationMessage, PollCreationMessage,
    PollUpdateMessage, ProtocolMessage, ReactionMessage, StickerMessage, VideoMessage,
};

/// Builds a protobuf message: the generated types are non-exhaustive, so
/// they start from Default and get their fields set.
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

fn wrap(inner: wa::Message) -> FutureProofMessage {
    make!(FutureProofMessage {
        message: inner.into(),
    })
}

fn conversation(text: &str) -> wa::Message {
    make!(wa::Message {
        conversation: s(text),
    })
}

fn poll(name: &str, options: &[&str]) -> PollCreationMessage {
    make!(PollCreationMessage {
        name: s(name),
        options: options
            .iter()
            .map(|o| make!(wa::message::poll_creation_message::Option { option_name: s(o) }))
            .collect(),
    })
}

fn protocol(kind: ProtocolType, edited: Option<wa::Message>) -> wa::Message {
    make!(wa::Message {
        protocol_message: make!(ProtocolMessage {
            r#type: Some(kind),
            edited_message: edited.map(Into::into).unwrap_or_default(),
        })
        .into(),
    })
}

/// From main_test.go: TestExtractTextContentNonText.
#[test]
fn text_of_each_kind_of_message() {
    let cases: Vec<(&str, wa::Message, &str)> = vec![
        ("empty message", wa::Message::default(), ""),
        ("plain conversation", conversation("hello"), "hello"),
        (
            "extended text",
            make!(wa::Message {
                extended_text_message: make!(ExtendedTextMessage {
                    text: s("quoted reply"),
                })
                .into(),
            }),
            "quoted reply",
        ),
        (
            "reaction",
            make!(wa::Message {
                reaction_message: make!(ReactionMessage { text: s("❤️") }).into(),
            }),
            "[reaction: ❤️]",
        ),
        (
            "reaction removed",
            make!(wa::Message {
                reaction_message: make!(ReactionMessage { text: s("") }).into(),
            }),
            "[reaction removed]",
        ),
        (
            "poll with options",
            make!(wa::Message {
                poll_creation_message: poll("Lunch?", &["Pizza", "Sushi"]).into(),
            }),
            "[poll: Lunch? — options: Pizza | Sushi]",
        ),
        (
            "poll without options",
            make!(wa::Message {
                poll_creation_message: poll("Yes or no", &[]).into(),
            }),
            "[poll: Yes or no]",
        ),
        (
            "poll vote",
            make!(wa::Message {
                poll_update_message: PollUpdateMessage::default().into(),
            }),
            "[poll vote]",
        ),
        (
            "location named",
            make!(wa::Message {
                location_message: make!(LocationMessage {
                    name: s("Tel Aviv"),
                    degrees_latitude: Some(32.0853),
                    degrees_longitude: Some(34.7818),
                })
                .into(),
            }),
            "[location: Tel Aviv (32.085300, 34.781800)]",
        ),
        (
            "location unnamed",
            make!(wa::Message {
                location_message: make!(LocationMessage {
                    degrees_latitude: Some(1.5),
                    degrees_longitude: Some(2.5),
                })
                .into(),
            }),
            "[location: 1.500000, 2.500000]",
        ),
        (
            "live location with caption",
            make!(wa::Message {
                live_location_message: make!(LiveLocationMessage {
                    caption: s("on my way"),
                    degrees_latitude: Some(3.0),
                    degrees_longitude: Some(4.0),
                })
                .into(),
            }),
            "[live location: on my way (3.000000, 4.000000)]",
        ),
        (
            "contact card",
            make!(wa::Message {
                contact_message: make!(ContactMessage {
                    display_name: s("Alice"),
                })
                .into(),
            }),
            "[contact: Alice]",
        ),
        (
            "contacts array",
            make!(wa::Message {
                contacts_array_message: make!(ContactsArrayMessage {
                    contacts: ["Alice", "Bob"]
                        .iter()
                        .map(|n| make!(ContactMessage { display_name: s(n) }))
                        .collect(),
                })
                .into(),
            }),
            "[contacts: Alice, Bob]",
        ),
        (
            "edit recurses into corrected text",
            protocol(
                ProtocolType::MESSAGE_EDIT,
                Some(conversation("corrected text")),
            ),
            "[edited] corrected text",
        ),
        (
            "edit with no inner text",
            protocol(ProtocolType::MESSAGE_EDIT, None),
            "[edited message]",
        ),
        (
            "revoke",
            protocol(ProtocolType::REVOKE, None),
            "[message deleted]",
        ),
        (
            "view once is unwrapped",
            make!(wa::Message {
                view_once_message: wrap(conversation("secret")).into(),
            }),
            "secret",
        ),
        (
            "view once v2",
            make!(wa::Message {
                view_once_message_v2: wrap(conversation("secret2")).into(),
            }),
            "secret2",
        ),
        (
            "ephemeral unwraps without prefix",
            make!(wa::Message {
                ephemeral_message: wrap(conversation("disappearing")).into(),
            }),
            "disappearing",
        ),
        (
            "sticker",
            make!(wa::Message {
                sticker_message: StickerMessage::default().into(),
            }),
            "[sticker]",
        ),
    ];
    for (name, msg, want) in cases {
        assert_eq!(text_content(&msg), want, "{name}");
    }
}

/// From main_test.go: TestPollVariants.
#[test]
fn every_poll_version() {
    let want = "[poll: Lunch? — options: Pizza]";
    let p = || poll("Lunch?", &["Pizza"]);
    let cases = [
        (
            "V1",
            make!(wa::Message {
                poll_creation_message: p().into(),
            }),
        ),
        (
            "V2",
            make!(wa::Message {
                poll_creation_message_v2: p().into(),
            }),
        ),
        (
            "V3",
            make!(wa::Message {
                poll_creation_message_v3: p().into(),
            }),
        ),
        (
            "V4",
            make!(wa::Message {
                poll_creation_message_v4: wrap(make!(wa::Message {
                    poll_creation_message: p().into(),
                }))
                .into(),
            }),
        ),
        (
            "V5",
            make!(wa::Message {
                poll_creation_message_v5: p().into(),
            }),
        ),
        (
            "V6",
            make!(wa::Message {
                poll_creation_message_v6: p().into(),
            }),
        ),
    ];
    for (name, msg) in cases {
        assert_eq!(text_content(&msg), want, "{name}");
    }
}

/// From main_test.go: TestTopLevelEditedMessage.
#[test]
fn top_level_edited_message() {
    let msg = make!(wa::Message {
        edited_message: wrap(conversation("corrected")).into(),
    });
    assert_eq!(text_content(&msg), "[edited] corrected");
}

/// From caption_test.go: TestExtractTextContent.
#[test]
fn captions_are_text() {
    let image = |caption: Option<String>| {
        make!(wa::Message {
            image_message: make!(ImageMessage { caption: caption }).into(),
        })
    };
    let document = |caption: &str| {
        make!(DocumentMessage {
            caption: s(caption),
        })
    };
    let cases: Vec<(&str, wa::Message, &str)> = vec![
        (
            "image with caption",
            image(s("look at this")),
            "look at this",
        ),
        ("image without caption", image(None), ""),
        (
            "multiline caption is preserved",
            image(s("first\nsecond\n\nfourth")),
            "first\nsecond\n\nfourth",
        ),
        (
            "video with caption",
            make!(wa::Message {
                video_message: make!(VideoMessage { caption: s("clip") }).into(),
            }),
            "clip",
        ),
        (
            "document with caption",
            make!(wa::Message {
                document_message: document("the contract").into(),
            }),
            "the contract",
        ),
        (
            "document-with-caption envelope",
            make!(wa::Message {
                document_with_caption_message: wrap(make!(wa::Message {
                    document_message: document("the contract").into(),
                }))
                .into(),
            }),
            "the contract",
        ),
        (
            "nested envelopes: ephemeral around view-once",
            make!(wa::Message {
                ephemeral_message: wrap(make!(wa::Message {
                    view_once_message_v2: wrap(image(s("disappearing"))).into(),
                }))
                .into(),
            }),
            "disappearing",
        ),
        (
            "audio",
            make!(wa::Message {
                audio_message: AudioMessage::default().into(),
            }),
            "",
        ),
    ];
    for (name, msg, want) in cases {
        assert_eq!(text_content(&msg), want, "{name}");
    }
}

/// From caption_test.go: TestExtractMediaInfoUnwraps.
#[test]
fn enveloped_image_keeps_its_attachment() {
    let msg = make!(wa::Message {
        view_once_message_v2: wrap(make!(wa::Message {
            image_message: make!(ImageMessage {
                caption: s("x"),
                file_length: Some(213_903),
            })
            .into(),
        }))
        .into(),
    });
    let now = chrono::Local
        .with_ymd_and_hms(2026, 11, 4, 22, 0, 52)
        .unwrap();
    let m = media(&msg, now);
    assert_eq!(m.media_type, "image");
    assert_eq!(m.file_length, 213_903);
    assert_eq!(m.filename, "image_20261104_220052.jpg");
}

#[test]
fn media_file_names() {
    let now = chrono::Local
        .with_ymd_and_hms(2026, 11, 4, 22, 0, 52)
        .unwrap();
    let doc = |name: Option<String>| {
        make!(wa::Message {
            document_message: make!(DocumentMessage { file_name: name }).into(),
        })
    };
    assert_eq!(media(&doc(s("contract.pdf")), now).filename, "contract.pdf");
    assert_eq!(media(&doc(None), now).filename, "document_20261104_220052");
    let audio = make!(wa::Message {
        audio_message: AudioMessage::default().into(),
    });
    assert_eq!(media(&audio, now).filename, "audio_20261104_220052.ogg");
    assert_eq!(media(&conversation("hi"), now).media_type, "");
}

/// From quoted_test.go: TestExtractQuotedMessageID.
#[test]
fn quoted_message_ids() {
    let ctx = || {
        make!(wa::ContextInfo {
            stanza_id: s("ORIG1"),
        })
    };
    let text_reply = || {
        make!(ExtendedTextMessage {
            text: s("yes"),
            context_info: ctx().into(),
        })
    };
    let cases: Vec<(&str, wa::Message, &str)> = vec![
        ("plain text", conversation("hi"), ""),
        (
            "text reply",
            make!(wa::Message {
                extended_text_message: text_reply().into(),
            }),
            "ORIG1",
        ),
        (
            "mention only, no reply",
            make!(wa::Message {
                extended_text_message: make!(ExtendedTextMessage {
                    text: s("@1"),
                    context_info: make!(wa::ContextInfo {
                        mentioned_jid: vec!["1@s.whatsapp.net".into()],
                    })
                    .into(),
                })
                .into(),
            }),
            "",
        ),
        (
            "image reply",
            make!(wa::Message {
                image_message: make!(ImageMessage {
                    context_info: ctx().into(),
                })
                .into(),
            }),
            "ORIG1",
        ),
        (
            "sticker reply",
            make!(wa::Message {
                sticker_message: make!(StickerMessage {
                    context_info: ctx().into(),
                })
                .into(),
            }),
            "ORIG1",
        ),
        (
            "location reply",
            make!(wa::Message {
                location_message: make!(LocationMessage {
                    context_info: ctx().into(),
                })
                .into(),
            }),
            "ORIG1",
        ),
        (
            "inside ephemeral envelope",
            make!(wa::Message {
                ephemeral_message: wrap(make!(wa::Message {
                    extended_text_message: text_reply().into(),
                }))
                .into(),
            }),
            "ORIG1",
        ),
        (
            "document with caption",
            make!(wa::Message {
                document_with_caption_message: wrap(make!(wa::Message {
                    document_message: make!(DocumentMessage {
                        context_info: ctx().into(),
                    })
                    .into(),
                }))
                .into(),
            }),
            "ORIG1",
        ),
    ];
    for (name, msg, want) in cases {
        assert_eq!(quoted_message_id(&msg), want, "{name}");
    }
}
