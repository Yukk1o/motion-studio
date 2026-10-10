//! Read-only source preparation for authoring tools. Runs on the Session owner.
use crate::{
    preview_inputs::{input_choices, InputPoll, InputRequest},
    session::{with_session, Result},
};
use serde_json::{json, Value};

pub fn choices(id: i64) -> Result<Value> {
    with_session(id, |session| {
        Ok(
            json!({"version":1,"revision":session.engine.revision(),"inputs":input_choices(session.engine.project()),"stages":["original"],"max_edge":motion_render::image_resources::MAX_PREVIEW_EDGE,"working_space":"linear","alpha_mode":"premultiplied"}),
        )
    })
}
pub fn request(id: i64, text: &str, expected_revision: u64) -> Result<Value> {
    let request: InputRequest = serde_json::from_value(crate::ops::parse_request(
        text,
        16 * 1024,
        "preview input request",
    )?)
    .map_err(|error| error.to_string())?;
    with_session(id, |session| {
        if session.engine.gesture_active() {
            return Err("finish the active editor gesture before preparing preview input".into());
        }
        if session.engine.revision() != expected_revision {
            return Err("preview input project revision changed".into());
        }
        let info = session.preview_inputs.request(
            session.engine.project(),
            &session.root,
            expected_revision,
            request,
        )?;
        Ok(json!({"version":1,"state":"requested","input":info}))
    })
}
/// Ready packets preserve native YUV/Arc image data. Upload them on the device
/// owner using PreparedInput::upload and use the matching clock/size contract.
pub fn poll(id: i64, sequence: u64) -> Result<InputPoll> {
    with_session(id, |session| {
        if session.engine.gesture_active() {
            return Err("preview input is suspended during the active editor gesture".into());
        }
        session
            .preview_inputs
            .poll(sequence, session.engine.revision())
    })
}
pub fn cancel(id: i64) -> Result<Value> {
    with_session(id, |session| {
        session.preview_inputs.cancel();
        Ok(json!({"cancelled":true}))
    })
}
/// Resolve and upload together on the owner thread, preventing stale packets
/// from being installed after another editor command changes the revision.
pub fn upload(
    id: i64,
    sequence: u64,
    target: &mut motion_render::preview_input::PreviewInputTexture,
) -> Result<Value> {
    match poll(id, sequence)? {
        InputPoll::Pending(info) => Ok(json!({"version":1,"state":"pending","input":info})),
        InputPoll::Ready(packet) => {
            packet.upload(target)?;
            Ok(json!({"version":1,"state":"ready","input":packet.info}))
        }
    }
}
