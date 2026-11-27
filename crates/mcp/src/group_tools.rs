//! get_group_info (issue #109): a group's name, description and members.
//! `messages.db` has only the group's name, so this read tool asks WhatsApp
//! through the bridge (`/api/group`) and adds the members' names from the
//! store. The name, description and member names come from other people
//! and are marked like message text.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use whatsapp_mcp_store::{Db, Store, Timestamp};

use crate::args::Args;
use crate::bridge::{Answer, Bridge};
use crate::output::{Output, ToolError};
use crate::send_tools::bridge_error;
use crate::untrusted::{mark_untrusted, one_line};

/// The group as the bridge answers it (`crates/bridge/src/api.rs`).
#[derive(Deserialize)]
struct BridgeGroup {
    jid: String,
    subject: Option<String>,
    description: Option<String>,
    creation_time: Option<i64>,
    announce: bool,
    locked: bool,
    participant_count: Option<u32>,
    participants: Vec<BridgeMember>,
}

#[derive(Deserialize)]
struct BridgeMember {
    jid: String,
    phone_number: Option<String>,
    lid: Option<String>,
    role: String,
    is_me: bool,
}

/// The tool's result.
#[derive(Serialize)]
struct GroupInfo {
    jid: String,
    name: Option<String>,
    description: Option<String>,
    creation_time: Option<Timestamp>,
    /// Only admins may send messages.
    announce: bool,
    /// Only admins may change the group's name, description and picture.
    locked: bool,
    participant_count: Option<u32>,
    participants: Vec<Member>,
}

#[derive(Serialize)]
struct Member {
    jid: String,
    name: Option<String>,
    phone_number: Option<String>,
    lid: Option<String>,
    role: String,
    is_me: bool,
}

pub fn get_group_info(
    store: &Store,
    bridge: &Bridge,
    args: &Args<'_>,
) -> Result<Output, ToolError> {
    let chat_jid = args.string("chat_jid")?;
    if !chat_jid.ends_with("@g.us") {
        return Err(ToolError(format!(
            "{chat_jid} is not a group: a group's JID ends in @g.us"
        )));
    }
    let group = ask_bridge(bridge, &chat_jid).map_err(|reason| {
        log::warn!("Group info failed: {reason}");
        ToolError(format!("Failed to get group info: {reason}"))
    })?;
    // Names are a help, not the answer: without the database, members are
    // shown by JID only.
    let db = store
        .open()
        .inspect_err(|e| log::warn!("No member names: {e}"))
        .ok();
    let participants = group
        .participants
        .into_iter()
        .map(|m| Member {
            name: db.as_deref().and_then(|db| member_name(db, &m)),
            jid: m.jid,
            phone_number: m.phone_number,
            lid: m.lid,
            role: m.role,
            is_me: m.is_me,
        })
        .collect();
    Output::object(&GroupInfo {
        jid: group.jid,
        name: group.subject.as_deref().map(one_line),
        description: group.description.as_deref().map(mark_untrusted),
        creation_time: group.creation_time.and_then(Timestamp::from_unix),
        announce: group.announce,
        locked: group.locked,
        participant_count: group.participant_count,
        participants,
    })
}

/// The group, or why the bridge did not give it.
fn ask_bridge(bridge: &Bridge, chat_jid: &str) -> Result<BridgeGroup, String> {
    match bridge.post("group", &json!({ "chat_jid": chat_jid })) {
        Answer::Reply {
            status: 200,
            json: Some(json),
            ..
        } if json.get("success").and_then(Value::as_bool) == Some(true) => {
            serde_json::from_value(Value::Object(json))
                .map_err(|e| format!("the bridge's answer could not be read: {e}"))
        }
        Answer::Reply { status, json, text } => {
            let reason = bridge_error(status, json.as_ref(), &text);
            // The bridge's own message already says what failed.
            Err(reason
                .strip_prefix("Failed to get group info: ")
                .map_or_else(|| reason.clone(), str::to_string))
        }
        Answer::Timeout => Err("timed out waiting for the WhatsApp bridge".to_string()),
        Answer::Failed(e) => Err(format!(
            "cannot reach the WhatsApp bridge, which this tool needs: {e}"
        )),
    }
}

/// A member's name from the store: tried by phone number, then by the JID
/// the group uses, then by LID. `None` when none of them has a name.
fn member_name(db: &Db, member: &BridgeMember) -> Option<String> {
    [
        member.phone_number.as_deref(),
        Some(member.jid.as_str()),
        member.lid.as_deref(),
    ]
    .into_iter()
    .flatten()
    .find_map(|jid| {
        let name = db.sender_name(jid);
        (name != jid).then(|| one_line(&name))
    })
}
